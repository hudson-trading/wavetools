//------------------------------------------------------------------------------
// wavecat.rs
// CLI tool to read and display waveform files
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use clap::Parser;
use glob::Pattern;
use wavetools::{open_wave_file_with_format, write_names, write_signals_wave, NameOptions, SignalNames, SignalOutputOptions, WaveFormat};
use std::path::PathBuf;
use std::process;

#[derive(Parser, Debug)]
#[command(name = "wavecat")]
#[command(about = "Read and display waveform files (FST or VCD)", long_about = "\
Read and display waveform files (FST or VCD format).

Examples:
  wavecat sim.fst
  wavecat --names --sort sim.vcd
  wavecat --start 100 --end 500 sim.fst
  wavecat --filter '*.clk' --time-pound sim.fst
  wavecat --format vcd dump.dat")]
struct Args {
    /// Waveform file to read (FST or VCD)
    file: PathBuf,

    /// Starting time
    #[arg(short, long)]
    start: Option<u64>,

    /// Ending time
    #[arg(short, long)]
    end: Option<u64>,

    /// Print variable names only
    #[arg(short, long)]
    names: bool,

    /// Sort entries lexically
    #[arg(long)]
    sort: bool,

    /// Display time with #
    #[arg(long)]
    time_pound: bool,

    /// No space before range
    #[arg(long)]
    no_range_space: bool,

    /// Force file format instead of auto-detecting (fst or vcd)
    #[arg(long, value_parser = parse_format)]
    format: Option<WaveFormat>,

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

fn print_names(handle_to_names: &SignalNames, sort: bool) -> Result<(), String> {
    let mut stdout = std::io::stdout();
    write_names(&mut stdout, handle_to_names, sort)
        .map_err(|e| format!("Failed to write names: {}", e))
}

fn process_wave_file(args: &Args) -> Result<(), String> {
    let name_options = NameOptions {
        no_range_space: args.no_range_space,
    };

    let (mut reader, handle_to_names) =
        open_wave_file_with_format(&args.file, &name_options, args.format)?;

    let patterns: Vec<Pattern> = args.filter
        .iter()
        .flat_map(|s| s.split_whitespace())
        .map(|p| Pattern::new(p).map_err(|e| format!("Invalid glob pattern '{}': {}", p, e)))
        .collect::<Result<_, _>>()?;

    let handle_to_names = if !patterns.is_empty() {
        handle_to_names
            .into_iter()
            .filter_map(|(handle, names)| {
                let filtered: Vec<String> = names
                    .into_iter()
                    .filter(|name| patterns.iter().any(|p| p.matches(name)))
                    .collect();
                if filtered.is_empty() { None } else { Some((handle, filtered)) }
            })
            .collect()
    } else {
        handle_to_names
    };

    if args.names {
        print_names(&handle_to_names, args.sort)?;
    } else {
        let output_options = SignalOutputOptions {
            time_pound: args.time_pound,
            sort: args.sort,
        };

        let mut stdout = std::io::stdout();
        write_signals_wave(
            &mut stdout,
            &mut reader,
            &handle_to_names,
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
