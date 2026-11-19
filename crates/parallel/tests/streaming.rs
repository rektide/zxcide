use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
    rc::Rc,
};
use zxc_codec::{
    DecodeOptions, Dictionary, EncodeOptions, Mode,
    wire::{BLOCK_HEADER_SIZE, DataBlockHeader, FRAME_HEADER_SIZE, FrameHeader},
};
use zxc_parallel::{Backend, Config, decode, encode};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/conformance/v9");
const BLOCK: usize = 4096;

fn config(backend: Backend, workers: usize, slots: usize) -> Config {
    Config {
        workers,
        slots,
        backend,
    }
}

fn fixtures(name: &str) -> Vec<u8> {
    fs::read(Path::new(FIXTURES).join("valid").join(name)).unwrap()
}

fn dictionaries() -> [Dictionary; 3] {
    ["dict_http.zxd", "dict_text.zxd", "dict_id_zero.zxd"].map(|name| Dictionary::from_bytes(&fixtures(name)).unwrap())
}

fn serial(plain: &[u8], options: EncodeOptions<'_>) -> Vec<u8> {
    let mut archive = Vec::new();
    zxc_codec::encode(plain, &mut archive, options).unwrap();
    archive
}

fn source() -> Vec<u8> {
    let text = fixtures("dict_seekable_l7.expected");
    let mut seed = 0xf12345ad4b67b89e_u64;
    (0..BLOCK * 65 + 37)
        .map(|i| {
            // Mix high-entropy RAW candidates and dictionary-friendly blocks.
            if (i / BLOCK).is_multiple_of(7) {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed as u8
            } else {
                text[i % text.len()]
            }
        })
        .collect()
}

