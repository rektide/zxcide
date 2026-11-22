//! Diagnostic for single, checked, dictionaryless SPL16 v9 frames on Linux x86-64.
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

fn pass(decoder: &mut BlockDecoder, src: &[u8], blocks: &[Block], emit: bool) -> (usize, u64) {
    let (mut bytes, mut tag) = (0, 0u64);
    for (i, &(header, ref range)) in blocks.iter().enumerate() {
        let block = decoder.decode(header, &src[range.clone()], None, i as u64, None).unwrap();
        assert!(block.checksum.is_some());
        let out = block.bytes;
        let n = out.len();
        bytes += n;
        let word = u64::from(out[0]) | u64::from(out[n / 2]) << 8 | u64::from(out[n - 1]) << 16;
        tag = (tag ^ word ^ n as u64).wrapping_mul(0x9e3779b97f4a7c15);
        if emit {
            io::stdout().lock().write_all(out).unwrap();
        }
    }
    (bytes, tag)
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3);
    let emit = args[1] == "verify";
    assert!(emit || args[1] == "serve");
    let mut src = std::fs::read(&args[2]).unwrap();
    let frame = FrameHeader::parse(src[..16].try_into().unwrap()).unwrap();
    assert!(frame.checksum() && !frame.seekable() && frame.dictionary_id().is_none());
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

    let (bytes, tag) = pass(&mut decoder, &src, &blocks, emit);
    if emit {
        io::stdout().flush().unwrap();
    } else {
        println!("ready {} {bytes} {tag} bad-checksum-rejected", blocks.len());
        io::stdout().flush().unwrap();
        for line in io::stdin().lines() {
            let repeats: usize = line.unwrap().parse().unwrap();
            assert!(repeats > 0);
            // RESET_STAGES
            let cpu = ns(THREAD_CPU);
            let wall = ns(MONOTONIC);
            for _ in 0..repeats {
                assert_eq!(pass(&mut decoder, &src, &blocks, false), (bytes, tag));
            }
            let wall = ns(MONOTONIC) - wall;
            let cpu = ns(THREAD_CPU) - cpu;
            println!("{wall} {cpu}"); // PRINT_STAGES
            io::stdout().flush().unwrap();
        }
    }
}
