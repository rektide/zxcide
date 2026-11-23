#!/usr/bin/env python3
"""Both skew directions, including draining a 40-byte child at the front."""
import hashlib, json, random, struct
from prepare import S

out=S/'runs'; meta=json.loads((out/'meta.json').read_text())
for side in ('l','r'):
    for shape in ('early','sparse'):
        name=f'skew-{side}-{shape}'; rng=random.Random(13271)
        digest=hashlib.sha256(); packed=bytearray(); units=bulk=0
        for i,n in enumerate([257,513,1024,4097]*32):
            # For sparse skew use only 1/32 minority, <=31 on short nodes.
            small=list(range(40)) if shape=='early' else list(range(i%32,n,32))
            flags=bytearray(n); flags[:]=bytes([side=='l'])*n
            for j in small: flags[j] ^= 1
            ones=sum(flags); l=n-ones
            outbytes=rng.randbytes(n)
            src=bytes(b for b,f in zip(outbytes,flags) if not f)+bytes(b for b,f in zip(outbytes,flags) if f)
            bits=bytearray((n+7)//8)
            for j,f in enumerate(flags): bits[j//8] |= f << (j%8)
            if n%8: bits[-1] |= (255<<(n%8))&255
            packed+=struct.pack('<IIII',n,l,i,0)+bits+src+rng.randbytes(32)
            digest.update(outbytes); units+=n
            for j in range(0,n//32*4,4):
                if l<8 or ones<32: break
                bulk+=32; ones-=int.from_bytes(bits[j:j+4],'little').bit_count()
        (out/f'{name}.bin').write_bytes(packed)
        meta[name]={'count':128,'units':units,'bulk':bulk,'suffix':units-bulk,
                    'input':{'bytes':len(packed),'sha256':hashlib.sha256(packed).hexdigest()},
                    'expected':{'bytes':units,'sha256':digest.hexdigest()}}
(out/'meta.json').write_text(json.dumps(meta,indent=2)+'\n')
