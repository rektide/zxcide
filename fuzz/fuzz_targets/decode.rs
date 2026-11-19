#![no_main]
use libfuzzer_sys::fuzz_target;
use zxc_codec::{DecodeOptions, Dictionary, decode};

fuzz_target!(|data: &[u8]| {
    let _ = Dictionary::from_bytes(data);
    let _ = decode(
        data,
        std::io::sink(),
        DecodeOptions {
            max_output: Some(8 << 20),
            ..Default::default()
        },
    );
});
