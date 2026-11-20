#!/usr/bin/env python3
"""Run only under the repository's exclusive .test-agent/zxc-perf.lock."""

import argparse
import csv
from contextlib import contextmanager
from datetime import datetime, timezone
import hashlib
from itertools import permutations
import os
from pathlib import Path
from random import Random
import shutil
import struct
import subprocess
import tempfile
from time import perf_counter_ns


PCTS = (0, 1, 2, 4)
DECODE_SEED = 0x514C2026
ENCODE_SEED = 0x93D02026
ENCODE_PASSES = 7
MANIFEST = Path(__file__).with_name('plain.sha256')
DECODE_CPUS = set(range(8))  # one hardware thread per physical core on the 5800X
ENCODE_CPUS = {2}


@contextmanager
def table(path, columns):
    with path.open('x', buffering=1) as out:
        writer = csv.writer(out, delimiter='\t', lineterminator='\n')
        writer.writerow(columns.split())
        yield writer.writerow


def sha256(path):
    with path.open('rb') as fd:
        return hashlib.file_digest(fd, 'sha256').hexdigest()


def concat(inputs, output):
    with output.open('xb') as out:
        for path in inputs:
            with path.open('rb') as fd:
                shutil.copyfileobj(fd, out)


def sample(argv, work, *, cpus, stdout=subprocess.DEVNULL, reference=None):
    os.sched_setaffinity(0, cpus)
    with tempfile.TemporaryFile(dir=work) as err:
        start = perf_counter_ns()
        proc = subprocess.Popen(argv, stdout=stdout if reference is None else subprocess.PIPE, stderr=err)
        if reference is not None:
            with reference.open('rb') as plain, proc.stdout as decoded:
                while data := plain.read(1 << 20):
                    if decoded.read(len(data)) != data:
                        raise ValueError(f'output mismatch: {argv}')
                if decoded.read(1):
                    raise ValueError(f'output exceeds expected length: {argv}')
        _, status, usage = os.wait4(proc.pid, 0)
        stop = perf_counter_ns()
        proc.returncode = os.waitstatus_to_exitcode(status)
        err.seek(0)
        stderr = err.read()
    if proc.returncode or stderr:
        raise RuntimeError((argv, proc.returncode, stderr[:1024]))
    return (stop - start) / 1e6, usage.ru_utime * 1e3, usage.ru_stime * 1e3


def blocks(frame):
    # Exactly one checksummed 512 KiB frame, no dictionary or seek table.
    assert frame[:4] == struct.pack('<I', 0x9cb02ef5) and frame[4:7] == bytes([9, 19, 128])
    pos = 16
    while frame[pos] != 255:
        kind = frame[pos]
        length = struct.unpack_from('<I', frame, pos + 3)[0]
        body = frame[pos + 8:pos + 8 + length]
        check = frame[pos + 8 + length:pos + 12 + length]
        pos += 12 + length
        if kind == 0:
            yield (kind, len(body), 0, 0, 0, 0, body, b'', check)
            continue
        assert kind == 1
        ns, nl, lm, tm, reserved, off = struct.unpack_from('<IIBBBB', body)
        ls = nl
        ts = ns
        descriptors_end = 12
        if lm:
            ls = struct.unpack_from('<I', body, descriptors_end)[0]
            descriptors_end += 4
        if tm:
            ts = struct.unpack_from('<I', body, descriptors_end)[0]
            descriptors_end += 4
        literals_end = descriptors_end + ls
        assert literals_end + ts <= len(body)
        yield (kind, nl, ns, lm, tm, off, body[descriptors_end:literals_end],
               body[literals_end:], check)


