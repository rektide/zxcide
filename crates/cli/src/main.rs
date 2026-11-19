use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};
use tempfile::NamedTempFile;
use zxc_codec::{DecodeOptions, Dictionary, EncodeOptions, Mode};

#[derive(Parser)]
#[command(version, about = "Stream ZXC v9 archives. Input and output default to standard I/O.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Wait up to 30 seconds for a Tracy capture before starting work.
    #[cfg(feature = "tracy")]
    #[arg(long, global = true)]
    wait_for_tracy: bool,
}
#[derive(Args)]
struct Files {
    /// Source file (- for stdin).
    #[arg(default_value = "-")]
    input: PathBuf,
    /// Write to a new file (- for stdout).
    #[arg(short, long, default_value = "-")]
    output: PathBuf,
}
#[derive(Clone, Copy, ValueEnum)]
enum Layout {
    Split,
    Packed,
    Dense,
}
#[derive(Subcommand)]
enum Command {
    /// Compress into one frame.
    Encode {
        #[command(flatten)]
        files: Files,
        #[arg(long, value_enum, default_value = "split")]
        mode: Layout,
        /// Block size exponent (default 19 = 512 KiB).
        #[arg(long,default_value_t=19,value_parser=clap::value_parser!(u8).range(12..=21))]
        block_log: u8,
        #[arg(long)]
        no_checksum: bool,
        /// Append an upstream-compatible random-access table.
        #[arg(long)]
        seekable: bool,
        /// Use a pre-trained .zxd dictionary.
        #[arg(short = 'D', long)]
        dictionary: Option<PathBuf>,
    },
    /// Decompress and verify all frames.
    Decode {
        #[command(flatten)]
        files: Files,
        #[command(flatten)]
        options: DecodeArgs,
    },
    /// Verify all frames without writing plaintext.
    Test {
        #[arg(default_value = "-")]
        input: PathBuf,
        #[command(flatten)]
        options: DecodeArgs,
    },
}
#[derive(Args)]
struct DecodeArgs {
    /// Available .zxd dictionary (repeat for concatenated frames).
    #[arg(short = 'D', long)]
    dictionary: Vec<PathBuf>,
    /// Maximum total bytes to decompress.
    #[arg(long)]
    max_output: Option<u64>,
}
fn input(path: &Path) -> io::Result<Box<dyn Read>> {
    if path == Path::new("-") {
        Ok(Box::new(io::stdin().lock()))
    } else {
        Ok(Box::new(BufReader::new(File::open(path)?)))
    }
}
fn dict(path: &Path) -> io::Result<Dictionary> {
    let mut bytes = Vec::new();
    File::open(path)?.take(16 + 65535 + 128 + 1).read_to_end(&mut bytes)?;
    Dictionary::from_bytes(&bytes)
}
fn output(path: &Path, action: impl FnOnce(&mut dyn Write) -> io::Result<()>) -> io::Result<()> {
    if path == Path::new("-") {
        let mut w = BufWriter::new(io::stdout().lock());
        action(&mut w)?;
        w.flush()
    } else {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut tmp = NamedTempFile::new_in(parent)?;
        {
            let mut w = BufWriter::new(&mut tmp);
            action(&mut w)?;
            w.flush()?;
        }
        tmp.persist_noclobber(path)?;
        Ok(())
    }
}
fn decode(path: &Path, w: &mut dyn Write, opt: DecodeArgs) -> io::Result<()> {
    let dicts = opt.dictionary.iter().map(|p| dict(p)).collect::<io::Result<Vec<_>>>()?;
    zxc_codec::decode(
        input(path)?,
        w,
        DecodeOptions {
            dictionaries: &dicts,
            max_output: opt.max_output,
        },
    )
}
fn run(cli: Cli) -> io::Result<()> {
    #[cfg(feature = "tracy")]
    let _client = {
        use std::time::{Duration, Instant};
        let client = tracy_client::Client::start();
        if cli.wait_for_tracy {
            let until = Instant::now() + Duration::from_secs(30);
            while !tracy_client::Client::is_connected() {
                if Instant::now() >= until {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "no Tracy capture connected"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        client
    };
    match cli.command {
        Command::Encode {
            files,
            mode,
            block_log,
            no_checksum,
            seekable,
            dictionary,
        } => {
            let dictionary = dictionary.as_deref().map(dict).transpose()?;
            let opts = EncodeOptions {
                block_log,
                mode: match mode {
                    Layout::Split => Mode::Split,
                    Layout::Packed => Mode::Packed,
                    Layout::Dense => Mode::Dense,
                },
                checksum: !no_checksum,
                seekable,
                dictionary: dictionary.as_ref(),
            };
            output(&files.output, |w| zxc_codec::encode(input(&files.input)?, w, opts))
        }
        Command::Decode { files, options } => output(&files.output, |w| decode(&files.input, w, options)),
        Command::Test { input, options } => decode(&input, &mut io::sink(), options),
    }
}
fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zxc: {e}");
            ExitCode::FAILURE
        }
    }
}
