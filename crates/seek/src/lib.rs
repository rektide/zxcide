//! Positioned reads of indexed ZXC v9 frames, including concatenated archives.
#![forbid(unsafe_code)]

use std::{
    fs::File,
    io::{self, Write},
    ops::Range,
    path::Path,
};
use zxcide::{
    BlockDecoder, Dictionary,
    wire::{
        BLOCK_HEADER_SIZE, DIGEST_SIZE, DataBlockHeader, FRAME_HEADER_SIZE, FooterWidths, FrameHeader,
        SEEK_ANCHOR_SIZE, SEEK_ENTRY_SIZE, SEEK_GROUP_BLOCKS, seek_table_len, validate_eof_header,
        validate_seek_header,
    },
};

const TAIL_HEADERS_SIZE: usize = 2 * BLOCK_HEADER_SIZE;
const GROUP_SIZE: usize = SEEK_ANCHOR_SIZE + SEEK_GROUP_BLOCKS * SEEK_ENTRY_SIZE;
const MIN_FRAME_SIZE: u64 = (FRAME_HEADER_SIZE + TAIL_HEADERS_SIZE + 3) as u64;
const MAX_FRAMES: usize = 1 << 20;

/// A source with a length and exact, cursor-independent positioned reads.
/// Keep its length and contents stable for the entire life of an [`Archive`].
/// Sharing an archive across threads requires a `Sync` source.
pub trait ReadAt {
    fn len(&self) -> io::Result<u64>;

    fn is_empty(&self) -> io::Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Read exactly `buf.len()` bytes at `offset`, or return an error. Do not
    /// read into the destination from a shared cursor or change such a cursor.
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
}

impl<T: ReadAt + ?Sized> ReadAt for &T {
    fn len(&self) -> io::Result<u64> {
        T::len(self)
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        T::read_exact_at(self, offset, buf)
    }
}

impl ReadAt for [u8] {
    fn len(&self) -> io::Result<u64> {
        Ok(self.len() as u64)
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let input = usize::try_from(offset)
            .ok()
            .and_then(|offset| self.get(offset..))
            .and_then(|tail| tail.get(..buf.len()))
            .ok_or(io::ErrorKind::UnexpectedEof)?;
        buf.copy_from_slice(input);
        Ok(())
    }
}

impl ReadAt for Vec<u8> {
    fn len(&self) -> io::Result<u64> {
        ReadAt::len(self.as_slice())
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.as_slice().read_exact_at(offset, buf)
    }
}

/// Uses cursor-independent `FileExt` on Unix. For a portable file-backed
/// source, use [`FileSource::open`].
#[cfg(unix)]
impl ReadAt for File {
    fn len(&self) -> io::Result<u64> {
        Ok(self.metadata()?.len())
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        std::os::unix::fs::FileExt::read_exact_at(self, buf, offset)
    }
}

/// A privately opened file. Unix uses pread; the portable fallback serializes
/// seek + read under one lock on a cursor never exposed to callers. Decoding
/// always happens outside the lock.
#[derive(Debug)]
pub struct FileSource {
    #[cfg(unix)]
    file: File,
    #[cfg(not(unix))]
    file: std::sync::Mutex<File>,
    length: u64,
}
impl FileSource {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = File::open(path)?;
        let length = file.metadata()?.len();
        #[cfg(not(unix))]
        let file = std::sync::Mutex::new(file);
        Ok(Self { file, length })
    }
}
impl ReadAt for FileSource {
    fn len(&self) -> io::Result<u64> {
        Ok(self.length)
    }
    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        #[cfg(unix)]
        {
            ReadAt::read_exact_at(&self.file, offset, buf)
        }
        #[cfg(not(unix))]
        {
            use io::{Read, Seek, SeekFrom};
            let mut f = self.file.lock().map_err(|_| io::Error::other("file cursor poisoned"))?;
            f.seek(SeekFrom::Start(offset))?;
            f.read_exact(buf)
        }
    }
}

/// Metadata of one frame, in archive order. Reported sizes are not a digest or
/// proof of payload integrity.
#[derive(Clone, Debug)]
pub struct FrameInfo {
    pub plain_size: u64,
    pub compressed_size: u64,
    pub block_size: usize,
    pub blocks: u64,
    pub checksum: bool,
    pub dictionary_id: Option<u32>,
    header: FrameHeader,
    disk_start: u64,
    plain_end: u64,
    eof: u64, // Relative to disk_start, as are the group's anchors.
}

/// An immutable index of the archive's frames. Seek entries remain on the source.
#[derive(Debug)]
pub struct Archive<S> {
    source: S,
    frames: Vec<FrameInfo>,
    len: u64,
}

