use anyhow::{Context, Result, ensure};
use clap::{Parser, ValueEnum};
use serde_json::Value;
use std::{
    env,
    ffi::OsString,
    num::NonZeroU32,
    path::{Path, PathBuf},
};
use xshell::{Shell, cmd};
mod interop;

#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
enum Backend {
    Native,
    Component,
}

#[derive(Clone, Copy)]
enum BenchMode {
    Timing,
    Perf { dwarf: bool },
}

impl Backend {
    fn bench_name(self) -> &'static str {
        match self {
            Self::Native => "codec",
            Self::Component => "component",
        }
    }
}

#[derive(Parser)]
#[command(name = "cargo xtask", about = "ZXC development tasks")]
enum Task {
    /// Cross-decode every fixture with the pinned C CLI and the native Rust CLI.
    Interop {
        /// Existing C executable. By default fetch and build the pinned reference.
        c_cli: Option<PathBuf>,
        /// Existing Rust executable. By default build zxc-cli in release mode.
        rust_cli: Option<PathBuf>,
    },
    /// Build, validate, and bundle the release WebAssembly component.
    Component,
    /// Run Criterion for one backend. Pass Criterion arguments after `--`.
    Bench {
        backend: Backend,
        #[arg(last = true)]
        args: Vec<OsString>,
    },
    /// Time one exact case: native reusable, native one-shot, then component.
    Compare {
        #[arg(default_value = "upstream-l7/decode")]
        case: String,
        #[arg(last = true)]
        args: Vec<OsString>,
    },
    /// Capture one exact Criterion case with Linux perf.
    Perf {
        backend: Backend,
        #[arg(default_value = "upstream-l7/decode")]
        case: String,
        #[arg(long, default_value = "5")]
        seconds: NonZeroU32,
        /// Retain optimized debug info and collect DWARF call stacks.
        #[arg(long)]
        dwarf: bool,
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
    match task {
        Task::Interop { c_cli, rust_cli } => {
            let c = match c_cli {
                Some(path) => std::path::absolute(path)?,
                None => interop::oracle(&sh, target)?,
            };
            let rust = match rust_cli {
                Some(path) => std::path::absolute(path)?,
                None => {
                    cmd!(sh, "cargo build --locked --release -p zxc-cli").run()?;
                    target.join("release").join(format!("zxc{}", env::consts::EXE_SUFFIX))
                }
            };
            interop::check(&sh, &c, &rust)
        }
        Task::Component => build_component(&sh, target, false).map(drop),
        Task::Bench { backend, args } => {
            let bench = build_bench(&sh, target, backend, BenchMode::Timing)?;
            cmd!(sh, "{bench} --bench {args...}").run()?;
            Ok(())
        }
        Task::Compare { case, args } => {
            let native = build_bench(&sh, target, Backend::Native, BenchMode::Timing)?;
            let component = build_bench(&sh, target, Backend::Component, BenchMode::Timing)?;
            for (bench, id) in [
                (&native, case.clone()),
                (&native, format!("oneshot/{case}")),
                (&component, format!("component/{case}")),
            ] {
                check_case(&sh, bench, &id)?;
                cmd!(sh, "{bench} --bench --exact {id}").args(&args).run()?;
            }
            Ok(())
        }
        Task::Perf {
            backend,
            case,
            seconds,
            dwarf,
        } => perf(&sh, target, backend, &case, seconds, dwarf),
    }
}

fn build_component(sh: &Shell, target: &Path, dwarf: bool) -> Result<PathBuf> {
    let profile = if dwarf { "profiling" } else { "release" };
    cmd!(
        sh,
        "cargo build --locked --profile {profile} -p zxc-component --target wasm32-unknown-unknown"
    )
    .run()?;
    let core = target
        .join("wasm32-unknown-unknown")
        .join(profile)
        .join("zxc_component.wasm");
    let out = sh.create_dir(target.join(if dwarf { "profiling/component" } else { "component" }))?;
    let component = out.join("zxc-codec.wasm");
    cmd!(sh, "wasm-tools component new {core} -o {component}").run()?;
    cmd!(sh, "wasm-tools validate {component}").run()?;
    sh.copy_file("crates/component/wit/codec.wit", out.join("codec.wit"))?;
    sh.copy_file("LICENSE", out.join("LICENSE"))?;
    Ok(component)
}

fn build_bench(sh: &Shell, target: &Path, backend: Backend, mode: BenchMode) -> Result<PathBuf> {
    let mut build = cmd!(
        sh,
        "cargo bench --locked --no-run --message-format=json-render-diagnostics"
    );
    let dwarf = matches!(mode, BenchMode::Perf { dwarf: true });
    if dwarf {
        build = build.args(["--profile", "profiling"]);
    }
    build = match backend {
        Backend::Native => build.args(["-p", "zxc-codec"]),
        Backend::Component => {
            sh.set_var("ZXC_COMPONENT", build_component(sh, target, dwarf)?);
            build.args([
                "-p",
                "zxc-component",
                "--features",
                match mode {
                    BenchMode::Perf { .. } => "host-perf",
                    BenchMode::Timing => "host-bench",
                },
            ])
        }
    };
    let build = build.args(["--bench", backend.bench_name()]).read()?;
    let mut bench = None;
    for line in build.lines() {
        let msg: Value = serde_json::from_str(line)?;
        if msg["reason"] == "compiler-artifact"
            && msg["target"]["kind"][0] == "bench"
            && msg["target"]["name"] == backend.bench_name()
        {
            bench = msg["executable"].as_str().map(PathBuf::from);
        }
    }
    bench.context("Cargo did not report the benchmark executable")
}

fn check_case(sh: &Shell, bench: &Path, case: &str) -> Result<()> {
    let cases = cmd!(sh, "{bench} --bench --exact {case} --list").read()?;
    ensure!(
        cases.lines().any(|line| line.strip_suffix(": benchmark") == Some(case)),
        "no such case: {case} (check ZXC_BENCH_MODE and ZXC_BENCH_BLOCK_LOG)"
    );
    Ok(())
}

fn perf(sh: &Shell, target: &Path, backend: Backend, case: &str, seconds: NonZeroU32, dwarf: bool) -> Result<()> {
    let bench = build_bench(sh, target, backend, BenchMode::Perf { dwarf })?;
    let id = match backend {
        Backend::Native => case.to_owned(),
        Backend::Component => format!("component/{case}"),
    };
    check_case(sh, &bench, &id)?;
    let out = tempfile::Builder::new()
        .prefix(&format!("{}-perf-", backend.bench_name()))
        .tempdir_in(target)?
        .keep();
    eprintln!("Capture: {}", out.display());
    let _dir = sh.push_dir(&out);
    let seconds = seconds.to_string();
    let stacks = if dwarf { &["--call-graph", "dwarf"][..] } else { &[] };
    let record = cmd!(
        sh,
        "perf record -k mono -F 997 -e cycles:u {stacks...} -- {bench} --bench --exact {id} --profile-time {seconds}"
    );
    let report = match backend {
        Backend::Native => {
            record.run()?;
            "perf.data"
        }
        Backend::Component => {
            record.env("ZXC_JITDUMP", if dwarf { "dwarf" } else { "1" }).run()?;
            cmd!(sh, "perf inject --jit -i perf.data -o perf.jit.data").run()?;
            "perf.jit.data"
        }
    };
    println!("Report: perf report -i {}", out.join(report).display());
    Ok(())
}
