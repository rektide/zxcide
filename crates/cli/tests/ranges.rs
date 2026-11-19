use std::{fs, path::Path, process::Command};

const VALID: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/conformance/v9/valid");

#[test]
fn indexed_cli_uses_plaintext_offsets_and_frame_local_dictionaries() {
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("multi.zxc");
    let input = Path::new(VALID);
    let read = |name: &str| fs::read(input.join(name)).unwrap();
    fs::write(
        &archive,
        [read("seekable_2groups.zxc"), read("dict_seekable_l7.zxc")].concat(),
    )
    .unwrap();
    let first = read("seekable_2groups.expected");
    let second = read("dict_seekable_l7.expected");
    let cli = || Command::new(env!("CARGO_BIN_EXE_zxc"));
    let info = cli().arg("info").arg(&archive).output().unwrap();
    assert!(info.status.success(), "{}", String::from_utf8_lossy(&info.stderr));
    assert!(String::from_utf8(info.stdout).unwrap().contains(&format!(
        "2 indexed frame(s), {} uncompressed bytes",
        first.len() + second.len()
    )));
    // Failure only becomes visible after the first three plaintext bytes; an
    // incomplete result must not appear at a named destination.
    let out = dir.path().join("part");
    let range = || {
        let mut c = cli();
        c.arg("range")
            .arg(&archive)
            .arg((first.len() - 3).to_string())
            .args(["8", "-o"])
            .arg(&out);
        c
    };
    assert!(!range().output().unwrap().status.success());
    assert!(!out.exists());
    let result = range().arg("-D").arg(input.join("dict_text.zxd")).output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    assert_eq!(
        fs::read(out).unwrap(),
        [&first[first.len() - 3..], &second[..5]].concat()
    );
}

#[test]
fn rejects_offset_wrap_without_emitting_bytes() {
    let result = Command::new(env!("CARGO_BIN_EXE_zxc"))
        .arg("range")
        .arg(Path::new(VALID).join("seekable_4blocks.zxc"))
        .arg(u64::MAX.to_string())
        .arg("1")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("overflows"));
    assert!(result.stdout.is_empty());
}
