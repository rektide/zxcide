use crate::{BlockDecoder, Dictionary, bad, encode::Workspace, require, wire::*};
use std::io::{self, Read, Write};

#[derive(Clone, Copy, Debug, Default)]
pub enum Mode {
    #[default]
    Split,
    Packed,
    /// SPL with size-priced RLE and PivCo literal/token sections.
    Dense,
}

#[derive(Clone, Copy, Debug)]
pub struct EncodeOptions<'a> {
    /// log2(block size), from 12 (4 KiB) to 21 (2 MiB).
    pub block_log: u8,
    pub mode: Mode,
    pub checksum: bool,
    /// Emit a seek index. Requires four extra bytes of memory per block.
    pub seekable: bool,
    pub dictionary: Option<&'a Dictionary>,
}
impl Default for EncodeOptions<'_> {
    fn default() -> Self {
        Self {
            block_log: 19,
            mode: Mode::Split,
            checksum: true,
            seekable: false,
            dictionary: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DecodeOptions<'a> {
    /// Resolved by ID on each frame. ID zero is a valid ID.
    pub dictionaries: &'a [Dictionary],
    /// Cumulative limit across all frames; tested before each output write.
    pub max_output: Option<u64>,
}

fn fill(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}
fn read<const N: usize>(r: &mut impl Read) -> io::Result<[u8; N]> {
    let mut h = [0; N];
    r.read_exact(&mut h)?;
    Ok(h)
}
/// Reusable block buffers for repeated independent compression jobs.
#[derive(Default)]
pub struct Encoder {
    input: Vec<u8>,
    work: Workspace,
    sizes: Vec<u32>,
}
impl Encoder {
    pub fn encode(&mut self, mut r: impl Read, mut w: impl Write, opt: EncodeOptions<'_>) -> io::Result<()> {
        zone!("encode frame");
        if !(12..=21).contains(&opt.block_log) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "block_log must be 12..=21"));
        }
        let mut h = [0; FRAME_HEADER_SIZE];
        h[..4].copy_from_slice(&MAGIC);
        h[4] = 9;
        h[5] = opt.block_log;
        h[6] = (opt.checksum as u8) << 7 | (opt.seekable as u8) << 5;
        if let Some(d) = opt.dictionary {
            h[6] |= 64;
            h[7..11].copy_from_slice(&d.id.to_le_bytes());
        }
        let sum = header_sum(&h);
        h[14..].copy_from_slice(&sum.to_le_bytes());
        w.write_all(&h)?;
        self.input.resize(1usize << opt.block_log, 0);
        self.sizes.clear();
        let (mut disk, mut plain, mut index, mut digest) = (FRAME_HEADER_SIZE as u64, 0, 0, 0);
        loop {
            let n = {
                zone!("read source block");
                fill(&mut r, &mut self.input)?
            };
            if n == 0 {
                break;
            }
            zone!("encode block");
            let bytes = &self.input[..n];
            let (kind, payload) = self.work.block(bytes, opt.mode, opt.dictionary);
            w.write_all(&block_header(kind, payload.len() as u32))?;
            w.write_all(payload)?;
            let physical =
                BLOCK_HEADER_SIZE as u32 + payload.len() as u32 + if opt.checksum { CHECKSUM_SIZE as u32 } else { 0 };
            if opt.checksum {
                let c = checksum(bytes, index);
                digest = fold(digest, c);
                w.write_all(&c.to_le_bytes())?;
            }
            if opt.seekable {
                self.sizes.push(physical);
            }
            disk = add(disk, u64::from(physical))?;
            plain = add(plain, n as u64)?;
            index = add(index, 1)?;
            if n < self.input.len() {
                break;
            }
        }
        w.write_all(&block_header(255, 0))?;
        disk = add(disk, BLOCK_HEADER_SIZE as u64)?;
        if opt.seekable {
            let length = seek_table_len(index)?;
            w.write_all(&seek_header(length))?;
            let mut anchor = FRAME_HEADER_SIZE as u64;
            for group in self.sizes.chunks(SEEK_GROUP_BLOCKS) {
                w.write_all(&u64::to_le_bytes(anchor))?;
                for size in group {
                    w.write_all(&size.to_le_bytes())?;
                    anchor = add(anchor, u64::from(*size))?;
                }
            }
            disk = add(add(disk, BLOCK_HEADER_SIZE as u64)?, length)?;
        }
        w.write_all(&footer(plain, disk, opt.checksum.then_some(digest))?)
    }
}

