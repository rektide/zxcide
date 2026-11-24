use std::{cell::RefCell, fs, io, ops::Range};
use zxcide::{Dictionary, EncodeOptions, Mode, decode, encode};
use zxcide_seek::{Archive, ReadAt};

const VALID: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/conformance/v9/valid");
const INVALID: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/conformance/v9/invalid");

fn fixture(name: &str) -> Vec<u8> {
    fs::read(format!("{VALID}/{name}")).unwrap()
}

fn make_frame(plain: &[u8], checksum: bool, dictionary: Option<&Dictionary>) -> Vec<u8> {
    let mut frame = Vec::new();
    encode(
        plain,
        &mut frame,
        EncodeOptions {
            block_log: 12,
            mode: Mode::Dense,
            checksum,
            seekable: true,
            dictionary,
        },
    )
    .unwrap();
    frame
}

// Independent walk of a valid fixture, to get the physical read allowlist.
struct Layout {
    data: Vec<Range<usize>>,
    table: Range<usize>,
    footer: Range<usize>,
}

fn layout(frame: &[u8], base: usize) -> Layout {
    let checks = frame[6] & 128 != 0;
    let mut pos = 16;
    let mut data = Vec::new();
    while frame[pos] != 255 {
        let size = u32::from_le_bytes(frame[pos + 3..pos + 7].try_into().unwrap()) as usize;
        let end = pos + 8 + size + 4 * usize::from(checks);
        data.push(base + pos..base + end);
        pos = end;
    }
    let table = base + pos + 16;
    let footer = table + 8 * data.len().div_ceil(64) + 4 * data.len();
    Layout {
        data,
        table: table..footer,
        footer: footer..base + frame.len(),
    }
}

struct Counting<'a> {
    data: &'a [u8],
    allowed: RefCell<Vec<Range<usize>>>,
    reads: RefCell<Vec<Range<usize>>>,
}

impl ReadAt for Counting<'_> {
    fn len(&self) -> io::Result<u64> {
        Ok(self.data.len() as u64)
    }

    fn read_exact_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let start = offset as usize;
        let end = start + buf.len();
        assert!(
            self.allowed.borrow().iter().any(|r| r.start <= start && end <= r.end),
            "unrelated read of {start}..{end}; allowed: {:?}",
            self.allowed.borrow()
        );
        self.reads.borrow_mut().push(start..end);
        self.data.read_exact_at(offset, buf)
    }
}

