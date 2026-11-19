use std::{fs, io, path::Path};
use zxc_codec::{DecodeOptions, Dictionary, decode};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/conformance/v9");

#[test]
fn reference_decoding() {
    let valid = Path::new(ROOT).join("valid");
    let dicts = ["dict_http", "dict_text", "dict_id_zero"].map(|s| Dictionary::from_bytes(&fs::read(valid.join(format!("{s}.zxd"))).unwrap()).unwrap());
    let mut count = 0;
    for f in fs::read_dir(&valid).unwrap() {
        let p = f.unwrap().path();
        if !p.extension().is_some_and(|e| e == "zxc") {
            continue;
        }
        let mut plain = Vec::new();
        decode(
            fs::File::open(&p).unwrap(),
            &mut plain,
            DecodeOptions {
                dictionaries: &dicts,
                max_output: Some(1 << 21),
            },
        )
        .unwrap_or_else(|e| panic!("{p:?}: {e}"));
        assert!(plain == fs::read(p.with_extension("expected")).unwrap(), "{p:?}: plaintext mismatch");
        count += 1;
    }
    assert_eq!(count, 18);
}
#[test]
fn reference_rejections() {
    let valid = Path::new(ROOT).join("valid");
    let dicts = [Dictionary::from_bytes(&fs::read(valid.join("dict_http.zxd")).unwrap()).unwrap()];
    let mut count = 0;
    for f in fs::read_dir(Path::new(ROOT).join("invalid")).unwrap() {
        let p = f.unwrap().path();
        let name = p.file_stem().unwrap().to_str().unwrap();
        let opts = DecodeOptions {
            dictionaries: if name == "dict_id_mismatch" { &dicts } else { &[] },
            max_output: Some(1 << 21),
        };
        let result = decode(fs::File::open(&p).unwrap(), io::sink(), opts);
        // This vector only corrupts a seek entry. Sequential readers skip entries.
        assert_eq!(result.is_ok(), name == "sek_forged_entry", "{name}: {result:?}");
        count += 1;
    }
    assert_eq!(count, 35);
}
