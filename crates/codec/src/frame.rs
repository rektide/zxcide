use crate::{BlockDecoder, BlockEncoder, Dictionary, EncodedBlock, VerifiedBlock, bad, require, wire::*};
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
/// Fill one source block, retrying interrupted reads. A short block is final.
pub fn read_source_block(r: &mut impl Read, buf: &mut Vec<u8>, block_size: usize) -> io::Result<()> {
    zone!("read source block");
    buf.resize(block_size, 0);
    let len = fill(r, buf)?;
    buf.truncate(len);
    Ok(())
}

struct FrameState {
    disk: u64,
    plain: u64,
    index: u64,
    digest: u64,
}

impl FrameState {
    fn new() -> Self {
        Self {
            disk: FRAME_HEADER_SIZE as u64,
            plain: 0,
            index: 0,
            digest: 0,
        }
    }

    fn record_block(&mut self, plain: usize, physical: usize, checksum: Option<u32>) -> io::Result<()> {
        self.disk = add(self.disk, physical as u64)?;
        self.plain = add(self.plain, plain as u64)?;
        self.index = add(self.index, 1)?;
        if let Some(c) = checksum {
            self.digest = fold(self.digest, c);
        }
        Ok(())
    }

    fn record_end(&mut self) -> io::Result<()> {
        self.disk = add(self.disk, BLOCK_HEADER_SIZE as u64)?;
        Ok(())
    }

    fn record_seek(&mut self, len: u64) -> io::Result<()> {
        self.disk = add(add(self.disk, BLOCK_HEADER_SIZE as u64)?, len)?;
        Ok(())
    }

    fn footer(&self, checksum: bool) -> io::Result<Vec<u8>> {
        footer(self.plain, self.disk, checksum.then_some(self.digest))
    }
}

/// Ordered output state for one encoded frame. Does not flush the writer.
pub struct EncodeFrame<'a> {
    frame: FrameHeader,
    state: FrameState,
    sizes: &'a mut Vec<u32>,
}

impl<'a> EncodeFrame<'a> {
    /// Write the prologue and reuse the supplied seek-table entry storage.
    pub fn start(w: &mut impl Write, opt: EncodeOptions<'_>, sizes: &'a mut Vec<u32>) -> io::Result<Self> {
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
        let frame = FrameHeader::parse(h)?;
        w.write_all(&h)?;
        sizes.clear();
        Ok(Self {
            frame,
            state: FrameState::new(),
            sizes,
        })
    }

    pub fn block_size(&self) -> usize {
        self.frame.block_size()
    }

    /// Write the next block, encoded with this frame's settings and index.
    pub fn commit(&mut self, w: &mut impl Write, block: &EncodedBlock<impl AsRef<[u8]>>) -> io::Result<()> {
        let payload = block.payload.as_ref();
        require(
            block.plain_len <= self.block_size() && self.frame.checksum() == block.checksum.is_some(),
            "encoded block does not match frame settings",
        )?;
        w.write_all(&block_header(block.kind, payload.len() as u32))?;
        w.write_all(payload)?;
        if let Some(c) = block.checksum {
            w.write_all(&c.to_le_bytes())?;
        }
        let physical = self.frame.block_overhead() + payload.len();
        if self.frame.seekable() {
            self.sizes.push(physical as u32);
        }
        self.state.record_block(block.plain_len, physical, block.checksum)
    }

    /// Write EOF, any seek table, and the footer after all blocks are committed.
    pub fn finish(mut self, w: &mut impl Write) -> io::Result<()> {
        w.write_all(&block_header(255, 0))?;
        self.state.record_end()?;
        if self.frame.seekable() {
            let length = seek_table_len(self.state.index)?;
            w.write_all(&seek_header(length))?;
            let mut anchor = FRAME_HEADER_SIZE as u64;
            for group in self.sizes.chunks(SEEK_GROUP_BLOCKS) {
                w.write_all(&u64::to_le_bytes(anchor))?;
                for size in group {
                    w.write_all(&size.to_le_bytes())?;
                    anchor = add(anchor, u64::from(*size))?;
                }
            }
            self.state.record_seek(length)?;
        }
        w.write_all(&self.state.footer(self.frame.checksum())?)
    }
}