#[test]
fn upstream_group_and_frame_boundaries_touch_only_their_own_data() {
    let a = fixture("seekable_2groups.zxc");
    let b = fixture("seekable_4blocks.zxc");
    let a_plain = fixture("seekable_2groups.expected");
    let b_plain = fixture("seekable_4blocks.expected");
    let boundary = a_plain.len();
    let plain = [a_plain, b_plain].concat();
    let (first, second) = (layout(&a, 0), layout(&b, a.len()));
    let bytes = [&a[..], &b[..]].concat();
    let source = Counting {
        data: &bytes,
        allowed: RefCell::new(vec![
            0..16,
            first.table.start - 16..first.table.start,
            first.footer.clone(),
            a.len()..a.len() + 16,
            second.table.start - 16..second.table.start,
            second.footer.clone(),
        ]),
        reads: RefCell::new(Vec::new()),
    };
    let archive = Archive::open(&source).unwrap();
    assert_eq!(archive.len(), plain.len() as u64);
    assert_eq!(archive.frames().len(), 2);
    let info = &archive.frames()[0];
    assert_eq!(
        (info.plain_size, info.compressed_size, info.block_size, info.blocks),
        (boundary as u64, a.len() as u64, 4096, 66)
    );
    assert!(!info.checksum);
    assert_eq!(info.dictionary_id, None);
    let split = first.table.start + 264;
    let table0 = first.table.start..split;
    let table1 = split..first.table.end;
    for (range, blocks, groups) in [
        (
            64 * 4096 - 7..64 * 4096 + 13,
            [first.data[63].clone(), first.data[64].clone()],
            [table0, table1.clone()],
        ),
        (
            boundary - 11..boundary + 9,
            [first.data[65].clone(), second.data[0].clone()],
            [table1, second.table.clone()],
        ),
    ] {
        source.allowed.replace([&blocks[..], &groups[..]].concat());
        source.reads.borrow_mut().clear();
        let mut output = Vec::new();
        archive
            .copy_range(range.start as u64..range.end as u64, &[], &mut output)
            .unwrap();
        assert_eq!(output, plain[range]);
        let reads = source.reads.borrow();
        for group in &groups {
            assert_eq!(reads.iter().filter(|&read| read == group).count(), 1, "group {group:?}");
        }
        for block in &blocks {
            let read_bytes: usize = reads
                .iter()
                .filter(|r| block.start <= r.start && r.end <= block.end)
                .map(Range::len)
                .sum();
            assert_eq!(read_bytes, block.len(), "full touched block {block:?}");
        }
    }
    source.allowed.borrow_mut().clear();
    archive
        .copy_range(archive.len()..archive.len(), &[], io::sink())
        .unwrap();
    let reversed = Range { start: 4, end: 2 };
    for range in [reversed, archive.len() + 1..archive.len() + 1, 0..u64::MAX] {
        assert_eq!(
            archive.copy_range(range, &[], io::sink()).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[test]
fn open_rejects_invalid_frame_structure_and_unindexed_frames() {
    let valid = fixture("seekable_4blocks.zxc");
    for name in [
        "bad_eof_compsize",
        "eof_flags_set",
        "eof_reserved_set",
        "sek_flag_no_table",
        "sek_flags_set",
        "sek_reserved_set",
        "sek_table_no_flag",
        "bad_footer_size",
        "all_0xff_garbage",
    ] {
        let bad = fs::read(format!("{INVALID}/{name}.zxc")).unwrap();
        assert!(Archive::open(bad).is_err(), "{name}");
    }
    let unindexed = fixture("empty.zxc");
    for bad in [
        &[][..],
        &valid[..valid.len() - 1],
        &valid[1..],
        &unindexed[..],
        &[&unindexed[..], &valid].concat(),
        &[&valid[..], &unindexed].concat(),
        &[&valid[..], &[0]].concat(),
    ] {
        assert!(Archive::open(bad).is_err());
    }
    let mut bad = valid;
    let end = bad.len() - 1;
    bad[end] |= 8;
    assert!(Archive::open(bad).is_err());
}

#[test]
fn seek_entries_are_checked_only_for_the_touched_group() {
    let forged = fs::read(format!("{INVALID}/sek_forged_entry.zxc")).unwrap();
    let archive = Archive::open(forged).unwrap();
    let mut output = Vec::new();
    assert!(archive.copy_range(0..1, &[], &mut output).is_err());
    assert!(output.is_empty());

    let fixture = fixture("seekable_2groups.zxc");
    let l = layout(&fixture, 0);
    let group1 = l.table.start + 264;
    let anchor1 = l.data[64].start as u64;
    for (offset, changed, reason) in [
        (l.table.start, 17u64.to_le_bytes().to_vec(), "invalid seek group anchor"),
        (
            l.table.start + 8,
            7u32.to_le_bytes().to_vec(),
            "invalid seek entry size",
        ),
        (
            l.table.start + 8,
            (l.data[0].len() as u32 + 1).to_le_bytes().to_vec(),
            "seek entry disagrees",
        ),
        (group1, (anchor1 + 1).to_le_bytes().to_vec(), "group runs past EOF"),
        (group1, (anchor1 - 1).to_le_bytes().to_vec(), "does not end at EOF"),
    ] {
        let mut bytes = fixture.clone();
        bytes[offset..offset + changed.len()].copy_from_slice(&changed);
        let archive = Archive::open(bytes).unwrap();
        let (bad, good) = if offset < group1 {
            (0, 64 * 4096)
        } else {
            (64 * 4096, 0)
        };
        let error = archive.copy_range(bad..bad + 1, &[], &mut output).unwrap_err();
        assert!(error.to_string().contains(reason), "{error}");
        assert!(output.is_empty());
        archive.copy_range(good..good + 1, &[], io::sink()).unwrap();
    }
}

#[test]
fn decoded_size_must_match_footer_before_writing() {
    let mut frame = fixture("seekable_4blocks.zxc");
    let footer = layout(&frame, 0).footer.start;
    // Reduce the final plaintext size by one without changing the block count
    // or the encoded footer widths.
    frame[footer..footer + 2].copy_from_slice(&32767u16.to_le_bytes());
    let archive = Archive::open(frame).unwrap();
    let mut output = Vec::new();
    let error = archive.copy_range(32766..32767, &[], &mut output).unwrap_err();
    assert!(error.to_string().contains("unexpected decoded block size"));
    assert!(output.is_empty());
}

#[test]
fn frame_local_dictionaries_checksums_and_empty_frames() {
    let dict = Dictionary::from_bytes(&fixture("dict_text.zxd")).unwrap();
    let zero_id = Dictionary::from_bytes(&fixture("dict_id_zero.zxd")).unwrap();
    let first = fixture("dict_seekable_l7.zxc");
    let empty = make_frame(&[], false, Some(&dict));
    let middle = make_frame(&[19; 8000], false, None);
    let last_plain = fixture("dict_id_zero.expected");
    let last = make_frame(&last_plain, true, Some(&zero_id));
    let archive = Archive::open([first, empty.clone(), middle, last, empty.clone()].concat()).unwrap();
    assert!(Archive::open(empty).unwrap().is_empty());
    let frames = archive.frames();
    assert_eq!(
        frames.iter().map(|f| f.dictionary_id).collect::<Vec<_>>(),
        [Some(dict.id()), Some(dict.id()), None, Some(0), Some(dict.id())]
    );
    assert_eq!(
        frames.iter().map(|f| f.checksum).collect::<Vec<_>>(),
        [true, false, false, true, false]
    );
    assert_eq!(frames[1].blocks, 0);
    let mut output = Vec::new();
    let error = archive.copy_range(0..1, &[], &mut output).unwrap_err();
    assert!(error.to_string().contains("dictionary"));
    assert!(output.is_empty());
    archive.copy_range(1024..1025, &[], &mut output).unwrap();
    assert_eq!(output, [19]);
    output.clear();
    let first_plain = fixture("dict_seekable_l7.expected");
    archive
        .copy_range(1015..archive.len(), &[dict, zero_id], &mut output)
        .unwrap();
    assert_eq!(output, [&first_plain[1015..], &[19; 8000], &last_plain].concat());
}

#[test]
fn checksums_are_verified_with_local_index_but_digest_is_not_checked() {
    let plain = [57; 8192];
    let bytes = make_frame(&plain, true, None);
    let l = layout(&bytes, 0);
    let mut corrupted = bytes.clone();
    corrupted[l.data[1].end - 1] ^= 1;
    let archive = Archive::open(corrupted).unwrap();
    let mut output = Vec::new();
    assert!(
        archive
            .copy_range(4096..4097, &[], &mut output)
            .unwrap_err()
            .to_string()
            .contains("checksum")
    );
    assert!(output.is_empty());
    archive.copy_range(0..1, &[], &mut output).unwrap();
    assert_eq!(output, [57]);

    // Two equal blocks have equal payloads but their trailers bind their positions.
    let mut swapped = bytes.clone();
    swapped[l.data[0].clone()].copy_from_slice(&bytes[l.data[1].clone()]);
    swapped[l.data[1].clone()].copy_from_slice(&bytes[l.data[0].clone()]);
    let archive = Archive::open(swapped).unwrap();
    assert!(
        archive
            .copy_range(0..1, &[], io::sink())
            .unwrap_err()
            .to_string()
            .contains("checksum")
    );

    let mut wrong_digest = bytes.clone();
    wrong_digest[l.footer.start] ^= 1;
    assert!(decode(&wrong_digest[..], io::sink(), Default::default()).is_err());
    let archive = Archive::open([bytes, wrong_digest].concat()).unwrap();
    output.clear();
    archive.copy_range(8191..8193, &[], &mut output).unwrap();
    assert_eq!(output, [57; 2]);
}

#[cfg(unix)]
#[test]
fn positioned_files_never_change_the_cursor_and_can_be_shared() {
    use std::{
        fs::File,
        io::{Seek, SeekFrom},
    };

    let mut file = File::open(format!("{VALID}/seekable_4blocks.zxc")).unwrap();
    file.seek(SeekFrom::Start(7)).unwrap();
    let archive = Archive::open(file.try_clone().unwrap()).unwrap();
    assert_eq!(file.stream_position().unwrap(), 7);
    let expected = fixture("seekable_4blocks.expected");
    std::thread::scope(|scope| {
        for start in [0, 8191, 24332] {
            let archive = &archive;
            let plain = &expected[start..start + 17];
            scope.spawn(move || {
                let mut output = Vec::new();
                archive
                    .copy_range(start as u64..start as u64 + 17, &[], &mut output)
                    .unwrap();
                assert_eq!(&output, plain);
            });
        }
    });
    assert_eq!(file.stream_position().unwrap(), 7);
}
