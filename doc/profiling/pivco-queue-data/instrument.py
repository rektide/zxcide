#!/usr/bin/env python3
"""Edit only copies of the clean exports; these counters must never be timed."""
from pathlib import Path

w = Path(__file__).resolve().parent


def edit(path, *pairs):
    s = path.read_text()
    for old, new in pairs:
        assert s.count(old) == 1, (path, old)
        s = s.replace(old, new)
    path.write_text(s)


for variant in ('baseline', 'candidate'):
    src = w / f'diag-{variant}/crates/codec/src'
    h = src / 'entropy.rs'
    b = src / 'block.rs'
    f = src / 'frame.rs'
    trial = variant == 'candidate'
    cold = 'cold' if trial else 'true'
    retained = 'queue.capacity().min(16) * usize::from(queue.capacity() <= 16)' if trial else '0'
    cached = 'self.sections.queue.capacity()' if trial else '0'
    trace = '''        let probe = |why: &str, pending, cap, osize| {
            eprintln!("E {:?} {pending} {cap} {osize} {why}", std::thread::current().id());
        };
        let mut grows = 0;
        let mut frontier = queue.len();
        let mut flat_symbols = 0;
        let mut gathered = 0;
        let order_cap = order.capacity();
        let out_cap = output.capacity();
        let scratch_cap = scratch.capacity();
        let zero = count.saturating_sub(output.len()) + count.saturating_sub(scratch.len());
'''
    stat = f'''        let keep = {retained};
        let byte_allocs = usize::from(out_cap != output.capacity()) + usize::from(scratch_cap != scratch.capacity());
        eprintln!("S {{:?}} {{count}} {{}} {{grows}} {{}} {{}} {{keep}} {{frontier}} {{}} {{order_len}} {{order_cap}} {{byte_allocs}} {{zero}} {{flat_symbols}} {{gathered}} {{gathered}} {{}} {{}}",
            std::thread::current().id(), usize::from({cold}), usize::from(!{cold}),
            queue.capacity() - keep, queue.capacity(), output.capacity(), scratch.capacity());
'''
    if trial:
        edit(h, ('        if queue.capacity() == 0 {', '        let cold = queue.capacity() == 0;\n        if cold {'))
    edit(h,
        ('        init_prefix(output, count);', trace + '        init_prefix(output, count);'),
        ('            let run = take(&mut src, (n * usize::from(node.flat.max(1))).div_ceil(8))?;',
         '''            let run = take(&mut src, (n * usize::from(node.flat.max(1))).div_ceil(8))
                .inspect_err(|e| probe(&e.to_string(), queue.len(), queue.capacity(), order.len()))?;'''),
        ('                        queue.push_back((c, base, cnt));', '''                        let old = queue.capacity();
                        queue.push_back((c, base, cnt));
                        frontier = frontier.max(queue.len());
                        grows += usize::from(queue.capacity() != old);'''),
        ('                        require(cnt == 0, "Huffman run selects missing child")?;',
         '''                        require(cnt == 0, "Huffman run selects missing child")
                            .inspect_err(|e| probe(&e.to_string(), queue.len(), queue.capacity(), order.len()))?;'''),
        ('                flat::unpack(&mut output[start..start + n], run, &node.lookup);',
         '                flat_symbols += n;\n                flat::unpack(&mut output[start..start + n], run, &node.lookup);'),
        ('        require(src.is_empty(), "trailing Huffman data")?;',
         '''        require(src.is_empty(), "trailing Huffman data")
            .inspect_err(|e| probe(&e.to_string(), queue.len(), queue.capacity(), order.len()))?;
        let order_len = order.len();'''),
        ('            region.copy_from_slice(&scratch[..n]);',
         '            gathered += n;\n            region.copy_from_slice(&scratch[..n]);'),
        ('        if queue.capacity() <= MAX_RETAINED_QUEUE {' if trial else '        Ok(&output[..count])',
         stat + ('        if queue.capacity() <= MAX_RETAINED_QUEUE {' if trial else '        Ok(&output[..count])')),
    )
    edit(b,
        ('        self.sections.fit_block(limit);', '''        self.sections.fit_block(limit);
        eprintln!("I {:?} {index} {limit} {} {}", std::thread::current().id(), header.kind,
            std::mem::size_of::<BlockDecoder>());'''),
        ('            )?\n        };\n        require(', '''            ).inspect_err(|e| eprintln!("X {:?} {} {} {} {} {e}", std::thread::current().id(),
                self.sections.literals.capacity(), self.sections.tokens.capacity(),
                self.sections.merge.capacity(), ''' + cached + '''))?
        };
        eprintln!("B {:?} {index} {limit} {} {} {} {} {}", std::thread::current().id(),
            header.kind, self.sections.literals.capacity(), self.sections.tokens.capacity(),
            self.sections.merge.capacity(), ''' + cached + ''');
        require('''),
        ('    let literals = section(', '''    eprintln!("L {:?} {}", std::thread::current().id(), enc[0]);
    let literals = section('''),
        ('    let sequences = section(', '''    eprintln!("T {:?} {}", std::thread::current().id(), enc[1]);
    let sequences = section('''),
    )
    edit(f, ('            frame: FrameHeader::parse(h)?,', '''            frame: {
                let header = FrameHeader::parse(h)?;
                eprintln!("F {} {:?}", header.block_size(), header.dictionary_id());
                header
            },'''))
