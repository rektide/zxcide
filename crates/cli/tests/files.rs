use std::{
    fs,
    process::{Command, Stdio},
};

#[test]
fn failed_decode_never_publishes_or_clobbers_output() {
    let dir = tempfile::tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_zxc");
    let src = dir.path().join("input");
    let dst = dir.path().join("output");
    fs::write(&src, b"hello").unwrap();
    let arch = Command::new(exe).arg("encode").arg(&src).output().unwrap();
    assert!(arch.status.success());
    let check = || {
        Command::new(exe)
            .arg("decode")
            .arg(&src)
            .arg("-o")
            .arg(&dst)
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    fs::write(&src, &arch.stdout[..arch.stdout.len() - 1]).unwrap();
    assert!(!check());
    assert!(!dst.exists());
    fs::write(&src, arch.stdout).unwrap();
    assert!(check());
    assert_eq!(fs::read(&dst).unwrap(), b"hello");
    fs::write(&dst, b"keep").unwrap();
    assert!(!check());
    assert_eq!(fs::read(&dst).unwrap(), b"keep");
}
