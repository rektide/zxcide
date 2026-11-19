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
