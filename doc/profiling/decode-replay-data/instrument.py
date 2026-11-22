#!/usr/bin/env python3
"""Instrument the pinned scratch exports only; fail if a source anchor drifts."""
import sys
from pathlib import Path


def edit(path, changes):
    text = path.read_text()
    for old, new in changes:
        assert text.count(old) == 1, (path, old, text.count(old))
        text = text.replace(old, new)
    path.write_text(text)


rs, c = map(Path, sys.argv[1:])
src = rs / 'crates/codec/src'
edit(src / 'lib.rs', [('mod block;', '#[doc(hidden)]\npub mod replay_stage;\nmod block;')])
(src / 'replay_stage.rs').write_text('''use std::{cell::Cell, time::Instant};

thread_local! {
    static START: Cell<Option<Instant>> = const { Cell::new(None) };
    static SUMS: Cell<[u64; 3]> = const { Cell::new([0; 3]) };
}

pub fn reset() { SUMS.set([0; 3]); }
pub fn report() -> [u64; 3] { SUMS.get() }
pub fn begin() { START.set(Some(Instant::now())); }
pub fn cut(stage: usize) {
    let end = Instant::now();
    let elapsed = end.duration_since(START.replace(Some(end)).unwrap()).as_nanos() as u64;
    let mut sums = SUMS.get();
    sums[stage] += elapsed;
    SUMS.set(sums);
}
''')
edit(src / 'block.rs', [
    ('            unpack_with_sections(\n                header.kind,',
     '            crate::replay_stage::begin();\n            let decoded = unpack_with_sections(\n                header.kind,'),
    ('                &mut self.sections,\n            )?\n        };',
     '                &mut self.sections,\n            )?;\n            crate::replay_stage::cut(1);\n            decoded\n        };'),
    ('        let checksum = if header.frame.checksum() {',
     '        let checksum = if header.frame.checksum() {\n            crate::replay_stage::begin();'),
    ('            Some(c)\n', '            crate::replay_stage::cut(2);\n            Some(c)\n'),
    ('    zone!("LZ reconstruct");', '    crate::replay_stage::cut(0);\n    zone!("LZ reconstruct");'),
])
edit(src / 'bin/replay.rs', [
    ('// RESET_STAGES', 'zxc_codec::replay_stage::reset();'),
    ('println!("{wall} {cpu}"); // PRINT_STAGES',
     'let [section, lz, hash] = zxc_codec::replay_stage::report();\n'
     '            println!("{wall} {cpu} {section} {lz} {hash}");'),
])
edit(c / 'src/lib/zxc_decompress.c', [
    ('#include "zxc_internal.h"', '#include "zxc_internal.h"\n#include "replay_stage.h"'),
    ('        t_ptr = ctx->tok_buffer;\n    }\n\n    uint8_t* d_ptr = dst;',
     '        t_ptr = ctx->tok_buffer;\n    }\n\n    replay_cut(0);\n    uint8_t* d_ptr = dst;'),
    ('        case ZXC_BLOCK_SPL:\n            decoded_sz', '        case ZXC_BLOCK_SPL:\n            replay_begin();\n            decoded_sz'),
    ('                                    : zxc_decode_block_spl(ctx, data, comp_sz, dst, dst_cap);\n            break;',
     '                                    : zxc_decode_block_spl(ctx, data, comp_sz, dst, dst_cap);\n'
     '            if (decoded_sz >= 0) replay_cut(1);\n            break;'),
    ('    if (has_checksum && LIKELY(decoded_sz >= 0)) {',
     '    if (has_checksum && LIKELY(decoded_sz >= 0)) {\n        replay_begin();'),
    ('            return ZXC_ERROR_BAD_CHECKSUM;\n    }',
     '            return ZXC_ERROR_BAD_CHECKSUM;\n        replay_cut(2);\n    }'),
])
(c / 'src/lib/replay_stage.h').write_bytes(Path(__file__).with_name('stage.h').read_bytes())
