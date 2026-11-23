#!/usr/bin/env python3
"""Check Tracy 0.14.1 unwrapped event CSVs against the pinned boundary map."""
import argparse
from collections import Counter, defaultdict
import csv
from dataclasses import dataclass, field
import json
from pathlib import Path
from statistics import median

from common import HERE, PINS, require

MAP = json.loads((HERE / 'boundaries.json').read_text())
IDS = MAP['ids']
require(MAP['schema'] == PINS['schema'] and MAP['c']['tree'] == PINS['c']['tree'] and
        MAP['rust']['revision'] == PINS['rust']['spans_revision'], 'boundary map pin mismatch')


@dataclass
class Event:
    name: str
    file: str
    line: int
    start: int
    end: int
    semantic: str | None = None
    children: list = field(default_factory=list)


def verify(path, impl, case, plain_bytes=None, copies=1):
    columns = {'name', 'src_file', 'src_line', 'ns_since_start', 'exec_time_ns', 'thread'}
    lanes = defaultdict(list)
    with path.open(newline='') as stream:
        rows = csv.DictReader(stream, strict=True)
        require(columns <= set(rows.fieldnames or []), 'missing event CSV columns')
        for r in rows:
            require(None not in r and all(r[c] is not None for c in columns), f'malformed row {r}')
            start = int(r['ns_since_start'])
            duration = int(r['exec_time_ns'])
            require(duration >= 0 and r['thread'], f'open or malformed zone {r}')
            lanes[r['thread']].append(Event(r['name'], r['src_file'], int(r['src_line']),
                                             start, start + duration))
    durations = {key: [] for key in [*IDS, 'spl.prepare', 'spl.tail']}
    unmapped = Counter()
    checked_lanes = 0
    sites = MAP[impl]['sites']
    for lane, events in lanes.items():
        stack = []
        observed_checked = False
        for e in sorted(events, key=lambda e: (e.start, -e.end)):
            while stack and stack[-1].end <= e.start:
                stack.pop()
            require(not stack or e.end <= stack[-1].end, f'crossing intervals on lane {lane}: {e.name}')
            parent = next((s for s in reversed(stack) if s.semantic), None)
            for name, file, line, semantic, expected_parent, *alias in sites:
                if name != e.name or not (e.file == file or e.file.endswith('/' + file)) or line != e.line:
                    continue
                if alias and (not parent or parent.semantic != expected_parent):
                    break
                require((parent.semantic if parent else None) == expected_parent,
                        f'{semantic} has wrong same-lane parent on {lane}')
                e.semantic = semantic
                durations[semantic].append(e.end - e.start)
                if parent:
                    parent.children.append(e)
                observed_checked |= semantic == 'block.checked'
                break
            if not e.semantic:
                require(e.name not in {site[0] for site in sites if len(site) == 5},
                        f'unrecognized site for {e.name}: {e.file}:{e.line}')
                unmapped[e.name] += 1
            stack.append(e)
        checked_lanes += observed_checked
        for e in events:
            child_ids = [child.semantic for child in e.children]
            if e.semantic == 'block.checked':
                expected = ['spl.payload'] if case == 'bad-reconstruct' else ['spl.payload', 'block.hash']
                require(child_ids == expected, f'wrong payload/hash cardinality or order on {lane}: {child_ids}')
            if e.semantic == 'spl.payload':
                possible = ['huf.inline.literal', 'huf.inline.token']
                require(child_ids == [x for x in possible if x in child_ids] + ['spl.reconstruct'],
                        f'wrong Huffman/reconstruction cardinality or order on {lane}: {child_ids}')
                for left, right in zip(e.children, e.children[1:]):
                    require(left.end <= right.start, 'overlapping SPL siblings')
                recon = e.children[-1]
                durations['spl.prepare'].append(recon.start - e.start)
                durations['spl.tail'].append(e.end - recon.end)
    counts = [len(durations[key]) for key in IDS]
    expected = [n * copies for n in MAP['cases'][case]]
    require(counts == expected, f'{impl}/{case}: expected {expected}, exported {counts}')

    def summary(values):
        if not values:
            return {'count': 0}
        out = {'count': len(values), 'elapsed_ns_sum': sum(values), 'elapsed_ns_min': min(values),
               'elapsed_ns_median': median(values), 'elapsed_ns_max': max(values)}
        if plain_bytes:
            out['elapsed_s_per_plaintext_GB'] = sum(values) / plain_bytes
        return out

    return {'stages': {key: summary(values) for key, values in durations.items()},
            'checked_lanes': checked_lanes, 'event_lanes': len(lanes), 'unmapped': dict(unmapped)}


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('impl', choices=('c', 'rust'))
    p.add_argument('case', choices=MAP['cases'])
    p.add_argument('csv', type=Path)
    args = p.parse_args()
    print(json.dumps(verify(args.csv, args.impl, args.case), indent=2))
