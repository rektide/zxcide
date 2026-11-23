#!/usr/bin/env python3
"""Extract real bottom-up gathers and verify their counts against the independent census."""
import argparse, json, hashlib, os, struct, subprocess as sp, sys
from collections import Counter
from pathlib import Path

from prepare import ROOT, S
sys.path.insert(0, str(ROOT/'doc/profiling/decode-replay-data'))
from perf import capture, identity, MANIFEST

def data():
    a = argparse.ArgumentParser(); a.add_argument('--fixtures', type=Path, default=ROOT/'.test-agent/decode-pool/fixtures')
    a = a.parse_args()
    out = S/'runs'; out.mkdir(exist_ok=True)
    meta = {}
    man = json.loads(MANIFEST.read_text())
    expected = {r['file']:r for r in man['members']}; expected['single.zxc'] = man['single']
    for name in ('single','mozilla','sao'):
        arc = a.fixtures/f'{name}.zxc'
        assert all(expected[arc.name][k] == v for k,v in identity(arc).items())
        _, cases = capture(arc,out)
        for kind, case in cases.items():
            case_name = f'{name}-{kind}'
            path = out/f'{case_name}.capture'
            idx = Path(case['index']['path'])
            indexed = iter(tuple(map(int, ln.split())) for ln in idx.read_text().splitlines())
            with path.open('wb') as f:
                sp.run([S/'capture',arc,idx], env=dict(os.environ,ZXC_MERGE_CAPTURE='1'),stdout=f,check=True)
            raw = memoryview(path.read_bytes()); o = 0
            packs = {case_name: (Counter(), hashlib.sha256())}
            if name == 'single':
                for dst in ('short','skew-l','skew-r'): packs[dst+'-'+kind] = (Counter(),hashlib.sha256())
            fs = {k:(out/f'{k}.bin').open('wb') for k in packs}
            seq = -1; previous = None
            try:
                while o < len(raw):
                    start,n,l = struct.unpack_from('<III',raw,o); o += 12
                    if start == 0xffffffff:
                        assert (n,l,struct.unpack_from('<I',raw,o)[0]) == next(indexed)
                        o += 4; seq += 1; previous = None; continue
                    size=(n+7)//8; r=n-l
                    mask=raw[o:o+size]; src=raw[o+size:o+size+n]; dst=raw[o+size+n:o+size+n+n]
                    assert len(dst) == n and 0 < l < n and seq >= 0
                    bits=int.from_bytes(mask,'little') & ((1<<n)-1)
                    assert bits.bit_count() == r
                    o += size+2*n
                    h = struct.pack('<IIII',n,l,seq,start)
                    guard = bytes((seq + 19*i)%256 for i in range(32))
                    choices = [case_name]
                    if name == 'single':
                        if n<128: choices.append('short-'+kind)
                        if l*16 <= n: choices.append('skew-l-'+kind)
                        if r*16 <= n: choices.append('skew-r-'+kind)
                    full=0; rem=r
                    if n >= 128 and l >= 8:
                        for j in range(0,n//32*4,4):
                            if rem < 32: break
                            full += 32
                            rem -= int.from_bytes(mask[j:j+4],'little').bit_count()
                    uniform_tiles = sum(int.from_bytes(mask[j:j+4],'little') in (0,0xffffffff) for j in range(0,n//32*4,4))
                    for k in choices:
                        f=fs[k]; f.write(h); f.write(mask); f.write(src); f.write(guard)
                        c,d = packs[k]; d.update(dst)
                        c.update(count=1, units=n, seen_below_32=n<32, seen_below_128=n<128,
                                 below_128_bytes=n if n<128 else 0, bulk=full, suffix=n-full,
                                 uniform_tiles=uniform_tiles, minority_l=l*16<=n, minority_r=r*16<=n)
                    previous = start
                assert seq+1 == case['count'] and next(indexed,None) is None
            finally:
                for f in fs.values(): f.close()
            for k,(c,d) in packs.items():
                if c['count']:
                    meta[k]={**c, 'index':case['index'], 'input':identity(out/f'{k}.bin'),
                             'expected':{'bytes':c['units'],'sha256':d.hexdigest()}}
            if case_name == 'single-lit': assert (packs[case_name][0]['count'], packs[case_name][0]['units'])==(4157,92741351)
            if case_name == 'single-tok': assert (packs[case_name][0]['count'], packs[case_name][0]['units'])==(5112,30483177)
            meta[case_name]['capture'] = identity(path)
            print(case_name,meta[case_name], flush=True)
    (out/'meta.json').write_text(json.dumps(meta,indent=2)+'\n')

if __name__=='__main__': data()