#[test]
fn byte_identical_to_serial_for_all_frame_settings() {
    let plain = source();
    let dictionaries = dictionaries();
    for mode in [Mode::Split, Mode::Packed, Mode::Dense] {
        for checksum in [false, true] {
            for seekable in [false, true] {
                for dictionary in [None, Some(&dictionaries[1]), Some(&dictionaries[2])] {
                    let options = EncodeOptions {
                        block_log: 12,
                        mode,
                        checksum,
                        seekable,
                        dictionary,
                    };
                    let expected = serial(&plain, options);
                    for &backend in Backend::available() {
                        let mut archive = Vec::new();
                        encode(&plain[..], &mut archive, options, config(backend, 3, 5)).unwrap();
                        assert!(archive == expected, "{backend:?}, {options:?}");
                        let mut output = Vec::new();
                        decode(
                            &archive[..],
                            &mut output,
                            DecodeOptions {
                                dictionaries: &dictionaries,
                                max_output: Some(plain.len() as u64),
                            },
                            config(backend, 5, 3),
                        )
                        .unwrap();
                        assert!(output == plain, "{backend:?}, {options:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn matches_reference_vectors_and_serial_rejections() {
    let root = Path::new(FIXTURES);
    let dictionaries = dictionaries();
    let options = DecodeOptions {
        dictionaries: &dictionaries,
        max_output: Some(1 << 21),
    };
    for dir in ["valid", "invalid"] {
        for file in fs::read_dir(root.join(dir)).unwrap() {
            let path = file.unwrap().path();
            if !path.extension().is_some_and(|e| e == "zxc") {
                continue;
            }
            let archive = fs::read(&path).unwrap();
            let mut expected = Vec::new();
            let status = if dir == "valid" {
                expected = fs::read(path.with_extension("expected")).unwrap();
                Ok(())
            } else {
                zxc_codec::decode(&archive[..], &mut expected, options).map_err(|e| (e.kind(), e.to_string()))
            };
            for &backend in Backend::available() {
                let mut output = Vec::new();
                let result = decode(&archive[..], &mut output, options, config(backend, 3, 4));
                assert_eq!(
                    result.map_err(|e| (e.kind(), e.to_string())),
                    status,
                    "{backend:?} {path:?}"
                );
                assert!(output == expected, "{backend:?} {path:?}");
            }
        }
    }
}

#[test]
fn frame_boundaries_dictionary_changes_and_cumulative_limit() {
    let dictionaries = dictionaries();
    let names = ["dict_seekable_l7", "multiblock_checksum", "empty", "dict_id_zero"];
    let frames = names.map(|name| fixtures(&format!("{name}.zxc")));
    let outputs = names.map(|name| fixtures(&format!("{name}.expected")));
    let archive = frames.concat();
    let expected = outputs.concat();
    for &backend in Backend::available() {
        for (workers, slots) in [(1, 3), (2, 1), (3, 2), (4, 7)] {
            for limit in [expected.len(), expected.len() - 1] {
                let mut output = Vec::new();
                let result = decode(
                    &archive[..],
                    &mut output,
                    DecodeOptions {
                        dictionaries: &dictionaries,
                        max_output: Some(limit as u64),
                    },
                    config(backend, workers, slots),
                );
                if limit == expected.len() {
                    result.unwrap();
                    assert_eq!(output, expected);
                } else {
                    assert_eq!(result.unwrap_err().to_string(), "decoded output limit exceeded");
                    assert_eq!(output, outputs[..3].concat());
                }
            }
        }
    }
}

#[test]
fn corruption_and_read_failure_report_in_order() {
    let plain = source();
    let options = EncodeOptions {
        block_log: 12,
        ..EncodeOptions::default()
    };
    let mut archive = serial(&plain[..BLOCK * 4], options);
    let settings = FrameHeader::parse(archive[..FRAME_HEADER_SIZE].try_into().unwrap()).unwrap();
    let mut p = FRAME_HEADER_SIZE;
    let starts = std::array::from_fn::<_, 4, _>(|_| {
        let start = p;
        p += DataBlockHeader::parse(archive[p..p + BLOCK_HEADER_SIZE].try_into().unwrap(), settings)
            .unwrap()
            .disk_size();
        start
    });
    // Block 1 fails in a worker. Block 3 fails synchronously during admission
    // (invalid header). Either may be noticed first, but block 1 must win.
    archive[starts[2] - 1] ^= 1;
    archive[starts[3] + 7] ^= 1;
    for &backend in Backend::available() {
        for (input, kind, reason) in [
            (
                &archive[..],
                io::ErrorKind::InvalidData,
                Some("block checksum mismatch"),
            ),
            (&archive[..starts[1] + 1], io::ErrorKind::UnexpectedEof, None),
        ] {
            let mut output = Vec::new();
            let error = decode(input, &mut output, DecodeOptions::default(), config(backend, 3, 5)).unwrap_err();
            assert_eq!(error.kind(), kind);
            if let Some(reason) = reason {
                assert_eq!(error.to_string(), reason);
            }
            assert_eq!(output, plain[..BLOCK]);
        }
    }
}

struct Fragmented<'a>(&'a [u8], Rc<()>, bool);
impl Read for Fragmented<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.2 = !self.2;
        if self.2 {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let n = buf.len().min(Rc::strong_count(&self.1) + 23);
        self.0.read(&mut buf[..n])
    }
}

struct ShortWriter(Rc<()>, Vec<u8>);
impl Write for ShortWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = buf.len().min(Rc::strong_count(&self.0) + 19);
        self.1.extend_from_slice(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("only the caller flushes")
    }
}

#[test]
fn short_non_send_io_and_single_worker_and_partial_windows() {
    let plain = source();
    let options = EncodeOptions {
        block_log: 12,
        seekable: true,
        ..EncodeOptions::default()
    };
    for &(workers, slots, len) in &[
        (1, 1, 0),
        (1, 9, BLOCK * 2),
        (2, 3, 0),
        (2, 1, BLOCK * 2),
        (3, 5, BLOCK + 19),
    ] {
        let plain = &plain[..len];
        let expected = serial(plain, options);
        for &backend in Backend::available() {
            let cfg = config(backend, workers, slots);
            let mut archive = ShortWriter(Rc::new(()), Vec::new());
            encode(
                Fragmented(plain, Rc::new(()), false),
                &mut archive as &mut dyn Write,
                options,
                cfg,
            )
            .unwrap();
            assert_eq!(archive.1, expected);
            let mut output = ShortWriter(Rc::new(()), Vec::new());
            decode(
                Fragmented(&archive.1, Rc::new(()), false),
                &mut output,
                DecodeOptions::default(),
                cfg,
            )
            .unwrap();
            assert_eq!(output.1, plain);
        }
    }
}

#[test]
fn rejects_invalid_configuration_without_io_even_in_serial() {
    let mut invalid = vec![
        config(Backend::Crossbeam, 0, 1),
        config(Backend::Crossbeam, Config::MAX_WORKERS + 1, 1),
        config(Backend::Crossbeam, 1, 0),
        config(Backend::Crossbeam, 1, Config::MAX_SLOTS + 1),
    ];
    for backend in [Backend::Flume, Backend::Kanal, Backend::Crossfire, Backend::Rayon] {
        if !Backend::available().contains(&backend) {
            invalid.push(config(backend, 1, 1));
            invalid.push(config(backend, 2, 2));
        }
    }
    for cfg in invalid {
        let mut out = Vec::new();
        let err = encode(&b"source"[..], &mut out, EncodeOptions::default(), cfg).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(out.is_empty());
        // An empty reader would produce UnexpectedEof if framing preceded validation.
        let err = decode(io::empty(), io::sink(), DecodeOptions::default(), cfg).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}
