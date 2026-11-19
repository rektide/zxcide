use anyhow::{Context, Result, ensure};
use clap::Parser;
use serde_json::Value;
use std::{
    env,
    ffi::OsString,
    num::NonZeroU32,
    path::{Path, PathBuf},
};
use xshell::{Shell, cmd};

#[derive(Parser)]
#[command(name = "cargo xtask", about = "ZXC development tasks")]
enum Task {
    /// Build, validate, and bundle the release WebAssembly component.
    Component,
    /// Benchmark the release component. Pass Criterion arguments after `--`.
    Bench {
        #[arg(last = true)]
        args: Vec<OsString>,
    },
    /// Capture user cycles for one exact component benchmark with Linux perf.
    Perf {
        #[arg(default_value = "component/upstream-l7/decode")]
        case: String,
        #[arg(long, default_value = "5")]
        seconds: NonZeroU32,
    },
}

fn main() -> Result<()> {
    let task = Task::parse();
    let sh = Shell::new()?;
    if let Some(path) = env::var_os("ZXC_BENCH_INPUT") {
        sh.set_var("ZXC_BENCH_INPUT", std::path::absolute(path)?);
    }
    if let Some(path) = env::var_os("CARGO_TARGET_DIR") {
        sh.set_var("CARGO_TARGET_DIR", std::path::absolute(path)?);
    }
    sh.change_dir(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap());
    let metadata = cmd!(sh, "cargo metadata --locked --no-deps --format-version 1").read()?;
    let metadata: Value = serde_json::from_str(&metadata)?;
    let target = Path::new(
        metadata["target_directory"]
            .as_str()
            .context("missing Cargo target directory")?,
    );
    let component = build_component(&sh, target)?;
    sh.set_var("ZXC_COMPONENT", component);
    match task {
        Task::Component => Ok(()),
        Task::Bench { args } => Ok(cmd!(
            sh,
            "cargo bench --locked -p zxc-component --features host-bench --bench component -- {args...}"
        )
        .run()?),
        Task::Perf { case, seconds } => perf(&sh, target, &case, seconds),
    }
}

fn build_component(sh: &Shell, target: &Path) -> Result<PathBuf> {
    cmd!(
        sh,
        "cargo build --locked --release -p zxc-component --target wasm32-unknown-unknown"
    )
    .run()?;
    let core = target.join("wasm32-unknown-unknown/release/zxc_component.wasm");
    let out = sh.create_dir(target.join("component"))?;
    let component = out.join("zxc-codec.wasm");
    cmd!(sh, "wasm-tools component new {core} -o {component}").run()?;
    cmd!(sh, "wasm-tools validate {component}").run()?;
    sh.copy_file("crates/component/wit/codec.wit", out.join("codec.wit"))?;
    sh.copy_file("LICENSE", out.join("LICENSE"))?;
    Ok(component)
}

fn perf(sh: &Shell, target: &Path, case: &str, seconds: NonZeroU32) -> Result<()> {
    let build = cmd!(
        sh,
        "cargo bench --locked --no-run --message-format=json-render-diagnostics
        -p zxc-component --bench component --features host-perf"
    )
    .read()?;
    let mut bench = None;
    for line in build.lines() {
        let msg: Value = serde_json::from_str(line)?;
        if msg["reason"] == "compiler-artifact"
            && msg["target"]["kind"][0] == "bench"
            && msg["target"]["name"] == "component"
        {
            bench = msg["executable"].as_str().map(PathBuf::from);
        }
    }
    let bench = bench.context("Cargo did not report the component benchmark executable")?;
    let cases = cmd!(sh, "{bench} --bench --exact {case} --list").read()?;
    ensure!(
        cases.lines().any(|line| line.strip_suffix(": benchmark") == Some(case)),
        "no such case: {case} (check ZXC_BENCH_MODE and ZXC_BENCH_BLOCK_LOG)"
    );
    let out = tempfile::Builder::new()
        .prefix("component-perf-")
        .tempdir_in(target)?
        .keep();
    eprintln!("Capture: {}", out.display());
    let _dir = sh.push_dir(&out);
    let seconds = seconds.to_string();
    cmd!(
        sh,
        "perf record -k mono -F 997 -e cycles:u -- {bench} --bench --exact {case} --profile-time {seconds}"
    )
    .env("ZXC_JITDUMP", "1")
    .run()?;
    cmd!(sh, "perf inject --jit -i perf.data -o perf.jit.data").run()?;
    println!("Report: perf report -i {}", out.join("perf.jit.data").display());
    Ok(())
}
