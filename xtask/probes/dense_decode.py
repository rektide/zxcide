#!/usr/bin/env python3
"""Compare CLI decoders on exactly the Dense archives in a frozen scorecard."""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import random
import statistics
import subprocess
import sys
import time


def digest(stream):
    return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for arg in ('scorecard', 'archives', 'before', 'after', 'c_cli'):
        parser.add_argument(arg, type=Path)
    parser.add_argument('--passes', type=int, default=7)
    args = parser.parse_args()
    if args.passes < 1:
        parser.error('--passes must be positive')
    jobs = {}
    for name, entry in json.loads(args.scorecard.read_text())['files'].items():
        path = args.archives / (name + '.zxc')
        with path.open('rb') as src:
            if digest(src) != entry['archives']['rust_dense']['sha256']:
                sys.exit(f'wrong archive: {path}')
        for label, cli, flags in [('before', args.before, ['decode']), ('after', args.after, ['decode']),
                                  ('C', args.c_cli, ['-qdc', '-T1'])]:
            cmd = [cli.resolve(strict=True), *flags, path]
            # Verify the decoded plaintext outside the timed loop. Stream
            # the digest to avoid keeping a corpus member in working memory.
            with subprocess.Popen(cmd, stdout=subprocess.PIPE) as proc:
                result = digest(proc.stdout)
                if proc.wait() or result != entry['sha256']:
                    sys.exit(f'bad decode: {name} / {label}')
            jobs[name, label] = (cmd, [])
    rng = random.Random(409)
    order = list(jobs)
    writer = csv.writer(sys.stdout, delimiter='\t', lineterminator='\n')
    writer.writerow(['pass', 'archive', 'decoder', 'elapsed_ms'])
    for iteration in range(args.passes + 1):
        rng.shuffle(order)
        for key in order:
            cmd, values = jobs[key]
            start = time.perf_counter_ns()
            subprocess.run(cmd, stdout=subprocess.DEVNULL, check=True)
            elapsed = (time.perf_counter_ns() - start) / 1e6
            if iteration:
                values.append(elapsed)
                writer.writerow([iteration, *key, f'{elapsed:.3f}'])
    for key, (_, values) in jobs.items():
        print(*key, f'{statistics.median(values):.3f} ms', file=sys.stderr)


if __name__ == '__main__':
    main()
