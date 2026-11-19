use anyhow::{Context, Result, ensure};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};
use xshell::{Shell, cmd};

const REV: &str = "2992c28e6e1df07feec0cb39678a6d4a9665c6bb";
const REPO: &str = "https://github.com/hellobertrand/zxc";

/// Cache the C checkout outside the source tree. Reuse it only if it still
/// matches the verification target; never reset a developer's edited files.
pub fn oracle(sh: &Shell, target: &Path) -> Result<PathBuf> {
    let root = sh.create_dir(target.join("oracle").join(REV))?;
    let src = root.join("source");
    let build = root.join("build");
    if !src.exists() {
        let tmp = tempfile::tempdir_in(&root)?;
        let staging = tmp.path();
        cmd!(sh, "git init -q {staging}").run()?;
        cmd!(sh, "git -C {staging} fetch -q --depth=1 {REPO} {REV}").run()?;
        cmd!(sh, "git -C {staging} checkout -q --detach FETCH_HEAD").run()?;
        std::fs::rename(staging, &src)?;
    }
    ensure!(
        cmd!(sh, "git -C {src} rev-parse HEAD").read()? == REV
            && cmd!(sh, "git -C {src} status --porcelain --ignored").read()?.is_empty(),
        "unexpected oracle checkout at {}; move it aside or supply a C executable",
        src.display()
    );
    cmd!(
        sh,
        "cmake -S {src} -B {build} -DCMAKE_BUILD_TYPE=Release -DZXC_BUILD_TESTS=OFF"
    )
    .run()?;
    cmd!(sh, "cmake --build {build} --config Release --parallel 2").run()?;
    let exe = format!("zxc{}", std::env::consts::EXE_SUFFIX);
    let c = [build.join(&exe), build.join("Release").join(&exe)]
        .into_iter()
        .find(|path| path.is_file())
        .context("CMake did not produce the C CLI")?;
    eprintln!("Reference {REV}: {}", c.display());
    Ok(c)
}

pub fn check(sh: &Shell, c: &Path, rust: &Path) -> Result<()> {
    let valid = Path::new("tests/conformance/v9/valid");
    let mut checks = 0;
    let mut range_checks = 0;
    let scratch = tempfile::tempdir()?;
    let indexed = scratch.path().join("indexed.zxc");
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
            if level % 2 == 1 {
                sh.write_file(&indexed, &archive.stdout)?;
                let start = plain.len() / 2;
                let length = (plain.len() - start).min(2049);
                let (offset, count) = (start.to_string(), length.to_string());
                let range = cmd!(sh, "{rust} range {indexed} {offset} {count} {dict...}")
                    .output()
                    .with_context(|| format!("indexed Rust range: {name}, C level {level}"))?;
                ensure!(
                    range.stdout == plain[start..start + length],
                    "range mismatch: {name}, C level {level}"
                );
                range_checks += 1;
            }
            let decoded = cmd!(sh, "{rust} decode {dict...}")
                .stdin(archive.stdout)
                .output()
                .with_context(|| format!("Rust decode: {name}, level {level}"))?;
            ensure!(decoded.stdout == plain, "Rust decode mismatch: {name}, level {level}");
            checks += 1;
        }
    }
    ensure!(checks > 0, "no conformance inputs found");
    println!("{checks} byte-for-byte cross-decoding checks and {range_checks} indexed range checks passed");
    Ok(())
}
