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
#[command(version, about = "Stream ZXC v9 archives and extract indexed byte ranges.")]
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
#[derive(Clone, Copy, ValueEnum)]
enum Transport {
    Crossbeam,
    #[cfg(feature = "flume")]
    Flume,
    #[cfg(feature = "kanal")]
    Kanal,
    #[cfg(feature = "crossfire")]
    Crossfire,
    #[cfg(feature = "rayon")]
    Rayon,
}
#[derive(Args)]
struct Execution {
    /// Worker count. One runs directly on the caller's thread.
    #[arg(long, default_value = "1")]
    threads: std::num::NonZeroUsize,
    /// Maximum admitted but unwritten blocks (default: twice the workers).
    #[arg(long)]
    slots: Option<std::num::NonZeroUsize>,
    /// Parallel scheduler to use when threads > 1.
    #[arg(long, value_enum, default_value = "crossbeam")]
    backend: Transport,
}
impl Execution {
    fn config(&self) -> zxc_parallel::Config {
        use zxc_parallel::Backend;
        zxc_parallel::Config {
            workers: self.threads.get(),
            slots: self
                .slots
                .map_or_else(|| self.threads.get().saturating_mul(2).min(256), |s| s.get()),
            backend: match self.backend {
                Transport::Crossbeam => Backend::Crossbeam,
                #[cfg(feature = "flume")]
                Transport::Flume => Backend::Flume,
                #[cfg(feature = "kanal")]
                Transport::Kanal => Backend::Kanal,
                #[cfg(feature = "crossfire")]
                Transport::Crossfire => Backend::Crossfire,
                #[cfg(feature = "rayon")]
                Transport::Rayon => Backend::Rayon,
            },
        }
    }
}
#[derive(Subcommand)]
enum Command {
    /// Compress into one frame.
    Encode {
        #[command(flatten)]
        execution: Execution,
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
    /// Copy uncompressed bytes from a seek-indexed file.
    Range {
        /// Archive path (each frame must have a seek table).
        archive: PathBuf,
        /// First uncompressed byte (zero-based, across all frames).
        start: u64,
        /// Number of bytes to copy.
        length: u64,
        /// Write to a new file (- for stdout).
        #[arg(short, long, default_value = "-")]
        output: PathBuf,
        /// Available .zxd dictionary (repeat for concatenated frames).
        #[arg(short = 'D', long)]
        dictionary: Vec<PathBuf>,
    },
    /// Show indexed frame metadata without reading block payloads.
    Info { archive: PathBuf },
}
#[derive(Args)]
struct DecodeArgs {
    #[command(flatten)]
    execution: Execution,
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
fn dictionaries(paths: &[PathBuf]) -> io::Result<Vec<Dictionary>> {
    paths.iter().map(|p| dict(p)).collect()
}
fn indexed(path: &Path) -> io::Result<zxc_seek::Archive<zxc_seek::FileSource>> {
    if path == Path::new("-") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "indexed reads require a file path",
        ));
    }
    zxc_seek::Archive::open(zxc_seek::FileSource::open(path)?)
}
fn info(path: &Path, mut w: impl Write) -> io::Result<()> {
    let archive = indexed(path)?;
    writeln!(
        w,
        "ZXC v9: {} indexed frame(s), {} uncompressed bytes",
        archive.frames().len(),
        archive.len()
    )?;
    writeln!(
        w,
        "frame\tplain_start\tplain_bytes\tarchive_bytes\tblock_bytes\tblocks\tchecksum\tdict_id"
    )?;
    let mut start = 0;
    for (index, frame) in archive.frames().iter().enumerate() {
        let id = frame
            .dictionary_id
            .map_or_else(|| "-".to_owned(), |id| format!("{id:08x}"));
        writeln!(
            w,
            "{index}\t{start}\t{}\t{}\t{}\t{}\t{}\t{id}",
            frame.plain_size, frame.compressed_size, frame.block_size, frame.blocks, frame.checksum
        )?;
        start += frame.plain_size;
    }
    w.flush()
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
    let dicts = dictionaries(&opt.dictionary)?;
    zxc_parallel::decode(
        input(path)?,
        w,
        DecodeOptions {
            dictionaries: &dicts,
            max_output: opt.max_output,
        },
        opt.execution.config(),
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
            execution,
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
            output(&files.output, |w| {
                zxc_parallel::encode(input(&files.input)?, w, opts, execution.config())
            })
        }
        Command::Decode { files, options } => output(&files.output, |w| decode(&files.input, w, options)),
        Command::Test { input, options } => decode(&input, &mut io::sink(), options),
        Command::Info { archive } => info(&archive, BufWriter::new(io::stdout().lock())),
        Command::Range {
            archive,
            start,
            length,
            output: destination,
            dictionary,
        } => {
            let end = start
                .checked_add(length)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "start + length overflows"))?;
            let archive = indexed(&archive)?;
            let dicts = dictionaries(&dictionary)?;
            output(&destination, |w| archive.copy_range(start..end, &dicts, w))
        }
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
