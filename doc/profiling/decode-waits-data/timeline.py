#!/usr/bin/env python3
"""Print the largest exposed head (or most b wait) from a named sidecar."""
import argparse
from pathlib import Path

from reduce import read_trace, reduce_frame


def show(path):
    _, frames = read_trace(path)
    for f, frame in enumerate(frames):
        _, jobs, heads = reduce_frame(frame)
        if not heads:
            continue
        h = max(heads, key=lambda i: (heads[i]['exposed'], heads[i]['b']))
        j = jobs[h]
        start = j['compute']
        print(f'{path.name} frame={f} head={h} compute={j["compute_ns"] / 1e3:.1f} us '
              f'kind={j["kind"]} payload={j["payload_bytes"]} plain={j["plain_bytes"]}')
        print('  since head compute-start (µs):')
        edges = [(j[k], f'head {k}') for k in ('next_start', 'admit', 'compute', 'computed', 'result_got', 'commit', 'credit')]
        for k in jobs[h + 1:]:
            if k['computed'] > j['computed']:
                continue
            edges.append((k['computed'], f'later {k["index"]} computed (worker {k["worker"]})'))
            if k['pending'] < j['commit']:
                edges.append((k['pending'], f'later {k["index"]} inserted in pending'))
        for t, label in sorted(edges):
            print(f'    {(t - start) / 1e3:9.1f}  {label}')
        print('  receive intervals (µs) / exposed by idle worker count:')
        print('   ', ', '.join(f'{k}={v / 1e3:.1f}' for k, v in sorted(heads[h].items())))


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('trace', type=Path, nargs='+')
    for path in p.parse_args().trace:
        show(path)
