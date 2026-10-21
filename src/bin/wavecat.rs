//------------------------------------------------------------------------------
// wavecat.rs
// CLI tool to read and display waveform files
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use clap::Parser;
use std::path::PathBuf;
use std::process;
use wavetools::{
    apply_filter, names_only, open_wave_files, parse_filter_patterns, write_attrs, write_names,
    write_signals_wave_multi, write_wave_file, NameOptions, SignalOutputOptions, WaveFormat,
};

const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (rev ",
    env!("WAVETOOLS_GIT_REV"),
    ")",
);

#[derive(Parser, Debug)]
#[command(name = "wavecat")]
#[command(version = VERSION)]
#[command(
    about = "Read, display, and export waveform files (FST or VCD)",
    long_about = "\
Read, display, and export waveform files (FST or VCD format).
Multiple files are overlayed (their signals are unioned).

Examples:
  wavecat sim.fst
  wavecat --names --sort sim.vcd
  wavecat --names --sort clk.vcd counters.vcd
  wavecat --start 100 --end 500 sim.fst
  wavecat --filter '*.clk' --time-pound sim.fst
  wavecat --format vcd dump.dat
  wavecat sim.fst -o sim.vcd
  wavecat sim.vcd -o dump.dat --out-format fst"
)]
struct Args {
    /// Waveform file(s) to read (FST or VCD); multiple files are overlayed
    #[arg(required = true)]
    file: Vec<PathBuf>,

    /// Starting time
    #[arg(short, long)]
    start: Option<u64>,

    /// Ending time
    #[arg(short, long)]
    end: Option<u64>,

    /// Print variable names only
    #[arg(short, long, conflicts_with = "attrs")]
    names: bool,

    /// Print variable attributes (type, size, direction)
    #[arg(short, long, conflicts_with = "names")]
    attrs: bool,

    /// Sort entries lexically
    #[arg(long)]
    sort: bool,

    /// Display time with #
    #[arg(long)]
    time_pound: bool,

    /// No space before range
    #[arg(long)]
    no_range_space: bool,

    /// Force input file format instead of auto-detecting (fst or vcd)
    #[arg(long, value_parser = parse_format)]
    format: Option<WaveFormat>,

    /// Write a waveform file (format inferred from .vcd or .fst)
    #[arg(short, long, conflicts_with_all = ["names", "attrs"])]
    out: Option<PathBuf>,

    /// Force output format (fst or vcd); requires --out
    #[arg(long, value_parser = parse_format, requires = "out")]
    out_format: Option<WaveFormat>,

    /// Filter signals by glob pattern(s); may be specified multiple times or as a
    /// space-separated list (e.g. --filter "*.foo *.bar" or --filter "*.foo" --filter "*.bar")
    #[arg(short, long, action = clap::ArgAction::Append)]
    filter: Vec<String>,
}

fn parse_format(s: &str) -> Result<WaveFormat, String> {
    match s.to_ascii_lowercase().as_str() {
        "fst" => Ok(WaveFormat::Fst),
        "vcd" => Ok(WaveFormat::Vcd),
        _ => Err(format!("unknown format '{}', expected 'fst' or 'vcd'", s)),
    }
}

fn main() {
    let args = Args::parse();

    if let Err(e) = process_wave_file(&args) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn process_wave_file(args: &Args) -> Result<(), String> {
    if let (Some(s), Some(e)) = (args.start, args.end) {
        if s > e {
            return Err(format!("--start ({}) must be <= --end ({})", s, e));
        }
    }

    let out_format = args
        .out
        .as_ref()
        .map(|path| {
            args.out_format.map(Ok).unwrap_or_else(|| {
                path.extension()
                    .and_then(|s| s.to_str())
                    .and_then(|s| parse_format(s).ok())
                    .ok_or_else(|| {
                        "Cannot infer output format; use .vcd/.fst or --out-format".to_string()
                    })
            })
        })
        .transpose()?;
    if let Some(out) = &args.out {
        // Reject aliases of an input before either writer can truncate it.
        if let Ok(target) = std::fs::canonicalize(out) {
            for input in &args.file {
                if std::fs::canonicalize(input).ok().as_ref() == Some(&target) {
                    return Err("Output file must differ from all input files".into());
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if let (Ok(a), Ok(b)) = (std::fs::metadata(input), std::fs::metadata(out)) {
                        if a.dev() == b.dev() && a.ino() == b.ino() {
                            return Err("Output file must differ from all input files".into());
                        }
                    }
                }
            }
        }
    }

    let name_options = NameOptions {
        no_range_space: args.no_range_space,
    };
    let patterns = parse_filter_patterns(&args.filter)?;

    let paths: Vec<&std::path::Path> = args.file.iter().map(std::path::PathBuf::as_path).collect();
    let (readers, mut hierarchy, offsets) = open_wave_files(&paths, &name_options, args.format)?;

    apply_filter(&mut hierarchy, &patterns);

    if let (Some(path), Some(format)) = (&args.out, out_format) {
        write_wave_file(
            path,
            format,
            readers,
            &offsets,
            &hierarchy,
            args.start.unwrap_or(0),
            args.end,
        )
        .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
    } else if args.attrs {
        let mut stdout = std::io::stdout();
        write_attrs(
            &mut stdout,
            &hierarchy.signal_map,
            &hierarchy.names,
            args.sort,
        )
        .map_err(|e| format!("Failed to write attrs: {}", e))?;
    } else if args.names {
        let names = names_only(&hierarchy.signal_map, &hierarchy.names);
        let mut stdout = std::io::stdout();
        write_names(&mut stdout, &names, args.sort)
            .map_err(|e| format!("Failed to write names: {}", e))?;
    } else {
        let names = names_only(&hierarchy.signal_map, &hierarchy.names);
        let output_options = SignalOutputOptions {
            time_pound: args.time_pound,
            sort: args.sort,
        };

        let mut stdout = std::io::stdout();
        write_signals_wave_multi(
            &mut stdout,
            readers,
            &offsets,
            &names,
            args.start.unwrap_or(0),
            args.end,
            &output_options,
        )
        .or_else(|e| {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                Ok(())
            } else {
                Err(format!("Failed to write signals: {}", e))
            }
        })?;
    }

    Ok(())
}
