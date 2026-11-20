#!/usr/bin/env python3
"""Insert diagnostic edges into exactly the pinned source. Never edit the checkout."""
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
parallel = root / 'crates/parallel/src'


def edit(path, replacements):
    text = path.read_text()
    for old, new in replacements:
        if text.count(old) != 1:
            raise ValueError(f'{path}: expected one anchor: {old!r}')
        text = text.replace(old, new)
    path.write_text(text)


(parallel / 'probe.rs').write_bytes(pathlib.Path(__file__).with_name('probe.rs').read_bytes())
edit(root / 'crates/cli/src/main.rs', [
    ('match run(Cli::parse()) {', 'match zxc_parallel::probe::session(|| run(Cli::parse())) {'),
])
edit(root / 'crates/codec/src/wire.rs', [
    ('impl DataBlockHeader {', '''impl DataBlockHeader {
    /// Diagnostic wire metadata: top 8 bits kind, low 24 payload bytes.
    pub fn probe_meta(self) -> u32 {
        (u32::from(self.kind) << 24) | self.payload_size as u32
    }'''),
])
edit(parallel / 'lib.rs', [
    ('mod channel;', 'mod channel;\n#[doc(hidden)]\npub mod probe;'),
    ('while let Some(mut frame) = DecodeFrame::start(&mut r, first)? {', '''loop {
        let clock = probe::frame_clock();
        let now = || clock.map_or(0, |clock| clock.elapsed().as_nanos() as u64);
        let frame_start = now();
        let Some(mut frame) = DecodeFrame::start(&mut r, first)? else { break };
        let mut meta = clock.map(|_| probe::Blocks::new());
        let (mut input, mut output) = meta.as_mut().map_or((None, None), |m| (Some(&mut m.input), Some(&mut m.output)));'''),
    ('|mut buf| Ok(read_data_block(&mut r, settings, &mut buf)?.map(|header| (header, buf))),', '''|mut buf| Ok(read_data_block(&mut r, settings, &mut buf)?.map(|header| {
                if let Some(input) = input.as_deref_mut() { probe::Blocks::push(input, header.probe_meta()); }
                (header, buf)
            })),'''),
    ('frame.commit(&mut w, header, block.as_verified(), &mut total, options.max_output)?;', '''let verified = block.as_verified();
                let bytes = verified.bytes.len();
                frame.commit(&mut w, header, verified, &mut total, options.max_output)?;
                if let Some(output) = output.as_deref_mut() { probe::Blocks::push(output, bytes as u32); }'''),
    ('frame.finish(&mut r)?;', '''let footer = now();
        frame.finish(&mut r)?;
        let frame_end = now();
        if let Some(meta) = meta { probe::frame_done(frame_start, footer, frame_end, meta)?; }'''),
])
edit(parallel / 'channel.rs', [
    ('use crate::{Config, Scheduler, compute};', 'use crate::{Config, Scheduler, compute, probe::{self, Op::*, NONE}};'),
    ('        thread::scope(|scope| {\n            // Both coordinator', '''        let (mut c, mut workers) = probe::pool(self.0.workers);
        c.mark(Pool, self.0.slots as u64, 0);
        let status = thread::scope(|scope| {
            // Both coordinator'''),
    ('for _ in 0..self.0.workers {', 'for lane in &mut workers {'),
    ('let mut scratch = None;', 'lane.mark(Entry, NONE, 0);\n                        let mut scratch = None;'),
    ('Q::recv_job(&rx)', '''lane.mark(JobRecv, NONE, 0);
                                let job = Q::recv_job(&rx);
                                let t = lane.at();
                                lane.save(t, JobGot, job.as_ref().map_or(NONE, |(i, _)| *i), 0);
                                job'''),
    ('''                            let result = compute(&mut scratch, index, job, work);
                            zone!("parallel send result");
                            if !Q::send_result(&tx, (index, result)) {
                                break;
                            }
                        }
                    })?;''', '''                            lane.mark(Compute, index, 0);
                            let result = compute(&mut scratch, index, job, work);
                            let t = lane.at();
                            lane.save(t, Computed, index, u32::from(result.is_ok()));
                            zone!("parallel send result");
                            lane.mark(ResultSend, index, 0);
                            let sent = Q::send_result(&tx, (index, result));
                            lane.mark(ResultSent, index, u32::from(sent));
                            if !sent { break; }
                        }
                        drop(scratch);
                        lane.mark(Exit, NONE, 0);
                    })?;'''),
    ('''                    let Some(buf) = free.pop() else { break };
                    let item = match next(buf) {''', '''                    let Some(buf) = free.pop() else { c.mark(Full, admitted, 0); break };
                    c.mark(Pop, admitted, 0);
                    c.mark(Next, admitted, 0);
                    let read = next(buf);
                    let t = c.at();
                    c.save(t, NextDone, admitted, match &read { Ok(Some(_)) => 1, Ok(None) => 0, Err(_) => 2 });
                    let item = match read {'''),
    ('if !Q::send_job(&job_tx, (admitted, item)) {', '''c.mark(JobSend, admitted, 0);
                    let sent = Q::send_job(&job_tx, (admitted, item));
                    c.mark(JobSent, admitted, u32::from(sent));
                    if !sent {'''),
    ('return end.expect("only a terminal read leaves the window empty");', '''let success = end.expect("only a terminal read leaves the window empty");
                    c.mark(Terminal, admitted, committed as u32);
                    return success;'''),
    ('''                        let (index, result) = Q::recv_result(&result_rx)
                            .ok_or_else(|| io::Error::other("block result queue disconnected"))?;''', '''                        c.mark(ResultRecv, committed, 0);
                        let result = Q::recv_result(&result_rx);
                        let t = c.at();
                        c.save(t, ResultGot, result.as_ref().map_or(NONE, |(i, _)| *i), 0);
                        let (index, result) = result.ok_or_else(|| io::Error::other("block result queue disconnected"))?;'''),
    ('pending[ring(index)] = Some(result);', 'pending[ring(index)] = Some(result);\n                        c.mark(Pending, index, 0);'),
    ('free.push(commit(result?)?);', 'c.mark(Commit, committed, 0);\n                    free.push(commit(result?)?);\n                    c.mark(Credit, committed, 0);'),
    ('''        })
    }
}

#[cfg(test)]''', '''        });
        let t = c.at();
        c.save(t, Joined, NONE, 0);
        probe::joined(c, workers);
        status
    }
}

#[cfg(test)]'''),
])
