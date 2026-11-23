#!/usr/bin/env python3
"""Prepare clean source exports; all benchmark plumbing stays outside production."""
import subprocess as sp
from pathlib import Path

ROOT = Path.cwd()
S = Path(__file__).resolve().parent
R = ROOT / 'doc/profiling/decode-replay-data'
PIN = '477254fb0c93b6a4c54ef9e3ac2b2ad76496e574'
CPIN = '5719a96f778226868b29dba621a8c4e302b74d7c'

def export(arm, pin=PIN, repo=ROOT):
    dest = S/f'src-{arm}'
    dest.mkdir()
    with sp.Popen(['git','-C',repo,'archive',pin],stdout=sp.PIPE) as p:
        sp.run(['tar','-x','-C',dest],stdin=p.stdout,check=True)
        assert p.wait() == 0
    return dest

def production(src, arm):
    if arm in ('a', 'capture'): return
    variant = arm
    arm = 'avx' if arm == 'avx2' else arm
    root = src/'crates/codec/src/entropy'
    (root/'merge').mkdir()
    module = (S/f'{arm}.rs').read_text()
    if variant == 'avx2':
        module=module.replace('use super::{cursor, tiles32};','use super::{simd_packed, tiles32};')
        module=module.replace('cursor(region, &mut dst[32 * groups..], &bitmap[4 * groups..], [l, r]);',
                              'simd_packed(simd, region, &mut dst[32 * groups..], &bitmap[4 * groups..], left, [l,r-left]);')
    (root/f'merge/{arm}.rs').write_text(module)
    hook = '''
    #[cfg(target_arch = "x86_64")]
    if dst.len() >= 128 && region.len() - left >= 32
        && let Some(proof) = PROOF
    {
        ENGINE(proof, region, dst, bitmap, left);
        return;
    }
'''
    proof = 'deposit::select(level)' if arm == 'deposit' else 'if let Level::Avx2(p) = level { Some(p) } else { None }'
    hook = hook.replace('PROOF',proof).replace('ENGINE',f'{arm}::merge32_{arm}')
    mod = '#[cfg(all(feature = "merge-simd", target_arch = "x86_64"))]'
    def imports(m): return mod+'\n'+m+'\n'
    merge = root/'merge.rs'
    t = merge.read_text()
    t = imports(f'#[path = "merge/{arm}.rs"]\nmod {arm};')+'\n'+imports((S/'windows.rs').read_text())+'\n'+t
    t = t.replace('    let level = Level::new();\n', '    let level = Level::new();\n'+hook)
    if variant == 'avx2':
        # Seed the old packed path, without changing its per-child bounds.
        t=t.replace('    mut select:', '    [mut i, mut j]: [usize; 2],\n    mut select:').replace(
            '    let (mut i, mut j, mut groups) = (0, 0, 0);','    let mut groups = 0;')
        t=t.replace('packed(region, dst, bitmap, left, |out, a, b, mask| {',
                    'packed(region, dst, bitmap, left, [0,0], |out, a, b, mask| {',1)
        first=t.index('#[cfg(feature = "merge-simd")]\npub(super) fn simd')
        where=t.index('    let level = Level::new();',first)
        body=t[t.index('    #[inline(always)]\n    fn kernel',first):where]
        body='\n'.join(l[4:] for l in body.rstrip().splitlines())+'\n'
        body=body.replace('fn kernel<S: Simd>(simd: S, region: &[u8], dst: &mut [u8], bitmap: &[u8], left: usize) {',
                          'fn simd_packed<S: fearless_simd::Simd>(simd: S, region: &[u8], dst: &mut [u8], bitmap: &[u8], left: usize, cursors: [usize; 2]) {\n    use fearless_simd::{SimdBase, u8x16};')
        body=body.replace('packed(region, dst, bitmap, left, |out, a, b, mask| {',
                          'packed(region, dst, bitmap, left, cursors, |out, a, b, mask| {')
        t=(t[:first]+'#[cfg(feature = "merge-simd")]\n'+body+'\n'+t[first:t.index('    #[inline(always)]\n    fn kernel',first)]+t[where:])
        t=t.replace('    use fearless_simd::{Level, Simd, SimdBase, u8x16};','    use fearless_simd::Level;')
        t=t.replace('token => kernel(token, region, dst, bitmap, left)','token => simd_packed(token, region, dst, bitmap, left, [0,0])')
    t += f'\n#[cfg(all(test, feature = "merge-simd", target_arch = "x86_64"))]\n#[path = "merge/race_tests.rs"]\nmod race_tests;\n'
    merge.write_text(t)
    (root/'merge/race_tests.rs').write_text((S/'race_tests.rs').read_text().replace('ENGINE',f'{arm}::merge32_{arm}'))
    sp.run(['cargo','fmt','--all'],cwd=src,check=True)

def plumbing(src, arm):
    lib = src/'crates/codec/src'
    bins = lib/'bin'; bins.mkdir(exist_ok=True)
    t = (R/'replay.rs').read_text().replace('h[0] == 1 && view.len() >= 16 && view[11] == 0','h[0] == 1 && view.len() >= 16')
    (bins/'replay.rs').write_text(t)
    (lib/'block.rs').open('a').write((R/'section-entry.rs').read_text())
    (lib/'lib.rs').open('a').write('\n#[doc(hidden)]\npub use block::replay_huf;\n')
    # Reuse only the resident request protocol and clocks.
    shared = t[:t.index('fn pass(')]
    (bins/'kernel.rs').write_text('#![allow(dead_code, unused_imports)]\n'+shared.replace('//!','//',1)+(S/'kernel.rs').read_text())
    if arm == 'capture':
        entropy = lib/'entropy.rs'
        entropy.write_text(entropy.read_text().replace('            region.copy_from_slice(&scratch[..n]);','''            if std::env::var_os("ZXC_MERGE_CAPTURE").is_some() {
                use std::io::Write;
                let mut w = std::io::stdout().lock();
                for x in [start, n, n - ones] { w.write_all(&(x as u32).to_le_bytes())?; }
                w.write_all(run)?;
                w.write_all(region)?;
                w.write_all(&scratch[..n])?;
            }
            region.copy_from_slice(&scratch[..n]);'''))
        (bins/'capture.rs').write_text((S/'capture.rs').read_text())

if __name__ == '__main__':
    import sys
    mode, arm = sys.argv[1:]
    if mode == 'c': export('c', CPIN, Path.home()/'archive/hellobertrand/zxc')
    elif mode == 'prod': production(export(arm), arm)
    elif mode == 'shim': plumbing(S/f'src-{arm}', arm)
