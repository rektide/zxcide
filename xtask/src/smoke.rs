use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::{BufWriter, Write},
    num::NonZeroU32,
    path::Path,
    process::{Child, Command},
    time::Duration,
};
use wait_timeout::ChildExt;
use xshell::{Shell, cmd};

const CORPUS: &str = "tests/conformance/v9";

pub fn fuzz(sh: &Shell, target: &Path, runs: NonZeroU32) -> Result<()> {
    cmd!(sh, "cargo +nightly fuzz --version")
        .run()
        .context("install nightly and cargo-fuzz (`cargo install --locked cargo-fuzz`)")?;
    let corpus = Path::new(CORPUS);
    for name in ["decode", "indexed", "roundtrip"] {
        // Use a fresh writable corpus. Mutations must never modify the fixtures
        // or make the next smoke run depend on a developer's last fuzz session.
        let seeds = tempfile::tempdir_in(target)?;
        for group in ["valid", "invalid"] {
            for entry in fs::read_dir(corpus.join(group))? {
                let entry = entry?;
                let p = entry.path();
                let filename = entry.file_name();
                let ext = p.extension();
                if name == "roundtrip" && group == "valid" && ext.is_some_and(|e| e == "expected") {
                    // First byte selects options; rotate modes across the seeded inputs.
                    let mut seed = vec![(filename.len() % 12) as u8];
                    seed.extend(fs::read(p)?);
                    fs::write(seeds.path().join(filename), seed)?;
                } else if name != "roundtrip" && ext.is_some_and(|e| e == "zxc") {
                    fs::copy(p, seeds.path().join(format!("{group}-{}", filename.to_string_lossy())))?;
                }
            }
        }
        let count = format!("-runs={runs}");
        let seeds = seeds.path();
        cmd!(sh, "cargo +nightly fuzz run {name} {seeds} -- {count} -seed=42").run()?;
    }
    Ok(())
}

// On errors (including a missing capture executable) don't leave a profiler
// or client running in the background. Reap the child after terminating it.
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
impl Process {
    fn finish(&mut self, name: &str) -> Result<()> {
        let status = self
            .0
            .wait_timeout(Duration::from_secs(90))?
            .with_context(|| format!("{name} timed out"))?;
        ensure!(status.success(), "{name} exited with {status}");
        Ok(())
    }
}

pub fn tracy(sh: &Shell, target: &Path, capture: &Path, repeats: NonZeroU32, port: u16) -> Result<()> {
    cmd!(
        sh,
        "cargo build --locked --profile profiling -p zxcide-cli --features tracy"
    )
    .run()?;
    let dir = tempfile::Builder::new().prefix("tracy-").tempdir_in(target)?.keep();
    let input = dir.join("repeated.zxc");
    let trace = dir.join("decode.tracy");
    let frame = sh.read_binary_file(Path::new(CORPUS).join("valid/spl_pivco_wide_l7.zxc"))?;
    let mut file = BufWriter::new(fs::File::create(&input)?);
    for _ in 0..repeats.get() {
        file.write_all(&frame)?;
    }
    file.flush()?;
    drop(file);
    eprintln!("Capture directory: {}", dir.display());
    let port = port.to_string();
    let binary = target
        .join("profiling")
        .join(format!("zxcide{}", std::env::consts::EXE_SUFFIX));
    let mut client = Process(
        Command::new(binary)
            .env("TRACY_PORT", &port)
            .args(["--wait-for-tracy", "test"])
            .arg(&input)
            .spawn()?,
    );
    let mut server = Process(
        Command::new(capture)
            .args(["-a", "127.0.0.1", "-p", &port, "-o"])
            .arg(&trace)
            .spawn()
            .context("start Tracy 0.14.1 capture tool (use --capture PATH)")?,
    );
    server.finish("tracy-capture (use Tracy 0.14.1)")?;
    client.finish("zxcide test")?;
    ensure!(fs::metadata(&trace)?.len() > 0, "empty capture");
    println!("Trace: {}", trace.display());
    Ok(())
}