impl<S: ReadAt> Archive<S> {
    /// Open one or more concatenated frames, all with seek tables, and validate
    /// the frame/tail headers and footer sizes. Does not load any seek entries,
    /// decode any blocks, check frame digests, or require dictionaries.
    /// Archives with more than 1,048,576 frames are rejected.
    pub fn open(source: S) -> io::Result<Self> {
        #[cfg(feature = "tracy")]
        let _zone = tracy_client::span!("open indexed archive");
        let mut end = source.len()?;
        if end == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "empty ZXC source"));
        }
        let mut frames = Vec::new();
        let mut len = 0u64;
        while end != 0 {
            require(frames.len() < MAX_FRAMES, "too many ZXC frames")?;
            let frame = scan(&source, end)?;
            len = len
                .checked_add(frame.plain_size)
                .ok_or_else(|| bad("archive plain size overflow"))?;
            end = frame.disk_start;
            frames.push(frame);
        }
        frames.reverse();
        let mut plain_end = 0;
        for frame in &mut frames {
            plain_end += frame.plain_size; // Total already checked above.
            frame.plain_end = plain_end;
        }
        Ok(Self { source, frames, len })
    }

    /// Total uncompressed size across all frames.
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn frames(&self) -> &[FrameInfo] {
        &self.frames
    }

    /// Copy an archive-relative, half-open range of uncompressed bytes. An empty
    /// in-bounds range succeeds without I/O or dictionary resolution. Each touched
    /// block is checked before writing any of it, but a later error can leave
    /// earlier blocks in the writer. This does not verify the whole-frame digest.
    /// Without block checksums, a self-consistent but incorrect seek table may
    /// redirect reads to other valid blocks of the same size.
    pub fn copy_range(&self, range: Range<u64>, dictionaries: &[Dictionary], mut writer: impl Write) -> io::Result<()> {
        #[cfg(feature = "tracy")]
        let _zone = tracy_client::span!("copy archive range");
        if range.start > range.end || range.end > self.len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid uncompressed range",
            ));
        }
        if range.is_empty() {
            return Ok(());
        }
        let first_frame = self.frames.partition_point(|frame| frame.plain_end <= range.start);
        let mut decoder = BlockDecoder::default();
        let mut body = Vec::new();
        for frame in &self.frames[first_frame..] {
            let frame_start = frame.plain_end - frame.plain_size;
            if frame_start >= range.end {
                break;
            }
            if frame.plain_size == 0 {
                continue;
            }
            // Plain offsets and block indices are relative to this frame.
            let start = range.start.saturating_sub(frame_start);
            let end = range.end.min(frame.plain_end) - frame_start;
            let dictionary = frame.header.dictionary(dictionaries)?;
            let block_size = frame.block_size as u64;
            let mut index = start / block_size;
            let end_index = end.div_ceil(block_size);
            while index < end_index {
                let group_first = index / SEEK_GROUP_BLOCKS as u64 * SEEK_GROUP_BLOCKS as u64;
                let spans = load_group(&self.source, frame, group_first)?;
                let group_end = (group_first + SEEK_GROUP_BLOCKS as u64).min(end_index);
                while index < group_end {
                    #[cfg(feature = "tracy")]
                    let _zone = tracy_client::span!("read indexed block");
                    let i = (index - group_first) as usize;
                    let offset = spans[i];
                    let header = DataBlockHeader::parse(read(&self.source, offset)?, frame.header)?;
                    require(
                        header.disk_size() as u64 == spans[i + 1] - offset,
                        "seek entry disagrees with block header",
                    )?;
                    zxcide::resize_zero!(body, header.body_size());
                    self.source
                        .read_exact_at(offset + BLOCK_HEADER_SIZE as u64, &mut body)?;
                    zxcide::probe!({ add("input:positioned body", body.len()) });
                    let plain_offset = index * block_size;
                    let expected = (frame.plain_size - plain_offset).min(block_size);
                    let verified = decoder.decode(header, &body, dictionary, index, Some(expected as usize))?;
                    let from = start.saturating_sub(plain_offset) as usize;
                    let to = (end - plain_offset).min(expected) as usize;
                    writer.write_all(&verified.bytes[from..to])?;
                    index += 1;
                }
            }
        }
        Ok(())
    }
}