/// Reusable block buffers for repeated independent compression jobs.
#[derive(Default)]
pub struct Encoder {
    input: Vec<u8>,
    work: BlockEncoder,
    sizes: Vec<u32>,
}
impl Encoder {
    pub fn encode(&mut self, mut r: impl Read, mut w: impl Write, opt: EncodeOptions<'_>) -> io::Result<()> {
        zone!("encode frame");
        let mut frame = EncodeFrame::start(&mut w, opt, &mut self.sizes)?;
        loop {
            read_source_block(&mut r, &mut self.input, frame.block_size())?;
            if self.input.is_empty() {
                break;
            }
            zone!("encode block");
            let block = self.work.encode(&self.input, opt, frame.state.index);
            frame.commit(&mut w, &block)?;
            if self.input.len() < frame.block_size() {
                break;
            }
        }
        frame.finish(&mut w)
    }
}

/// Encode one complete frame. The caller owns flushing the writer.
pub fn encode(r: impl Read, w: impl Write, opt: EncodeOptions<'_>) -> io::Result<()> {
    Encoder::default().encode(r, w, opt)
}

/// Ordered validation and output state for one decoded frame.
pub struct DecodeFrame {
    frame: FrameHeader,
    state: FrameState,
}

impl DecodeFrame {
    /// Read the next prologue. A clean EOF is valid only after a complete frame.
    pub fn start(r: &mut impl Read, first: bool) -> io::Result<Option<Self>> {
        let mut h = [0; FRAME_HEADER_SIZE];
        match fill(r, &mut h[..4])? {
            0 if !first => return Ok(None),
            4 => require(h[..4] == MAGIC, "bad ZXC magic or trailing data")?,
            _ if first => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "short ZXC header")),
            _ => return Err(bad("trailing bytes after frame")),
        }
        r.read_exact(&mut h[4..])?;
        Ok(Some(Self {
            frame: FrameHeader::parse(h)?,
            state: FrameState::new(),
        }))
    }

    pub fn header(&self) -> FrameHeader {
        self.frame
    }

    /// Write the next verified block after checking the cumulative output limit.
    pub fn commit(
        &mut self,
        w: &mut impl Write,
        header: DataBlockHeader,
        block: VerifiedBlock<'_>,
        total: &mut u64,
        max_output: Option<u64>,
    ) -> io::Result<()> {
        *total = add(*total, block.bytes.len() as u64)?;
        require(
            max_output.is_none_or(|max| *total <= max),
            "decoded output limit exceeded",
        )?;
        self.state
            .record_block(block.bytes.len(), header.disk_size(), block.checksum)?;
        zone!("write plaintext");
        w.write_all(block.bytes)
    }

    /// After reading EOF and committing all blocks, check the seek table extent
    /// and verify the footer, including any frame digest.
    pub fn finish(mut self, r: &mut impl Read) -> io::Result<()> {
        self.state.record_end()?;
        if self.frame.seekable() {
            let t = seek_table_len(self.state.plain.div_ceil(self.frame.block_size() as u64))?;
            validate_seek_header(read(r)?, t)?;
            let skipped = io::copy(&mut r.take(t), &mut io::sink())?;
            require(skipped == t, "short seek table")?;
            self.state.record_seek(t)?;
        }
        let expected = self.state.footer(self.frame.checksum())?;
        let mut got = [0; 25];
        r.read_exact(&mut got[..expected.len()])?;
        require(expected == got[..expected.len()], "frame footer or digest mismatch")
    }
}

/// Read a data block body, or return `None` after verifying the frame's EOF
/// header. The seek table and footer are deliberately left unread.
pub fn read_data_block(
    r: &mut impl Read,
    frame: FrameHeader,
    body: &mut Vec<u8>,
) -> io::Result<Option<DataBlockHeader>> {
    let h = read::<BLOCK_HEADER_SIZE>(r)?;
    if h[0] == 255 {
        validate_eof_header(h)?;
        return Ok(None);
    }
    let header = DataBlockHeader::parse(h, frame)?;
    body.resize(header.body_size(), 0);
    zone!("read payload");
    r.read_exact(body)?;
    Ok(Some(header))
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
        while let Some(mut frame) = DecodeFrame::start(&mut r, first)? {
            zone!("decode frame");
            first = false;
            let dict = frame.header().dictionary(opt.dictionaries)?;
            while let Some(header) = read_data_block(&mut r, frame.header(), &mut self.body)? {
                zone!("decode block");
                let block = self.block.decode(header, &self.body, dict, frame.state.index, None)?;
                frame.commit(&mut w, header, block, &mut total, opt.max_output)?;
            }
            frame.finish(&mut r)?;
        }
        Ok(())
    }
}

/// Decode all concatenated frames, checking every present checksum. A later
/// error may leave earlier blocks in the writer. The caller owns flushing.
pub fn decode(r: impl Read, w: impl Write, opt: DecodeOptions<'_>) -> io::Result<()> {
    Decoder::default().decode(r, w, opt)
}
