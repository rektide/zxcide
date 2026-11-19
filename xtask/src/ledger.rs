use super::{Backend, BenchMode, build_bench};
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::{fmt::Write, path::Path};
use xshell::{Shell, cmd};

const TIMING: &[&str] = &[
    "--sample-size",
    "20",
    "--warm-up-time",
    "0.3",
    "--measurement-time",
    "0.6",
];

fn interval(sh: &Shell, home: &Path, id: &str) -> Result<(f64, f64)> {
    let estimates: Value = serde_json::from_str(&sh.read_file(home.join(id).join("new/estimates.json"))?)?;
    let estimator = if estimates["slope"].is_object() {
        "slope"
    } else {
        "mean"
    };
    let ci = &estimates[estimator]["confidence_interval"];
    Ok((
        ci["lower_bound"].as_f64().context("missing lower bound")? / 1000.,
        ci["upper_bound"].as_f64().context("missing upper bound")? / 1000.,
    ))
}

pub(super) fn run(sh: &Shell, target: &Path, component: bool) -> Result<()> {
    let out = tempfile::Builder::new().prefix("ledger-").tempdir_in(target)?.keep();
    let home = out.join("criterion");
    sh.set_var("CRITERION_HOME", &home);
    let counts = cmd!(
        sh,
        "cargo bench --locked -p zxc-parallel --features ledger --bench ledger"
    )
    .read()?;
    cmd!(sh, "cargo bench --locked -p zxc-parallel --bench ledger -- {TIMING...}").run()?;
    let mut report = String::from("case\tplaintext_B\tsite\tbytes\tB_per_B\tCI95_low_us\tCI95_high_us\n");
    for line in counts.lines().filter_map(|s| s.strip_prefix("site\t")) {
        let id = line.split('\t').next().unwrap();
        let (lo, hi) = interval(sh, &home, &format!("ledger/{}", id.replace('/', "_")))?;
        writeln!(report, "{line}\t{lo:.3}\t{hi:.3}")?;
    }
    ensure!(report.lines().count() > 1, "missing ledger probe output");
    if component {
        let n = sh
            .read_binary_file("tests/conformance/v9/valid/spl_pivco_wide_l7.expected")?
            .len();
        let compressed = sh
            .read_binary_file("tests/conformance/v9/valid/spl_pivco_wide_l7.zxc")?
            .len();
        for (backend, prefix) in [(Backend::Native, "oneshot"), (Backend::Component, "component")] {
            let bench = build_bench(sh, target, backend, BenchMode::Timing)?;
            let id = format!("{prefix}/upstream-l7/decode");
            cmd!(sh, "{bench} --bench --exact {id} {TIMING...}").run()?;
            let (lo, hi) = interval(sh, &home, &format!("{prefix}_upstream-l7/decode"))?;
            let sites = if backend == Backend::Native {
                vec![("output:destination", n)]
            } else {
                vec![("ABI:result list", n), ("ABI:argument list", compressed)]
            };
            for (site, b) in sites {
                writeln!(
                    report,
                    "{id}\t{n}\t{site}\t{b}\t{:.5}\t{lo:.3}\t{hi:.3}",
                    b as f64 / n as f64
                )?;
            }
        }
    }
    let path = out.join("report.tsv");
    sh.write_file(&path, report)?;
    println!("Ranked sites and uninstrumented intervals: {}", path.display());
    Ok(())
}
