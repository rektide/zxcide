#!/usr/bin/env python3
"""Extract original width-six runs from the pinned C7 literal-section index."""
from collections import deque
from pathlib import Path
from struct import pack
import sys


def runs(section, n):
    # Canonical trie, as in the attribution census. Leaf symbols are ~symbol.
    lengths = sorted((d, 2*i+j) for i, b in enumerate(section[:128])
                     for j, d in enumerate((b & 15, b >> 4)) if d)
    tree = [[None, None]]
    code = prev = 0
    for d, symbol in lengths:
        code <<= d-prev
        node = 0
        for shift in range(d-1, 0, -1):
            branch = code >> shift & 1
            if tree[node][branch] is None:
                tree[node][branch] = len(tree)
                tree.append([None, None])
            node = tree[node][branch]
        assert tree[node][code & 1] is None
        tree[node][code & 1] = ~symbol
        code += 1
        prev = d
    height = [None]*len(tree)
    for i in reversed(range(len(tree))):
        a, b = (None if j is None else 0 if j < 0 else height[j] for j in tree[i])
        if a is not None and a == b:
            height[i] = a+1

    q = deque([(0, n)])
    off = 128
    while q:
        i, n = q.popleft()
        if not n or i < 0:
            continue
        h = height[i]
        size = (n*(h or 1)+7)//8
        run = section[off:off+size]
        assert len(run) == size
        off += size
        if h == 6:
            table = bytearray(64)
            for j in range(64):
                k = i
                for shift in range(6):
                    k = tree[k][j >> shift & 1]
                table[j] = ~k
            yield n, table, run
        elif not h:
            ones = int.from_bytes(run, 'little').bit_count()
            if n % 8:
                ones -= (run[-1] >> (n % 8)).bit_count()
            assert 0 <= ones <= n
            q.extend(zip(tree[i], (n-ones, ones)))
    assert off == len(section)


def extract(archive, index, dest):
    data = archive.read_bytes()
    ns = []
    with dest.open('wb') as out:
        for row in index.read_text().splitlines():
            off, length, count = map(int, row.split())
            for n, table, run in runs(memoryview(data)[off:off+length], count):
                out.write(pack('<I', n) + table + run)
                ns.append(n)
    return {'count': len(ns), 'units': sum(ns), 'min': min(ns), 'max': max(ns)}


if __name__ == '__main__':
    print(extract(*map(Path, sys.argv[1:])))
