use anyhow::{Result, ensure};
use std::{ffi::OsString, process::Command};

pub fn run(args: &[OsString]) -> Result<()> {
    let status = Command::new("python3")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/scorecard.py"))
        .args(args)
        .status()?;
    ensure!(status.success(), "scorecard failed: {status}");
    Ok(())
}
