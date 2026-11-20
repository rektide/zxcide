#!/usr/bin/env python3
"""Pinned, cross-verified Silesia CLI scorecard; Python standard library only."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import subprocess
import sys
import tempfile
import time
import urllib.request
import zipfile

URL = 'https://raw.githubusercontent.com/DataCompression/corpus-collection/d59143a41cc7d65063a70e86063a3c39aeae6d9f/Silesia-Corpus/silesia.zip'
ZIP_SHA = '1b39576418fa3ebb09cf0ceab59bf1cb9fa262c19d9dc6d3cda1e4fae7910a09'
FILES = ('dickens', 'mozilla', 'mr', 'nci', 'ooffice', 'osdb', 'reymont', 'samba', 'sao', 'webster', 'xml', 'x-ray')


def sha(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()


def invoke(command, output):
    with open(output, 'wb') as out:
        subprocess.run(command, stdout=out, check=True)


def verified_zip(path):
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists():
        # Stage beside the destination for an atomic, same-FS rename.
        with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as tmp:
            name = Path(tmp.name)
        try:
            with urllib.request.urlopen(URL, timeout=120) as response, name.open('wb') as out:
                while block := response.read(1024 * 1024):
                    out.write(block)
            if sha(name) != ZIP_SHA:
                raise ValueError(f'downloaded corpus ZIP checksum mismatch: {path}')
            os.replace(name, path)
        finally:
            name.unlink(missing_ok=True)
    if sha(path) != ZIP_SHA:
        raise ValueError(f'corpus ZIP checksum mismatch; refusing cached file: {path}')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--c-cli', type=Path, required=True)
    p.add_argument('--rust-cli', type=Path, required=True)
    p.add_argument('--c-build', required=True, help='revision, compiler, and flags of supplied binary')
    p.add_argument('--rust-build', required=True, help='revision, compiler, and flags of supplied binary')
    p.add_argument('--corpus', type=Path, default=Path('target/scorecard/silesia.zip'))
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--full', action='store_true', help='all 12 files, default dickens and sao')
    p.add_argument('--samples', type=int, default=7)
    p.add_argument('--warmups', type=int, default=1)
    p.add_argument('--seed', type=int, default=42)
    a = p.parse_args()
    if a.samples < 1 or a.warmups < 0:
        p.error('samples must be positive and warmups nonnegative')
    c, rust = str(a.c_cli.resolve(strict=True)), str(a.rust_cli.resolve(strict=True))
    verified_zip(a.corpus)
    cpu = platform.processor()
    if Path('/proc/cpuinfo').exists():
        cpu = next((line.split(':', 1)[1].strip() for line in Path('/proc/cpuinfo').read_text().splitlines() if line.startswith('model name')), cpu)
    result = {'schema': 1, 'timestamp_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'argv': sys.argv, 'platform': platform.platform(), 'cpu': cpu,
              'affinity': sorted(os.sched_getaffinity(0)) if hasattr(os, 'sched_getaffinity') else None,
              'corpus_url': URL, 'corpus_zip_sha256': ZIP_SHA, 'samples': a.samples,
              'warmups': a.warmups, 'seed': a.seed,
              'binaries': {'c': {'path': c, 'sha256': sha(c), 'build': a.c_build},
                           'rust': {'path': rust, 'sha256': sha(rust), 'build': a.rust_build}}, 'files': {}}
    rng = random.Random(a.seed)
    a.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='scorecard-', dir=a.output.parent) as scratch, zipfile.ZipFile(a.corpus) as z:
        work = Path(scratch)
        names = FILES if a.full else ('dickens', 'sao')
        if sorted(z.namelist()) != sorted(FILES):
            raise ValueError('unexpected ZIP member set')
        for name in names:
            print('scorecard:', name, file=sys.stderr, flush=True)
            plain = work / name
            # Fixed member names, never extract arbitrary ZIP paths.
            with z.open(name) as src, plain.open('wb') as dst:
                while block := src.read(1024 * 1024):
                    dst.write(block)
            plain_hash = sha(plain)
            entry = {'bytes': plain.stat().st_size, 'sha256': plain_hash, 'archives': {}}
            result['files'][name] = entry
            encoders = {'c3': [c, '-qzc', '-T1', '-3', str(plain)],
                        'c7': [c, '-qzc', '-T1', '-7', str(plain)],
                        'rust_split': [rust, 'encode', '--mode', 'split', str(plain)],
                        'rust_dense': [rust, 'encode', '--mode', 'dense', str(plain)]}
            jobs = []
            for label, command in encoders.items():
                archive = work / (name + '.' + label + '.zxc')
                invoke(command, archive)
                info = {'bytes': archive.stat().st_size, 'ratio_archive_over_plain': archive.stat().st_size / plain.stat().st_size,
                        'sha256': sha(archive), 'encode_command': command, 'encode_ms': [], 'decode': {}}
                entry['archives'][label] = info
                jobs.append((command, info['encode_ms']))
                for decoder, dec_cmd in {'c': [c, '-qdc', '-T1', str(archive)], 'rust': [rust, 'decode', str(archive)]}.items():
                    verified = work / 'verified'
                    invoke(dec_cmd, verified)
                    # byte-for-byte comparison, not solely a digest comparison
                    with plain.open('rb') as left, verified.open('rb') as right:
                        while True:
                            x, y = left.read(1024 * 1024), right.read(1024 * 1024)
                            if x != y:
                                raise ValueError(f'{name}/{label}/{decoder}: decoded bytes mismatch')
                            if not x:
                                break
                    d = {'command': dec_cmd, 'verified_exact': True, 'ms': []}
                    info['decode'][decoder] = d
                    jobs.append((dec_cmd, d['ms']))
            # Shuffle all operating points together per pass; no digest/check inside timing.
            for iteration in range(a.warmups + a.samples):
                rng.shuffle(jobs)
                for command, samples in jobs:
                    start = time.perf_counter_ns()
                    invoke(command, os.devnull)
                    elapsed = (time.perf_counter_ns() - start) / 1e6
                    if iteration >= a.warmups:
                        samples.append(elapsed)
            for info in entry['archives'].values():
                for samples in [info['encode_ms'], *(v['ms'] for v in info['decode'].values())]:
                    assert len(samples) == a.samples
        # Write only complete, fully verified runs.
        a.output.write_text(json.dumps(result, indent=2) + '\n')
    print(a.output)


if __name__ == '__main__':
    main()