/// Encode one complete frame. The caller owns flushing the writer.
pub fn encode(r: impl Read, w: impl Write, opt: EncodeOptions<'_>) -> io::Result<()> {
    Encoder::default().encode(r, w, opt)
}

/// Reusable input and reconstruction buffers. Each frame's integrity state is local.
#[derive(Default)]
pub struct Decoder {
    body: Vec<u8>,
    block: BlockDecoder,
}
impl Decoder {
    pub fn decode(&mut self, mut r: impl Read, mut w: impl Write, opt: DecodeOptions<'_>) -> io::Result<()> {
        let mut total = 0;
        let mut first = true;
        loop {
            let mut h = [0; FRAME_HEADER_SIZE];
            match fill(&mut r, &mut h[..4])? {
                0 if !first => return Ok(()),
                4 => require(h[..4] == MAGIC, "bad ZXC magic or trailing data")?,
                _ if first => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "short ZXC header")),
                _ => return Err(bad("trailing bytes after frame")),
            }
            zone!("decode frame");
            first = false;
            r.read_exact(&mut h[4..])?;
            let frame = FrameHeader::parse(h)?;
            let dict = frame.dictionary(opt.dictionaries)?;
            let (mut disk, mut plain, mut index, mut digest) = (FRAME_HEADER_SIZE as u64, 0, 0, 0);
            loop {
                let h = read::<BLOCK_HEADER_SIZE>(&mut r)?;
                if h[0] == 255 {
                    validate_eof_header(h)?;
                    disk = add(disk, BLOCK_HEADER_SIZE as u64)?;
                    break;
                }
                zone!("decode block");
                let header = DataBlockHeader::parse(h, frame)?;
                self.body.resize(header.body_size(), 0);
                {
                    zone!("read payload");
                    r.read_exact(&mut self.body)?;
                }
                let block = self.block.decode(header, &self.body, dict, index, None)?;
                if let Some(c) = block.checksum {
                    digest = fold(digest, c);
                }
                let bytes = block.bytes;
                total = add(total, bytes.len() as u64)?;
                require(
                    opt.max_output.is_none_or(|max| total <= max),
                    "decoded output limit exceeded",
                )?;
                plain = add(plain, bytes.len() as u64)?;
                index = add(index, 1)?;
                disk = add(disk, header.disk_size() as u64)?;
                {
                    zone!("write plaintext");
                    w.write_all(bytes)?;
                }
            }
            if frame.seekable() {
                let t = seek_table_len(plain.div_ceil(frame.block_size() as u64))?;
                validate_seek_header(read(&mut r)?, t)?;
                let skipped = io::copy(&mut (&mut r).take(t), &mut io::sink())?;
                require(skipped == t, "short seek table")?;
                disk = add(add(disk, BLOCK_HEADER_SIZE as u64)?, t)?;
            }
            let expected = footer(plain, disk, frame.checksum().then_some(digest))?;
            let mut got = [0; 25];
            r.read_exact(&mut got[..expected.len()])?;
            require(expected == got[..expected.len()], "frame footer or digest mismatch")?;
        }
    }
}

/// Decode all concatenated frames, checking every present checksum. A later
/// error may leave earlier blocks in the writer. The caller owns flushing.
pub fn decode(r: impl Read, w: impl Write, opt: DecodeOptions<'_>) -> io::Result<()> {
    Decoder::default().decode(r, w, opt)
}
