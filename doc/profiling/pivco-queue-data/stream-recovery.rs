use std::{fs, io, path::Path};
use zxc_codec::{DecodeOptions, Decoder, Dictionary};

fn main() {
    let fixtures = std::env::args_os().nth(1).unwrap();
    let fixtures = Path::new(&fixtures);
    let dictionaries = [Dictionary::from_bytes(&fs::read(fixtures.join("traversal.zxd")).unwrap()).unwrap()];
    let archive = fs::read(fixtures.join("dict-many.zxc")).unwrap();
    let plain = fs::read(fixtures.join("dict-many")).unwrap();
    let mut decoder = Decoder::default();
    let mut output = Vec::new();
    let opt = DecodeOptions { dictionaries: &dictionaries, max_output: None };
    let mut bad = archive.clone();
    // Block size excludes its 8-byte physical header and 4-byte checksum.
    let block_payload = |at: usize| u32::from_le_bytes(archive[at + 3..at + 7].try_into().unwrap()) as usize;
    let second = 16 + 8 + block_payload(16) + 4;
    bad[second + 8 + block_payload(second)] ^= 1;
    for (input, options, message) in [
        (&archive, DecodeOptions { max_output: Some(8191), ..opt }, "decoded output limit exceeded"),
        (&bad, opt, "block checksum mismatch"),
    ] {
        let err = decoder.decode(&input[..], &mut output, options).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(err.to_string(), message);
        assert_eq!(output, plain[..4096]); // first verified block only
        output.clear();
        decoder.decode(&archive[..], &mut output, opt).unwrap();
        assert_eq!(output, plain);
        output.clear();
    }
    println!("cumulative limit and late checksum rejected block two only; each recovered through EOF on the same Decoder");
}
