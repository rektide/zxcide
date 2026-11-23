#!/usr/bin/env python3
from prepare import ROOT, S, CPIN, export
from pathlib import Path
import hashlib, os, shutil, subprocess as sp

src=S/'src-c'
if not src.is_dir():
    repo=Path.home()/'archive/hellobertrand/zxc'
    if sp.run(['git','-C',repo,'cat-file','-e',f'{CPIN}^{{tree}}'],stderr=sp.DEVNULL).returncode==0:
        export('c',CPIN,repo)
    else:
        shutil.copytree(ROOT/'.test-agent/decode-compact-sol/build/src-c',src)
lib=src/'src/lib'
orig=(lib/'zxc_huffman.c').read_text()
print('pinned source',CPIN,hashlib.sha256(orig.encode()).hexdigest(),flush=True)
first=orig.index('    while (i + 32 <= n) {',orig.index('    // Same thing at 256 bits,'))
last=orig.index('\n#else',first)
signature='static inline void bounded(uint8_t* RESTRICT out, const uint8_t* RESTRICT src, size_t nl, size_t nr, const uint8_t* RESTRICT bits) {\n'
loop=orig[first:last].replace('while (i + 32 <= n)', 'while (n >= 128 && nl >= 8 && i + 32 <= n && nr - rp >= 32)',1)
bounded=signature+'''    const uint8_t *L=src, *R=src+nl;
    size_t i=0, lp=0, rp=0, n=nl+nr;
'''+loop+'''
    for (;i<n;i++) out[i]=(bits[i>>3]>>(i&7))&1 ? R[rp++] : L[lp++];
}
'''
trial=S/'pinned-kernel.c'
trial.write_text(orig+'\n'+bounded+(S/'kernel.c').read_text())
cmd=['/usr/bin/cc','-std=gnu17','-O3','-DNDEBUG','-D_POSIX_C_SOURCE=200809L','-DZXC_STATIC_DEFINE',
     '-mavx2','-mbmi2','-mpopcnt','-ffunction-sections','-fdata-sections','-Wl,--gc-sections','-g',
     '-I'+str(src/'include'), '-I'+str(lib), '-I'+str(lib/'vendors'),str(trial),str(lib/'zxc_pivco_tables.c'),'-o',str(S/'kernel-c')]
print(*cmd,flush=True); sp.run(cmd,env={**os.environ,'TMPDIR':str(S/'tmp')},check=True)