def census(archives, names, forms, work):
    with (work / 'blocks.tsv').open('x') as file:
        out = csv.writer(file, delimiter='\t', lineterminator='\n')
        out.writerow('file block sequences literals old_mode new_mode threshold old_wire new_wire'.split())
        for name in names:
            baseline = list(blocks(archives[0, name].read_bytes()))
            for pct in PCTS:
                candidate = blocks(archives[pct, name].read_bytes())
                for i, (old, new) in enumerate(zip(baseline, candidate, strict=True)):
                    kind, nl, ns, lm, tm, off, lit, tail, check = old
                    kn, nln, nsn, lmn, tmn, offn, litn, tailn, checkn = new
                    assert (kind, nl, ns, tm, off, tail, check) == (kn, nln, nsn, tmn, offn, tailn, checkn)
                    if kind == 0:
                        assert lit == litn
                        continue
                    old_wire, new_wire = len(lit) + 4 * bool(lm), len(litn) + 4 * bool(lmn)
                    if lm == lmn:
                        assert lit == litn
                        if lm == 2:
                            assert (nl - old_wire) * 100 >= nl * pct
                    else:
                        # This is the only transition observed in this no-dict sweep.
                        assert (lm, lmn) == (2, 0)
                        assert (nl - old_wire) * 100 < nl * pct
                    out.writerow((name, i, ns, nl, lm, lmn, pct, old_wire, new_wire))
    with table(work / 'sizes.tsv', 'file threshold plaintext_bytes archive_bytes plaintext_sha256 archive_sha256') as sizes:
        for name in (*names, *forms):
            plain = forms[name] if name in forms else forms['all'].parent / name
            plain_hash = sha256(plain)
            for pct in PCTS:
                archive = archives[pct, name]
                sizes((name, pct, plain.stat().st_size, archive.stat().st_size, plain_hash, sha256(archive)))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('scratch', type=Path)
    parser.add_argument('plaintext_dir', type=Path)
    args = parser.parse_args()
    scratch = args.scratch.resolve()
    work = scratch / 'run'
    work.mkdir()
    plain = work / 'plain'
    plain.mkdir()
    names = []
    for line in MANIFEST.read_text().splitlines():
        expected, name = line.split()
        shutil.copyfile(args.plaintext_dir / name, plain / name)
        assert sha256(plain / name) == expected, name
        names.append(name)
    assert names == sorted(set(names)) and len(names) == 12
    forms = {name: plain / name for name in ('all', 'without-sao')}
    concat((plain / name for name in names), forms['all'])
    concat((plain / name for name in names if name != 'sao'), forms['without-sao'])
    bins = scratch / 'bin'
    archives = {(pct, name): work / f'{name}-{pct}.zxc' for pct in PCTS for name in (*names, *forms)}

    def encode(pct, name):
        return [bins / f'rust-{pct}', 'encode', '--mode', 'dense', '--threads', '1', '--block-log', '19', plain / name]

    def decode(decoder, workers, form, pct):
        path = archives[pct, form]
        if decoder == 'rust':
            return [bins / 'rust-0', 'decode', '--threads', str(workers), path]
        return [bins / 'c', '-d', '-c', f'-T{workers}', '--progress', 'never', path]

    def log(message):
        print(datetime.now(timezone.utc).isoformat(), message, flush=True)

    log('encode fixtures from frozen plaintexts (untimed warmup)')
    for pct in PCTS:
        for name in names:
            with archives[pct, name].open('xb') as out:
                sample(encode(pct, name), work, cpus=ENCODE_CPUS, stdout=out)
        for form in forms:
            concat((archives[pct, name] for name in names if form == 'all' or name != 'sao'), archives[pct, form])
    census(archives, names, forms, work)

    scenarios = [(decoder, workers, 'all') for decoder in ('rust', 'c') for workers in (1, 4, 8)]
    scenarios += [(decoder, 1, form) for decoder in ('rust', 'c') for form in ('without-sao', 'sao')]
    with (work / 'verification.tsv').open('x', buffering=1) as file:
        out = csv.writer(file, delimiter='\t', lineterminator='\n')
        out.writerow('decoder workers form threshold bytes exact_bytes stderr_bytes exit'.split())
        for decoder, workers, form in scenarios:
            for pct in PCTS:
                sample(decode(decoder, workers, form, pct), work, cpus=DECODE_CPUS, reference=plain / form)
                out.writerow((decoder, workers, form, pct, (plain / form).stat().st_size, 'yes', 0, 0))
    log('40 exact decode checks passed; all nonliteral block contents preserved')

    with (work / 'decode.tsv').open('x', buffering=1) as file:
        out = csv.writer(file, delimiter='\t', lineterminator='\n')
        out.writerow('round decoder workers form position threshold wall_ms user_ms sys_ms'.split())
        rng = Random(DECODE_SEED)
        orders = {case: list(permutations(PCTS)) for case in scenarios}
        for order in orders.values():
            rng.shuffle(order)
        rounds = len(next(iter(orders.values())))
        for case in rng.sample(scenarios, len(scenarios)):
            for pct in rng.sample(PCTS, len(PCTS)):
                sample(decode(*case, pct), work, cpus=DECODE_CPUS)
        for trial in range(rounds):
            rng.shuffle(scenarios)
            for case in scenarios:
                for pos, pct in enumerate(orders[case][trial], 1):
                    times = sample(decode(*case, pct), work, cpus=DECODE_CPUS)
                    out.writerow((trial + 1, *case, pos, pct, *(f'{t:.6f}' for t in times)))
    log(f'{rounds} complete decode rounds')

    with (work / 'encode.tsv').open('x', buffering=1) as file:
        out = csv.writer(file, delimiter='\t', lineterminator='\n')
        out.writerow('round file position threshold wall_ms user_ms sys_ms'.split())
        rng = Random(ENCODE_SEED)
        for trial in range(1, ENCODE_PASSES + 1):
            order = rng.sample(names, len(names))
            for pos, pct in enumerate(rng.sample(PCTS, len(PCTS)), 1):
                for name in order:
                    times = sample(encode(pct, name), work, cpus=ENCODE_CPUS)
                    out.writerow((trial, name, pos, pct, *(f'{t:.6f}' for t in times)))
            log(f'encode round {trial}/{ENCODE_PASSES}')
    log('complete; every timed subprocess returned 0 with empty stderr')


if __name__ == '__main__':
    main()
