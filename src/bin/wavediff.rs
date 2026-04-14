//------------------------------------------------------------------------------
// wavediff.rs
// CLI tool to compare two waveform files and report differences
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use clap::Parser;
use std::io::Write;
use std::path::PathBuf;
use wavetools::{compare_signal_meta, compare_signal_names, diff_waves, open_and_read_waves, NameOptions};

#[derive(Parser, Debug)]
#[command(name = "wavediff")]
#[command(about = "Compare two waveform files (FST or VCD)", long_about = "\
Compare two waveform files (FST or VCD format) by signal name and value.

Exit codes:
  0  files are identical
  1  differences found
  2  error

Examples:
  wavediff baseline.fst current.fst
  wavediff --start 100 --end 500 sim1.vcd sim2.vcd
  wavediff --epsilon 0.001 analog1.fst analog2.vcd")]
struct Args {
    /// First waveform file to compare (FST or VCD)
    file1: PathBuf,

    /// Second waveform file to compare (FST or VCD)
    file2: PathBuf,

    /// Start time for comparison
    #[arg(short = 's', long)]
    start: Option<u64>,

    /// End time for comparison
    #[arg(short = 'e', long)]
    end: Option<u64>,

    /// Epsilon for comparing real-valued signals (absolute tolerance)
    #[arg(long)]
    epsilon: Option<f64>,

    /// Skip metadata comparison (type, size, direction, attributes)
    #[arg(long)]
    no_attrs: bool,
}

fn main() {
    let args = Args::parse();

    match run(args) {
        Ok(has_differences) => {
            if has_differences {
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(2);
        }
    }
}

fn run(args: Args) -> Result<bool, String> {
    if let (Some(s), Some(e)) = (args.start, args.end) {
        if s > e {
            return Err(format!("--start ({}) must be <= --end ({})", s, e));
        }
    }
    if let Some(eps) = args.epsilon {
        if eps < 0.0 {
            return Err(format!("--epsilon must be non-negative, got {}", eps));
        }
    }

    let name_options = NameOptions::default();
    let (reader1, signal_map1, reader2, signal_map2) =
        open_and_read_waves(&args.file1, &args.file2, &name_options)?;

    let (only_in_1, only_in_2) = compare_signal_names(&signal_map1, &signal_map2);

    if !only_in_1.is_empty() || !only_in_2.is_empty() {
        eprintln!("Signal name mismatch:");
        let print_sorted = |label: std::path::Display, names: std::collections::HashSet<String>| {
            eprintln!("  Only in {}:", label);
            let mut names: Vec<_> = names.into_iter().collect();
            names.sort();
            for name in names {
                eprintln!("    {}", name);
            }
        };
        if !only_in_1.is_empty() {
            print_sorted(args.file1.display(), only_in_1);
        }
        if !only_in_2.is_empty() {
            print_sorted(args.file2.display(), only_in_2);
        }
        return Err("Signal names differ between files".to_string());
    }

    let mut has_differences = false;

    if !args.no_attrs {
        let meta_diffs = compare_signal_meta(&signal_map1, &signal_map2);
        if !meta_diffs.is_empty() {
            has_differences = true;
            let mut stderr = std::io::stderr();
            for diff in &meta_diffs {
                let _ = writeln!(stderr, "{}", diff);
            }
        }
    }

    let mut stdout = std::io::stdout();
    let value_diffs = diff_waves(
        &mut stdout,
        reader1,
        &signal_map1,
        reader2,
        &signal_map2,
        args.start.unwrap_or(0),
        args.end,
        args.epsilon,
    )
    .map_err(|e| format!("Failed to diff files: {}", e))?;

    Ok(has_differences || value_diffs)
}
