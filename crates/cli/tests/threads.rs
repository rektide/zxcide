use std::{fs, process::Command};

#[test]
fn parallel_limit_failure_never_publishes_a_partial_file() {
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("result");
    let archive = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/conformance/v9/valid/multiblock_checksum.zxc"
    );
    let res = Command::new(env!("CARGO_BIN_EXE_zxc"))
        .args([
            "decode",
            archive,
            "--threads",
            "2",
            "--slots",
            "3",
            "--max-output",
            "4096",
            "-o",
        ])
        .arg(&dst)
        .output()
        .unwrap();
    assert!(!res.status.success());
    assert!(String::from_utf8_lossy(&res.stderr).contains("limit"));
    assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
}

#[test]
fn bad_footer_after_a_compatible_frame_does_not_publish() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("concat.zxc");
    let dest = dir.path().join("result");
    let frame = include_bytes!("../../../tests/conformance/v9/valid/multiblock_checksum.zxc");
    let mut concat = [frame.as_slice(), frame, frame].concat();
    concat[2 * frame.len() - 1] ^= 1;
    fs::write(&source, concat).unwrap();
    let res = Command::new(env!("CARGO_BIN_EXE_zxc"))
        .arg("decode")
        .arg(&source)
        .args(["--threads", "3", "--slots", "2", "-o"])
        .arg(&dest)
        .output()
        .unwrap();
    assert!(!res.status.success());
    assert!(String::from_utf8_lossy(&res.stderr).contains("frame footer or digest mismatch"));
    let entries: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(entries, [source]);
}
