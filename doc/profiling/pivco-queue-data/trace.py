#!/usr/bin/env python3
import csv
import gzip
from itertools import product
from pathlib import Path
import subprocess
from collections import defaultdict

w = Path(__file__).resolve().parent
variants = ['baseline', 'candidate']
cases = ['mixed12', 'mixed19', 'sao.dense', 'sao.c7', 'sao.split', 'silesia.dense',
         'silesia.c7', 'noise12', 'light12', 'raw-long', 'light-long', 'dict-cold', 'dict-many']
keys = ['count', 'cold', 'growth', 'reused', 'discarded_slots', 'retained_slots', 'frontier',
        'fifo_cap', 'order_len', 'order_cap', 'byte_events', 'zero', 'flat', 'gathered', 'returned',
        'out_cap', 'merge_cap']
sum_keys = ['cold', 'growth', 'reused', 'discarded_slots', 'byte_events', 'zero', 'flat',
            'gathered', 'returned', 'order_len', 'order_cap']
max_keys = ['retained_slots', 'frontier', 'fifo_cap', 'order_len', 'order_cap', 'meta_bytes']


def writer(f, header):
    table = csv.writer(f, delimiter='\t', lineterminator='\n')
    table.writerow(header)
    return table


with (w / 'allocations.tsv').open('w') as f, (w / 'worker-ends.tsv').open('w') as ends, \
     gzip.open(w / 'sections.tsv.gz', 'wt', compresslevel=9) as raw, \
     gzip.open(w / 'blocks.tsv.gz', 'wt', compresslevel=9) as blocks:
    first = ['archive', 'workers', 'variant']
    totals = writer(f, [*first, 'sections', 'order_allocs', 'fifo_events'] + [f'{k}_sum' for k in sum_keys]
                    + [f'{k}_max' for k in max_keys])
    sect = writer(raw, [*first, 'frame', 'worker', 'index', 'section', 'mode', *keys, 'meta_bytes'])
    headers = [*first, 'frame', 'worker', 'index', 'block_limit', 'kind',
               'literals_cap_B', 'tokens_cap_B', 'merge_cap_B', 'queue_cap_slots']
    blk = writer(blocks, [*headers, 'BlockDecoder_size_B'])
    end = writer(ends, headers)
    paired_semantics = {}
    for name, n in list(product(cases, (1, 4))) + [('downshift', 1)]:
        for variant in variants:
            args = [w / f'bin/diag-{variant}', 'decode', '--threads', str(n)]
            if name.startswith('dict-'):
                args += ['-D', w / 'fixtures/traversal.zxd']
            args += [w / f'fixtures/{name}.zxc']
            log = subprocess.run(args, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE).stderr.decode()
            frame = 0
            data, latest, where, sections = [], {}, {}, {}
            observed = []
            for row in log.splitlines():
                tag, who, *vals = row.split()
                pre = [name, n, variant]
                if tag == 'F':
                    for last in latest.values():
                        end.writerow(last)
                    latest.clear()
                    frame += 1
                    continue
                if tag == 'I':
                    index, _, _, decoder_size = map(int, vals)
                    where[who] = index, decoder_size
                elif tag in ('L', 'T'):
                    sections[who] = tag, int(vals[0])
                elif tag == 'S':
                    rec = dict(zip(keys, map(int, vals), strict=True))
                    rec['meta_bytes'] = 24 * rec['fifo_cap'] + 40 * rec['order_cap']
                    assert rec['retained_slots'] <= 16 and rec['frontier'] <= 256
                    assert rec['cold'] + rec['reused'] == 1
                    assert rec['gathered'] == rec['returned']
                    sect.writerow([*pre, frame, who, where[who][0], *sections[who], *rec.values()])
                    observed.append((frame, where[who][0], *sections[who], *[rec[k] for k in
                        ('count', 'flat', 'gathered', 'returned', 'frontier', 'order_len', 'order_cap')]))
                    data.append(rec)
                elif tag == 'B':
                    index, limit, kind, lit, tok, merge, queue = map(int, vals)
                    assert lit <= limit and merge <= limit and tok <= limit // 5 and queue <= 16
                    last = [*pre, frame, who, index, limit, kind, lit, tok, merge, queue]
                    blk.writerow([*last, where[who][1]])
                    latest[who] = last
                else:
                    raise ValueError(row)
            for last in latest.values():
                end.writerow(last)
            if variant == 'baseline':
                paired_semantics[name, n] = sorted(observed)
            else:
                assert paired_semantics[name, n] == sorted(observed)
            s = {k: sum(rec[k] for rec in data) for k in sum_keys}
            totals.writerow([*pre, len(data), len(data), s['cold'] + s['growth'], *s.values(),
                             *[max((rec[k] for rec in data), default=0) for k in max_keys]])
