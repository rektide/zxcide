use crate::{Dictionary, bad, block, encode::Workspace, require, wire::*};
use std::io::{self, Read, Write};

#[derive(Clone, Copy, Debug, Default)]
pub enum Mode {
    #[default]
    Split,
    Packed,
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
fn table_len(n: u64) -> io::Result<u64> {
    let sizes = n.checked_mul(4).ok_or_else(|| bad("seek table size overflow"))?;
    add(sizes, n.div_ceil(64) * 8)
}
fn sek(n: u64) -> [u8; 8] {
    block_header(254, (n ^ (n >> 32)) as u32)
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
        let mut h = [0; 16];
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
        let (mut disk, mut plain, mut index, mut digest) = (16, 0, 0, 0);
        loop {
            let n = fill(&mut r, &mut self.input)?;
            if n == 0 {
                break;
            }
            zone!("encode block");
            let bytes = &self.input[..n];
            let (kind, payload) = self.work.block(bytes, opt.mode, opt.dictionary);
            w.write_all(&block_header(kind, payload.len() as u32))?;
            w.write_all(payload)?;
            let physical = 8 + payload.len() as u32 + if opt.checksum { 4 } else { 0 };
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
        disk = add(disk, 8)?;
        if opt.seekable {
            let length = table_len(index)?;
            w.write_all(&sek(length))?;
            let mut anchor = 16;
            for group in self.sizes.chunks(64) {
                w.write_all(&u64::to_le_bytes(anchor))?;
                for size in group {
                    w.write_all(&size.to_le_bytes())?;
                    anchor = add(anchor, u64::from(*size))?;
                }
            }
            disk = add(add(disk, 8)?, length)?;
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
    payload: Vec<u8>,
    out: Vec<u8>,
}
impl Decoder {
    pub fn decode(&mut self, mut r: impl Read, mut w: impl Write, opt: DecodeOptions<'_>) -> io::Result<()> {
        let mut total = 0;
        let mut first = true;
        loop {
            let mut h = [0; 16];
            match fill(&mut r, &mut h[..4])? {
                0 if !first => return Ok(()),
                4 => require(h[..4] == MAGIC, "bad ZXC magic or trailing data")?,
                _ if first => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "short ZXC header")),
                _ => return Err(bad("trailing bytes after frame")),
            }
            zone!("decode frame");
            first = false;
            r.read_exact(&mut h[4..])?;
            require(h[4] == 9, "unsupported ZXC version")?;
            require(
                h[14..] == header_sum(&h).to_le_bytes() && (12..=21).contains(&h[5]) && h[6] & 31 == 0 && h[11..14] == [0; 3],
                "invalid ZXC file header",
            )?;
            let limit = 1usize << h[5];
            let checks = h[6] & 128 != 0;
            let dict = if h[6] & 64 != 0 {
                let id = u32::from_le_bytes(h[7..11].try_into().unwrap());
                Some(
                    opt.dictionaries
                        .iter()
                        .find(|d| d.id == id)
                        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("dictionary {id:08x} required")))?,
                )
            } else {
                require(h[7..11] == [0; 4], "reserved dictionary field")?;
                None
            };
            let (mut disk, mut plain, mut index, mut digest) = (16, 0, 0, 0);
            loop {
                let (kind, size) = parse_block(read(&mut r)?)?;
                disk = add(disk, 8)?;
                if kind == 255 {
                    require(size == 0, "invalid EOF size")?;
                    break;
                }
                zone!("decode block");
                require(kind <= 2 && size as usize <= limit, "invalid data block type or size")?;
                self.payload.resize(size as usize, 0);
                r.read_exact(&mut self.payload)?;
                let bytes = if kind == 0 {
                    &self.payload[..]
                } else {
                    block::unpack(kind, &self.payload, limit, dict, &mut self.out)?
                };
                if checks {
                    let c = u32::from_le_bytes(read(&mut r)?);
                    require(checksum(bytes, index) == c, "block checksum mismatch")?;
                    digest = fold(digest, c);
                }
                total = add(total, bytes.len() as u64)?;
                require(opt.max_output.is_none_or(|max| total <= max), "decoded output limit exceeded")?;
                plain = add(plain, bytes.len() as u64)?;
                index = add(index, 1)?;
                disk = add(disk, u64::from(size) + if checks { 4 } else { 0 })?;
                w.write_all(bytes)?;
            }
            if h[6] & 32 != 0 {
                let t = table_len(plain.div_ceil(limit as u64))?;
                require(read::<8>(&mut r)? == sek(t), "invalid seek table header")?;
                let skipped = io::copy(&mut (&mut r).take(t), &mut io::sink())?;
                require(skipped == t, "short seek table")?;
                disk = add(add(disk, 8)?, t)?;
            }
            let expected = footer(plain, disk, checks.then_some(digest))?;
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
