//! Diagnostic for single, checked, dictionaryless SPL16 v9 frames on Linux x86-64.
#![allow(unexpected_cfgs)] // replay_perf is set only by the scratch replay build.
use std::io::{self, Write};
use zxc_codec::{
    BlockDecoder,
    wire::{DataBlockHeader, FrameHeader, validate_eof_header},
};

#[repr(C)]
struct Timespec {
    sec: i64,
    nsec: i64,
}

unsafe extern "C" {
    fn clock_gettime(clock: i32, time: *mut Timespec) -> i32;
}

const MONOTONIC: i32 = 1;
const THREAD_CPU: i32 = 3;

fn ns(clock: i32) -> u64 {
    let mut t = Timespec { sec: 0, nsec: 0 };
    // Linux x86-64 clock_gettime ABI; pointer refers to live writable storage.
    assert_eq!(unsafe { clock_gettime(clock, &mut t) }, 0);
    (t.sec as u64) * 1_000_000_000 + t.nsec as u64
}

type Block = (DataBlockHeader, std::ops::Range<usize>);

fn replay(count: usize, reject: &str, emit: bool, mut pass: impl FnMut(bool) -> (usize, u64)) {
    let (bytes, tag) = pass(emit);
    if !emit {
        println!("ready {count} {bytes} {tag} {reject}");
        io::stdout().flush().unwrap();
        for line in io::stdin().lines() {
            let repeats: usize = line.unwrap().parse().unwrap();
            assert!(repeats > 0);
            // RESET_STAGES
            let cpu = ns(THREAD_CPU);
            let wall = ns(MONOTONIC);
            for _ in 0..repeats {
                assert_eq!(pass(false), (bytes, tag));
            }
            let wall = ns(MONOTONIC) - wall;
            let cpu = ns(THREAD_CPU) - cpu;
            println!("{wall} {cpu}"); // PRINT_STAGES
            io::stdout().flush().unwrap();
        }
    }
    io::stdout().flush().unwrap();
}

fn consume(result: &mut (usize, u64), out: &[u8], emit: bool) {
    let n = out.len();
    result.0 += n;
    let word = u64::from(out[0]) | u64::from(out[n / 2]) << 8 | u64::from(out[n - 1]) << 16;
    result.1 = (result.1 ^ word ^ n as u64).wrapping_mul(0x9e3779b97f4a7c15);
    if emit {
        io::stdout().lock().write_all(out).unwrap();
    }
}

fn pass(decoder: &mut BlockDecoder, src: &[u8], blocks: &[Block], emit: bool) -> (usize, u64) {
    let mut result = (0, 0);
    for (i, &(header, ref range)) in blocks.iter().enumerate() {
        let block = decoder.decode(header, &src[range.clone()], None, i as u64, None).unwrap();
        assert!(block.checksum.is_some());
        consume(&mut result, block.bytes, emit);
    }
    result
}

#[cfg(replay_perf)]
fn huf(src: &[u8], index: &str, emit: bool, limit: usize) {
    use zxc_codec::replay_huf;
    let index = std::fs::read_to_string(index).unwrap();
    let mut end = 0;
    let rows: Vec<_> = index
        .lines()
        .map(|line| {
            let mut cols = line.split_whitespace().map(|s| s.parse::<usize>().unwrap());
            let (off, len, n) = (cols.next().unwrap(), cols.next().unwrap(), cols.next().unwrap());
            assert!(cols.next().is_none() && end <= off && len >= 128 && n > 0 && n <= limit);
            let data = src.get(off..off.checked_add(len).unwrap()).unwrap();
            end = off + len;
            (data, n)
        })
        .collect();
    assert!(!rows.is_empty());
    let (mut out, mut scratch) = (Vec::with_capacity(limit), Vec::with_capacity(limit));
    assert!(replay_huf(&[0xff; 128], 1, &mut out, &mut scratch).is_err());
    replay(rows.len(), "bad-table-rejected", emit, |emit| {
        let mut result = (0, 0);
        for &(data, n) in &rows {
            let bytes = replay_huf(data, n, &mut out, &mut scratch).unwrap();
            assert_eq!(bytes.len(), n);
            consume(&mut result, bytes, emit);
        }
        result
    });
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(args.len() == 3 || cfg!(replay_perf) && args.len() == 4);
    let emit = args[1] == "verify";
    assert!(emit || args[1] == "serve");
    let mut src = std::fs::read(&args[2]).unwrap();
    let frame = FrameHeader::parse(src[..16].try_into().unwrap()).unwrap();
    assert!(frame.checksum() && !frame.seekable() && frame.dictionary_id().is_none());
    #[cfg(replay_perf)]
    if let Some(index) = args.get(3) {
        return huf(&src, index, emit, frame.block_size());
    }
    let mut pos = 16;
    let mut blocks = Vec::new();
    loop {
        let h: [u8; 8] = src[pos..pos + 8].try_into().unwrap();
        if h[0] == 255 {
            validate_eof_header(h).unwrap();
            break;
        }
        let parsed = DataBlockHeader::parse(h, frame).unwrap();
        let body = pos + 8..pos + parsed.disk_size();
        let view = &src[body.clone()];
        assert!(h[0] == 1 && view.len() >= 16 && view[11] == 0); // SPL16
        blocks.push((parsed, body));
        pos += parsed.disk_size();
    }
    assert!(blocks.len() > 1);
    let mut decoder = BlockDecoder::default();
    let i = blocks.len() / 2;
    let (header, body) = &blocks[i];
    src[body.end - 4] ^= 1;
    let err = decoder.decode(*header, &src[body.clone()], None, i as u64, None).unwrap_err();
    assert_eq!(err.to_string(), "block checksum mismatch");
    src[body.end - 4] ^= 1;

    replay(blocks.len(), "bad-checksum-rejected", emit, |emit| pass(&mut decoder, &src, &blocks, emit));
}
