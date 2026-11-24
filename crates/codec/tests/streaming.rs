use std::io::{self, Read, Write};
use zxcide::{DecodeOptions, Dictionary, EncodeOptions, Mode, decode, encode};

struct Drip<'a>(&'a [u8], bool);
impl Read for Drip<'_> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.1 = !self.1;
        if self.1 {
            return Err(io::ErrorKind::Interrupted.into());
        }
        let n = b.len().min(3);
        self.0.read(&mut b[..n])
    }
}
struct Short(Vec<u8>);
impl Write for Short {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let n = b.len().min(3);
        self.0.extend_from_slice(&b[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn boundaries_and_fragmented_io() {
    let mut seed = 0x123456789abcdefu64;
    let noise: Vec<u8> = (0..70000)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect();
    for mode in [Mode::Split, Mode::Packed, Mode::Dense] {
        for n in [0, 1, 127, 210, 215, 216, 217, 255, 256, 4095, 4096, 4097, 65535, 65536] {
            let opt = EncodeOptions {
                block_log: 12,
                mode,
                checksum: n % 2 == 0,
                seekable: n % 2 == 1,
                ..Default::default()
            };
            let plain = &noise[..n];
            let mut reference = Vec::new();
            encode(plain, &mut reference, opt).unwrap();
            let mut frame = Short(Vec::new());
            encode(Drip(plain, false), &mut frame, opt).unwrap();
            assert_eq!(frame.0, reference);
            let mut output = Short(Vec::new());
            decode(Drip(&frame.0, false), &mut output, Default::default()).unwrap();
            assert_eq!(output.0, plain);
        }
    }
}
#[test]
fn concatenation_resets_dictionary_and_index() {
    const V: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/conformance/v9/valid/");
    let load = |p: &str| std::fs::read(format!("{V}{p}")).unwrap();
    let ds = ["dict_text.zxd", "dict_id_zero.zxd"].map(|p| Dictionary::from_bytes(&load(p)).unwrap());
    let names = ["dict_seekable_l7", "multiblock_checksum", "empty", "dict_id_zero"];
    let data = names.map(|p| load(&format!("{p}.zxc"))).concat();
    let expected = names.map(|p| load(&format!("{p}.expected"))).concat();
    let opt = DecodeOptions {
        dictionaries: &ds,
        max_output: Some(expected.len() as u64),
    };
    let mut plain = Vec::new();
    decode(&data[..], &mut plain, opt).unwrap();
    assert_eq!(plain, expected);
    for garbage in [vec![0], vec![0; 3], vec![0; 4]] {
        assert!(decode(&[&data[..], &garbage].concat()[..], io::sink(), opt).is_err());
    }
    assert!(
        decode(
            &data[..],
            io::sink(),
            DecodeOptions {
                max_output: Some(expected.len() as u64 - 1),
                ..opt
            }
        )
        .is_err()
    );
}
#[test]
fn verification_precedes_block_write() {
    let mut frame = Vec::new();
    encode(&b"some short test input"[..], &mut frame, Default::default()).unwrap();
    for end in 0..frame.len() {
        assert!(decode(&frame[..end], io::sink(), Default::default()).is_err());
    }
    frame[24] ^= 1;
    let mut out = Vec::new();
    assert!(decode(&frame[..], &mut out, Default::default()).is_err());
    assert!(out.is_empty());
}
#[test]
fn density_sections_roundtrip() {
    const SRC: &[u8] = include_bytes!("../../../tests/conformance/v9/valid/spl_pivco_wide_l7.expected");
    let mut archive = Vec::new();
    encode(
        SRC,
        &mut archive,
        EncodeOptions {
            mode: Mode::Dense,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(archive[16], 1);
    assert_eq!(archive[32], 2, "expected inline Huffman literals");
    let mut plain = Vec::new();
    decode(&archive[..], &mut plain, Default::default()).unwrap();
    assert_eq!(plain, SRC);
}
