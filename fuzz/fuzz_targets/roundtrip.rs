#![no_main]
use libfuzzer_sys::fuzz_target;
use zxcide::{EncodeOptions, Mode, decode, encode};

fuzz_target!(|data: &[u8]| {
    let Some((&flags, bytes)) = data.split_first() else {
        return;
    };
    let mut encoded = Vec::new();
    let opt = EncodeOptions {
        block_log: 12,
        mode: [Mode::Split, Mode::Packed, Mode::Dense][usize::from(flags) % 3],
        checksum: flags & 4 != 0,
        seekable: flags & 8 != 0,
        dictionary: None,
    };
    encode(bytes, &mut encoded, opt).unwrap();
    let mut plain = Vec::new();
    decode(&encoded[..], &mut plain, Default::default()).unwrap();
    assert_eq!(bytes, plain);
});