fn scan(source: &impl ReadAt, end: u64) -> io::Result<FrameInfo> {
    require(end >= MIN_FRAME_SIZE, "short indexed frame")?;
    let widths = FooterWidths::parse(read::<1>(source, end - 1)?[0])?;
    let sizes_start = end - 1 - widths.size_bytes() as u64;
    let mut sizes = [0; 16];
    let sizes = &mut sizes[..widths.size_bytes()];
    source.read_exact_at(sizes_start, sizes)?;
    let (plain_size, compressed_size) = widths.sizes(sizes)?;
    require(
        compressed_size >= MIN_FRAME_SIZE && compressed_size <= end,
        "invalid compressed frame bounds",
    )?;
    let disk_start = end - compressed_size;
    let header = FrameHeader::parse(read(source, disk_start)?)?;
    require(header.seekable(), "ZXC frame has no seek table")?;
    let blocks = plain_size.div_ceil(header.block_size() as u64);
    let table_len = seek_table_len(blocks)?;
    let digest_size = if header.checksum() { DIGEST_SIZE } else { 0 };
    let tail = sizes_start - disk_start; // The minimum frame size exceeds the largest size-field footer.
    let eof = tail
        .checked_sub(table_len + (TAIL_HEADERS_SIZE + digest_size) as u64)
        .filter(|&eof| eof >= FRAME_HEADER_SIZE as u64)
        .ok_or_else(|| bad("invalid indexed frame layout"))?;
    let data_len = eof - FRAME_HEADER_SIZE as u64;
    let overhead = header.block_overhead() as u64;
    require(
        blocks <= data_len / overhead && data_len <= blocks.saturating_mul(overhead + header.block_size() as u64),
        "frame data size disagrees with block count",
    )?;
    let [eof_header, seek_header] = read::<TAIL_HEADERS_SIZE>(source, disk_start + eof)?
        .as_chunks::<BLOCK_HEADER_SIZE>()
        .0
        .try_into()
        .unwrap();
    validate_eof_header(eof_header)?;
    validate_seek_header(seek_header, table_len)?;
    Ok(FrameInfo {
        plain_size,
        compressed_size,
        block_size: header.block_size(),
        blocks,
        checksum: header.checksum(),
        dictionary_id: header.dictionary_id(),
        header,
        disk_start,
        plain_end: 0,
        eof,
    })
}

fn load_group(source: &impl ReadAt, frame: &FrameInfo, first: u64) -> io::Result<[u64; SEEK_GROUP_BLOCKS + 1]> {
    #[cfg(feature = "tracy")]
    let _zone = tracy_client::span!("load seek group");
    let group = first / SEEK_GROUP_BLOCKS as u64;
    let count = (frame.blocks - first).min(SEEK_GROUP_BLOCKS as u64) as usize;
    let mut buf = [0; GROUP_SIZE];
    let entries_end = SEEK_ANCHOR_SIZE + count * SEEK_ENTRY_SIZE;
    let table_start = frame.disk_start + frame.eof + TAIL_HEADERS_SIZE as u64;
    source.read_exact_at(table_start + group * GROUP_SIZE as u64, &mut buf[..entries_end])?;
    let anchor = u64::from_le_bytes(buf[..SEEK_ANCHOR_SIZE].try_into().unwrap());
    let header_end = FRAME_HEADER_SIZE as u64;
    require(
        (header_end..=frame.eof).contains(&anchor) && (group != 0 || anchor == header_end),
        "invalid seek group anchor",
    )?;
    let mut offsets = [0; SEEK_GROUP_BLOCKS + 1];
    let mut pos = anchor;
    offsets[0] = frame.disk_start + pos;
    let min = frame.header.block_overhead() as u64;
    for (i, &size) in buf[SEEK_ANCHOR_SIZE..entries_end]
        .as_chunks::<SEEK_ENTRY_SIZE>()
        .0
        .iter()
        .enumerate()
    {
        let size = u32::from_le_bytes(size) as u64;
        require(
            (min..=min + frame.block_size as u64).contains(&size),
            "invalid seek entry size",
        )?;
        require(size <= frame.eof - pos, "seek group runs past EOF block")?;
        pos += size;
        offsets[i + 1] = frame.disk_start + pos;
    }
    require(
        first + count as u64 != frame.blocks || pos == frame.eof,
        "last seek group does not end at EOF block",
    )?;
    Ok(offsets)
}

fn read<const N: usize>(source: &impl ReadAt, offset: u64) -> io::Result<[u8; N]> {
    let mut buf = [0; N];
    source.read_exact_at(offset, &mut buf)?;
    Ok(buf)
}

fn require(ok: bool, message: &'static str) -> io::Result<()> {
    if ok { Ok(()) } else { Err(bad(message)) }
}

fn bad(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
