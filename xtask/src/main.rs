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
mod ledger;
mod smoke;
mod scorecard;

#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
enum Backend {
    Native,
    Component,
}

#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
enum Guest {
    Scalar,
    Simd128,
    RelaxedSimd,
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
    /// Compare the current Rust CLI to pinned C on verified Silesia inputs.
    ComparisonReport {
        /// Measure only dickens and sao (otherwise all twelve files).
        #[arg(long)]
        quick: bool,
        /// Comma-separated upstream compression levels (distinct, 1..7).
        #[arg(long, default_value = "3,7")]
        levels: String,
        /// Also measure the saved Rust executable from this completed run directory.
        #[arg(long, value_name = "RUN_DIR")]
        before: Option<PathBuf>,
    },
    /// Run the pinned Silesia CLI scorecard (pass options after --).
    Scorecard {
        #[arg(last = true)]
        args: Vec<OsString>,
    },
    /// Rank native byte-movement sites and time the same cases without probes.
    Ledger {
        /// Also time the wide-Huffman WIT boundary and native one-shot control.
        #[arg(long)]
        component: bool,
    },
    /// Seed each libFuzzer target from the pinned corpus and run a bounded smoke test.
    FuzzSmoke {
        #[arg(long, default_value = "2000")]
        runs: NonZeroU32,
    },
    /// Capture phase timings for repeated upstream wide-Huffman frames.
    Tracy {
        #[arg(long, default_value = "tracy-capture")]
        capture: PathBuf,
        #[arg(long, default_value = "1000")]
        repeats: NonZeroU32,
        #[arg(long,default_value="18086",value_parser=clap::value_parser!(u16).range(1..))]
        port: u16,
    },
    /// Cross-decode frozen fixtures and generated cases with the C and Rust CLIs.
    Interop {
        /// Existing C executable. By default fetch and build the pinned reference.
        c_cli: Option<PathBuf>,
        /// Existing Rust executable. By default build zxc-cli in release mode.
        rust_cli: Option<PathBuf>,
        /// Run the generated gate using 3 Rust workers and 4 in-flight slots.
        #[arg(long, value_parser=["crossbeam","flume","kanal","crossfire","rayon"])]
        parallel_backend: Option<String>,
        /// Seed for generated cases (decimal u64).
        #[arg(long, default_value_t = interop::generated::DEFAULT_SEED)]
        seed: u64,
        /// Number of generated cases after the fixtures (0 runs only the fixtures).
        #[arg(long, default_value_t = interop::generated::DEFAULT_CASES)]
        cases: u64,
        /// Replay one zero-based generated case, skipping the frozen matrix.
        #[arg(long, conflicts_with = "cases")]
        case: Option<u64>,
    },
    /// Build, validate, and bundle the release WebAssembly component.
    Component {
        #[arg(long, value_enum, conflicts_with = "all")]
        variant: Option<Guest>,
        /// Cut all ISA variants into the same bundle.
        #[arg(long)]
        all: bool,
    },
    /// Run Criterion for one backend. Pass Criterion arguments after `--`.
    Bench {
        backend: Backend,
        /// ISA of the guest under test (component backend only).
        #[arg(long, value_enum)]
        variant: Option<Guest>,
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
        Task::ComparisonReport { quick, before, levels } => {
            let mut run = cmd!(sh, "python3 -B xtask/comparison_report.py --target {target} --levels {levels}");
            if quick {
                run = run.arg("--quick");
            }
            if let Some(before) = before {
                run = run.arg("--before").arg(std::path::absolute(before)?);
            }
            run.quiet().run()?;
            Ok(())
        }
        Task::Scorecard { args } => scorecard::run(&args),
        Task::Ledger { component } => ledger::run(&sh, target, component),
        Task::FuzzSmoke { runs } => smoke::fuzz(&sh, target, runs),
        Task::Tracy { capture, repeats, port } => smoke::tracy(&sh, target, &capture, repeats, port),
        Task::Interop {
            c_cli,
            rust_cli,
            parallel_backend,
            seed,
            cases,
            case,
        } => {
            let c = match c_cli {
                Some(path) => std::path::absolute(path)?,
                None => interop::oracle(&sh, target)?,
            };
            let rust = match rust_cli {
                Some(path) => std::path::absolute(path)?,
                None => {
                    let features = if parallel_backend.is_some() {
                        &["--features", "transports"][..]
                    } else {
                        &[]
                    };
                    cmd!(sh, "cargo build --locked --release -p zxc-cli {features...}").run()?;
                    target.join("release").join(format!("zxc{}", env::consts::EXE_SUFFIX))
                }
            };
            let indices = if let Some(index) = case {
                index..=index
            } else {
                if parallel_backend.is_none() {
                    interop::check(&sh, &c, &rust)?;
                }
                if cases == 0 {
                    println!("Generated: skipped (--cases 0)");
                    return Ok(());
                }
                0..=cases - 1
            };
            interop::generated::check(&sh, target, &c, &rust, seed, indices, parallel_backend.as_deref())
        }
        Task::Component { variant, all } => {
            let one = [variant.unwrap_or(Guest::Scalar)];
            for &v in if all {
                &[Guest::Scalar, Guest::Simd128, Guest::RelaxedSimd][..]
            } else {
                &one
            } {
                println!("{}", build_component(&sh, target, false, v)?.display());
            }
            Ok(())
        }
        Task::Bench { backend, variant, args } => {
            ensure!(
                backend == Backend::Component || variant.is_none(),
                "--variant selects a Wasm component"
            );
            let bench = build_bench_variant(
                &sh,
                target,
                backend,
                BenchMode::Timing,
                variant.unwrap_or(Guest::Scalar),
            )?;
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

fn build_component(sh: &Shell, target: &Path, dwarf: bool, guest: Guest) -> Result<PathBuf> {
    let profile = if dwarf { "profiling" } else { "release" };
    let (filename, features, validation) = match guest {
        Guest::Scalar => ("zxc-codec.wasm", "-simd128,-relaxed-simd", "-simd,-relaxed-simd"),
        Guest::Simd128 => ("zxc-codec-simd128.wasm", "+simd128,-relaxed-simd", "simd,-relaxed-simd"),
        Guest::RelaxedSimd => (
            "zxc-codec-relaxed-simd.wasm",
            "+simd128,+relaxed-simd",
            "simd,relaxed-simd",
        ),
    };
    let extra = if guest == Guest::Scalar {
        &[][..]
    } else {
        &["--features", "zxc-codec/merge-simd"][..]
    };
    cmd!(
        sh,
        "cargo build --locked --profile {profile} -p zxc-component --target wasm32-unknown-unknown {extra...}"
    )
    .env("CARGO_ENCODED_RUSTFLAGS", format!("-Ctarget-feature={features}"))
    .run()?;
    let core = target
        .join("wasm32-unknown-unknown")
        .join(profile)
        .join("zxc_component.wasm");
    let out = sh.create_dir(target.join(if dwarf { "profiling/component" } else { "component" }))?;
    let component = out.join(filename);
    cmd!(sh, "wasm-tools component new {core} -o {component}").run()?;
    cmd!(sh, "wasm-tools validate --features={validation} {component}").run()?;
    if guest != Guest::Scalar {
        let op = if guest == Guest::Simd128 {
            "i8x16.swizzle"
        } else {
            "i8x16.relaxed_swizzle"
        };
        ensure!(
            cmd!(sh, "wasm-tools print {component}")
                .read()?
                .split_whitespace()
                .any(|word| word == op),
            "expected {op} in {filename}"
        );
    }
    sh.copy_file("crates/component/wit/codec.wit", out.join("codec.wit"))?;
    sh.copy_file("LICENSE", out.join("LICENSE"))?;
    Ok(component)
}

fn build_bench(sh: &Shell, target: &Path, backend: Backend, mode: BenchMode) -> Result<PathBuf> {
    build_bench_variant(sh, target, backend, mode, Guest::Scalar)
}

fn build_bench_variant(sh: &Shell, target: &Path, backend: Backend, mode: BenchMode, guest: Guest) -> Result<PathBuf> {
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
            sh.set_var("ZXC_COMPONENT", build_component(sh, target, dwarf, guest)?);
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
