//------------------------------------------------------------------------------
// wavecat.rs
// CLI tool to read and display waveform files
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use clap::Parser;
use glob::Pattern;
use wavetools::{
    names_only, open_wave_file_with_format, write_attrs, write_names, write_signals_wave,
    NameOptions, SignalMap, SignalOutputOptions, WaveFormat,
};
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

fn process_wave_file(args: &Args) -> Result<(), String> {
    let name_options = NameOptions {
        no_range_space: args.no_range_space,
    };

    let (mut reader, signal_map) =
        open_wave_file_with_format(&args.file, &name_options, args.format)?;

    let patterns: Vec<Pattern> = args.filter
        .iter()
        .flat_map(|s| s.split_whitespace())
        .map(|p| Pattern::new(p).map_err(|e| format!("Invalid glob pattern '{}': {}", p, e)))
        .collect::<Result<_, _>>()?;

    let signal_map: SignalMap = if !patterns.is_empty() {
        signal_map
            .into_iter()
            .filter_map(|(handle, mut info)| {
                info.vars.retain(|v| patterns.iter().any(|p| p.matches(&v.name)));
                if info.vars.is_empty() { None } else { Some((handle, info)) }
            })
            .collect()
    } else {
        signal_map
    };

    if args.attrs {
        let mut stdout = std::io::stdout();
        write_attrs(&mut stdout, &signal_map, args.sort)
            .map_err(|e| format!("Failed to write attrs: {}", e))?;
    } else if args.names {
        let names = names_only(&signal_map);
        let mut stdout = std::io::stdout();
        write_names(&mut stdout, &names, args.sort)
            .map_err(|e| format!("Failed to write names: {}", e))?;
    } else {
        let names = names_only(&signal_map);
        let output_options = SignalOutputOptions {
            time_pound: args.time_pound,
            sort: args.sort,
        };

        let mut stdout = std::io::stdout();
        write_signals_wave(
            &mut stdout,
            &mut reader,
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
