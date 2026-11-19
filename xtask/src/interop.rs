use anyhow::{Context, Result, ensure};
use std::{ffi::OsString, path::Path};
use xshell::{Shell, cmd};

pub fn check(sh: &Shell, c: &Path, rust: &Path) -> Result<()> {
    let valid = Path::new("tests/conformance/v9/valid");
    let mut checks = 0;
    for path in sh.read_dir(valid)? {
        if path.extension().is_none_or(|ext| ext != "expected") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|v| v.to_str())
            .context("invalid fixture name")?;
        let plain = sh.read_binary_file(&path)?;
        let dict = match name {
            "dict_http" | "dict_no_checksum" => Some("dict_http.zxd"),
            "dict_seekable_l7" => Some("dict_text.zxd"),
            "dict_id_zero" => Some("dict_id_zero.zxd"),
            _ => None,
        }
        .map(|file| [OsString::from("-D"), valid.join(file).into_os_string()]);
        let dict = dict.as_ref().map_or(&[][..], |args| args.as_slice());
        for (mode, flags) in [
            ("split", &["--seekable", "--block-log", "12"][..]),
            ("packed", &["--no-checksum"]),
            ("dense", &[]),
        ] {
            let archive = cmd!(sh, "{rust} encode --mode {mode} {flags...} {dict...}")
                .stdin(&plain)
                .output()
                .with_context(|| format!("Rust encode: {name}, {mode}"))?;
            let decoded = cmd!(sh, "{c} -qdc -T1 {dict...}")
                .stdin(archive.stdout)
                .output()
                .with_context(|| format!("C decode: {name}, {mode}"))?;
            ensure!(decoded.stdout == plain, "C decode mismatch: {name}, {mode}");
            checks += 1;
        }
        for level in 1..=7 {
            let flags = if level % 2 == 1 { &["-C", "-S"][..] } else { &["-N"] };
            let flag = format!("-{level}");
            let archive = cmd!(sh, "{c} -qzc -T1 {flag} {flags...} {dict...}")
                .stdin(&plain)
                .output()
                .with_context(|| format!("C encode: {name}, level {level}"))?;
            let decoded = cmd!(sh, "{rust} decode {dict...}")
                .stdin(archive.stdout)
                .output()
                .with_context(|| format!("Rust decode: {name}, level {level}"))?;
            ensure!(decoded.stdout == plain, "Rust decode mismatch: {name}, level {level}");
            checks += 1;
        }
    }
    ensure!(checks > 0, "no conformance inputs found");
    println!("{checks} byte-for-byte cross-decoding checks passed");
    Ok(())
}
