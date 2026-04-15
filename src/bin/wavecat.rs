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
    names_only, open_wave_files, write_attrs, write_names, write_signals_wave_multi, NameOptions,
    SignalMap, SignalOutputOptions, WaveFormat,
};
use std::path::PathBuf;
use std::process;

#[derive(Parser, Debug)]
#[command(name = "wavecat")]
#[command(about = "Read and display waveform files (FST or VCD)", long_about = "\
Read and display waveform files (FST or VCD format).
Multiple files are overlayed (their signals are unioned).

Examples:
  wavecat sim.fst
  wavecat --names --sort sim.vcd
  wavecat --names --sort clk.vcd counters.vcd
  wavecat --start 100 --end 500 sim.fst
  wavecat --filter '*.clk' --time-pound sim.fst
  wavecat --format vcd dump.dat")]
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

fn parse_filter_patterns(filter: &[String]) -> Result<Vec<Pattern>, String> {
    filter
        .iter()
        .flat_map(|s| s.split_whitespace())
        .map(|p| Pattern::new(p).map_err(|e| format!("Invalid glob pattern '{}': {}", p, e)))
        .collect()
}

fn apply_filters(signal_map: SignalMap, patterns: &[Pattern]) -> SignalMap {
    if patterns.is_empty() {
        return signal_map;
    }
    signal_map
        .into_iter()
        .filter_map(|(handle, mut info)| {
            info.vars.retain(|v| patterns.iter().any(|p| p.matches(&v.name)));
            if info.vars.is_empty() { None } else { Some((handle, info)) }
        })
        .collect()
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
    let patterns = parse_filter_patterns(&args.filter)?;

    let paths: Vec<&std::path::Path> = args.file.iter().map(|p| p.as_path()).collect();
    let (readers, signal_map, offsets) = open_wave_files(&paths, &name_options, args.format)?;

    let signal_map = apply_filters(signal_map, &patterns);

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
