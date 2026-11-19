use anyhow::{Context, Result, bail, ensure};
use std::{
    fs::{self, File, OpenOptions},
    ops::RangeInclusive,
    path::Path,
    process::{Command, Stdio},
};
use xshell::Shell;

// Bump when the mapping from (seed, index) to recipe, plaintext, or ranges changes.
const GENERATOR_REV: u64 = 1;
const SHAPES: u64 = 12;
pub const DEFAULT_SEED: u64 = 0x5a58_432d_7639;
pub const DEFAULT_CASES: u64 = SHAPES;
const BLOCK: usize = 1 << 12;
const LARGE_BLOCK: usize = 1 << 17;
const GROUP_EDGE: usize = 64 * BLOCK;
const HTTP_DICT: &str = "tests/conformance/v9/valid/dict_http.zxd";

#[cfg(test)]
mod tests;

// SplitMix64: wrapping u64 addition/multiplication and logical right shifts.
// Random byte runs take little-endian words; bounded choices reduce u64 modulo n.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn noise(&mut self, len: usize) -> Vec<u8> {
        let mut bytes = vec![0; len];
        for chunk in bytes.chunks_mut(8) {
            chunk.copy_from_slice(&self.next().to_le_bytes()[..chunk.len()]);
        }
        bytes
    }

    fn periodic(&mut self, len: usize) -> Vec<u8> {
        let period = 3 + self.below(41);
        let motif = self.noise(period);
        motif.into_iter().cycle().take(len).collect()
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Frame {
    len: usize,
    mode: &'static str,
    level: u8,
    block_log: u8,
    checksum: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct Case {
    family: &'static str,
    frames: Vec<Frame>,
    indexed: bool,
    dictionary: Option<&'static str>,
    plain: Vec<u8>,
    ranges: Vec<(usize, usize)>,
}

fn generate(seed: u64, index: u64) -> Case {
    // Mix the index separately so a case never consumes another case's state.
    let index_key = Rng(index ^ GENERATOR_REV).next();
    let mut rng = Rng(seed ^ index_key);
    let slot = index % SHAPES;
    let cycle = index / SHAPES;
    let mut boundaries = Vec::new();
    let mut dictionary = None;
    let (family, indexed, inputs) = match slot {
        0 => ("empty", true, vec![(12, true, vec![])]),
        1 => ("one byte", false, vec![(12, false, rng.noise(1))]),
        // The plaintext size field in the footer widens from one byte to two.
        2..=4 => (
            "footer width",
            slot != 4,
            vec![(12, slot == 3, rng.noise(253 + slot as usize))],
        ),
        5..=7 => (
            "4 KiB edge",
            slot == 7,
            vec![(12, slot != 6, rng.periodic(BLOCK - 6 + slot as usize))],
        ),
        8 => {
            let mut mixed = rng.noise(LARGE_BLOCK);
            let first = 1024;
            let second = first + 65_535;
            mixed.copy_within(first..first + 512, second);
            let records = (0..8)
                .map(|i| {
                    format!(
                        "stream=alpha event=heartbeat source={:016x} sequence={i:04} status=ok\n",
                        rng.next()
                    )
                })
                .collect::<String>();
            mixed.extend(records.bytes().cycle().take(LARGE_BLOCK));
            mixed.extend(rng.noise(LARGE_BLOCK));
            mixed.extend(records.bytes().cycle().take(LARGE_BLOCK / 2));
            boundaries.extend([second, 2 * LARGE_BLOCK, 3 * LARGE_BLOCK]);
            ("mixed blocks and distant match", true, vec![(17, false, mixed)])
        }
        9 => {
            let mut input = Vec::new();
            for block in 0u16..66 {
                let mut plain = rng.periodic(BLOCK);
                plain[..2].copy_from_slice(&block.to_le_bytes());
                input.extend(plain);
            }
            boundaries.push(GROUP_EDGE); // between zero-based blocks 63 and 64
            ("seek groups", true, vec![(12, true, input)])
        }
        10 => (
            "concatenation",
            true,
            vec![
                (12, false, rng.noise(BLOCK + 37)),
                (13, true, vec![]),
                (14, true, rng.periodic((1 << 14) + 73)),
            ],
        ),
        11 => {
            dictionary = Some(HTTP_DICT);
            let mut records = Vec::new();
            for _ in 0..96 {
                let user = 10_000 + rng.below(90_000);
                let suffix = ["profile", "settings", "history"][rng.below(3)];
                records.extend_from_slice(format!("GET /api/v1/users/{user}/{suffix} HTTP/1.1 Host: api.example.com Accept: application/json Accept-Encoding: gzip, br User-Agent: zxc-client/1.0 Connection: keep-alive ").as_bytes());
            }
            ("HTTP dictionary", true, vec![(12, true, records)])
        }
        _ => unreachable!(),
    };

    let mut plain = Vec::new();
    let mut frames = Vec::new();
    for (frame, (block_log, checksum, bytes)) in inputs.into_iter().enumerate() {
        let variant = slot + frame as u64;
        let mode = ["split", "packed", "dense"][((variant + cycle % 3) % 3) as usize];
        let level = [1, 3, 5, 7][((variant + cycle % 4) % 4) as usize];
        let block_size = 1 << block_log;
        if !plain.is_empty() && !bytes.is_empty() {
            boundaries.push(plain.len());
        }
        if bytes.len() > block_size {
            boundaries.push(plain.len() + block_size);
        }
        frames.push(Frame {
            len: bytes.len(),
            mode,
            level,
            block_log,
            checksum,
        });
        plain.extend(bytes);
    }

    let mut ranges = Vec::new();
    if indexed {
        ranges.push((plain.len(), 0));
        if !plain.is_empty() {
            let start = rng.below(plain.len());
            ranges.push((start, 1 + rng.below((plain.len() - start).min(97))));
        }
        for edge in boundaries {
            let before = edge.min(17);
            let after = (plain.len() - edge).min(19);
            ranges.push((edge - before, before + after));
        }
        ranges.sort_unstable();
        ranges.dedup();
    }
    Case {
        family,
        frames,
        indexed,
        dictionary,
        plain,
        ranges,
    }
}

pub fn check(
    sh: &Shell,
    target: &Path,
    c: &Path,
    rust: &Path,
    seed: u64,
    indices: RangeInclusive<u64>,
    backend: Option<&str>,
) -> Result<()> {
    let (mut cases, mut range_checks) = (0u64, 0u128);
    for index in indices {
        let case = generate(seed, index);
        let selection = backend.map_or_else(String::new, |b| format!(" --parallel-backend {b}"));
        let replay = format!(
            "cargo xtask interop --seed {seed} --case {index}{selection} -- {} {}",
            replay_path(c),
            replay_path(rust),
        );
        let recipe = format!(
            "generator revision {GENERATOR_REV}, seed {seed}, case {index}: {}\nindexed: {}; dictionary: {:?}\nframes: {:#?}\nranges (start, length): {:?}\nC CLI: {c:?}\nRust CLI: {rust:?}\nReplay: {replay}",
            case.family, case.indexed, case.dictionary, case.frames, case.ranges,
        );
        let scratch = tempfile::Builder::new()
            .prefix("generated-interop-")
            .tempdir_in(target)
            .with_context(|| format!("Could not create artifacts directory in {}\n{recipe}", target.display()))?;
        let dir = scratch.path();
        let result = (|| {
            fs::write(dir.join("case.txt"), &recipe)?;
            fs::write(dir.join("expected.bin"), &case.plain)?;
            verify(sh, c, rust, dir, &case, backend)
        })();
        if let Err(error) = result {
            let dir = scratch.keep();
            if let Err(write_error) = fs::write(dir.join("failure.txt"), format!("{error:#}\n")) {
                eprintln!("Could not write failure.txt: {write_error}");
            }
            return Err(error.context(format!("{recipe}\nArtifacts: {}", dir.display())));
        }
        scratch.close()?;
        cases += 1;
        range_checks += 2 * case.ranges.len() as u128;
    }
    println!(
        "Generated: {cases} cases, {} byte-for-byte cross-decoding checks and {range_checks} indexed range checks passed (generator revision {GENERATOR_REV}, seed {seed})",
        2 * u128::from(cases),
    );
    Ok(())
}

fn replay_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    // Quote for a POSIX shell (PowerShell on Windows), even when the path has spaces.
    #[cfg(windows)]
    let path = path.replace('\'', "''");
    #[cfg(not(windows))]
    let path = path.replace('\'', "'\\''");
    format!("'{path}'")
}

fn verify(sh: &Shell, c: &Path, rust: &Path, dir: &Path, case: &Case, backend: Option<&str>) -> Result<()> {
    let rust_archive = dir.join("rust.zxc");
    let c_archive = dir.join("c.zxc");
    let dictionary = case.dictionary.into_iter().flat_map(|path| ["-D", path]);
    let execution = backend
        .into_iter()
        .flat_map(|name| ["--threads", "3", "--slots", "4", "--backend", name]);
    let mut start = 0;
    for (i, frame) in case.frames.iter().enumerate() {
        let input = dir.join("frame-input.bin");
        fs::write(&input, &case.plain[start..start + frame.len])?;
        start += frame.len;
        let mut r = Command::new(rust);
        r.args(["encode", "--mode", frame.mode, "--block-log"])
            .arg(frame.block_log.to_string())
            .args(dictionary.clone());
        if !frame.checksum {
            r.arg("--no-checksum");
        }
        if case.indexed {
            r.arg("--seekable");
        }
        if backend.is_some() {
            let output = OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("serial.zxc"))?;
            run(sh, &mut r, Some(&input), output).context("serial archive baseline")?;
            r.args(execution.clone());
        }
        let mut reference = Command::new(c);
        reference
            .args(["-qzc", "-T1", "-B"])
            .arg(format!("{}K", 1u32 << (frame.block_log - 10)))
            .arg(format!("-{}", frame.level))
            .arg(if frame.checksum { "-C" } else { "-N" })
            .args(dictionary.clone());
        if case.indexed {
            reference.arg("-S");
        }

        for (producer, command, archive) in [("Rust", &mut r, &rust_archive), ("C", &mut reference, &c_archive)] {
            (|| {
                let output = OpenOptions::new().create(true).append(true).open(archive)?;
                let prior = output.metadata()?.len();
                run(sh, command, Some(&input), output)?;
                ensure!(
                    fs::metadata(archive)?.len() > prior,
                    "encoder emitted an empty archive frame"
                );
                Ok(())
            })()
            .with_context(|| format!("{producer} encode, frame {i}"))?;
        }
        fs::remove_file(input)?;
    }

    if backend.is_some() {
        ensure!(
            fs::read(dir.join("serial.zxc"))? == fs::read(&rust_archive)?,
            "parallel Rust archive differs from serial Rust archive"
        );
    }
    let output = dir.join("actual.bin");
    let mut c_decode = Command::new(c);
    c_decode.args(["-qdc", "-T1"]).args(dictionary.clone());
    let mut rust_decode = Command::new(rust);
    rust_decode.arg("decode").args(dictionary.clone()).args(execution);
    for (direction, command, archive) in [
        ("Rust archive -> C full decode", &mut c_decode, &rust_archive),
        ("C archive -> Rust full decode", &mut rust_decode, &c_archive),
    ] {
        (|| {
            run(sh, command, Some(archive), File::create(&output)?)?;
            exact(&case.plain, &fs::read(&output)?, 0)
        })()
        .context(direction)?;
    }

    for (producer, archive) in [("Rust", rust_archive), ("C", c_archive)] {
        for &(start, len) in &case.ranges {
            (|| {
                let mut command = Command::new(rust);
                command
                    .arg("range")
                    .arg(&archive)
                    .arg(start.to_string())
                    .arg(len.to_string())
                    .args(dictionary.clone());
                run(sh, &mut command, None, File::create(&output)?)?;
                exact(&case.plain[start..start + len], &fs::read(&output)?, start)
            })()
            .with_context(|| format!("{producer} archive -> Rust range (start {start}, length {len})"))?;
        }
    }
    Ok(())
}

fn run(sh: &Shell, command: &mut Command, input: Option<&Path>, output: File) -> Result<()> {
    let input = match input {
        Some(path) => Stdio::from(File::open(path)?),
        None => Stdio::null(),
    };
    let status = command
        .current_dir(sh.current_dir())
        .stdin(input)
        .stdout(Stdio::from(output))
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("starting {command:?}"))?;
    ensure!(
        status.status.success(),
        "{command:?}: {}: {}",
        status.status,
        String::from_utf8_lossy(&status.stderr).trim(),
    );
    Ok(())
}

fn exact(expected: &[u8], actual: &[u8], base: usize) -> Result<()> {
    if let Some(i) = expected.iter().zip(actual).position(|(a, b)| a != b) {
        bail!(
            "first difference at plaintext offset {}: expected 0x{:02x}, got 0x{:02x} (expected length {}, got {})",
            base + i,
            expected[i],
            actual[i],
            expected.len(),
            actual.len(),
        );
    }
    ensure!(
        expected.len() == actual.len(),
        "length mismatch at plaintext offset {}: expected length {}, got {}",
        base + expected.len().min(actual.len()),
        expected.len(),
        actual.len(),
    );
    Ok(())
}
