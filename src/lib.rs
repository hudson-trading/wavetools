//------------------------------------------------------------------------------
// lib.rs
// Shared waveform library: file opening, hierarchy building, VCD streaming
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

mod cat;
mod diff;

pub use cat::{write_signals_wave, SignalOutputOptions};
pub use diff::{compare_signal_names, diff_waves, open_and_read_waves};

use fst_reader::{is_fst_file, FstHierarchyEntry, FstReader};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, Write};
use std::path::Path;

/// Mapping from signal handle indices to their fully qualified hierarchical names
pub type SignalNames = HashMap<usize, Vec<String>>;

/// An FST reader over a buffered file
pub(crate) type FstFileReader = FstReader<BufReader<File>>;

/// Options for formatting variable names
#[derive(Debug, Clone, Default)]
pub struct NameOptions {
    /// Remove space before range brackets (e.g., "dat[3:0]" vs "dat [3:0]")
    pub no_range_space: bool,
}

/// Build a mapping from signal handles to their fully qualified names
///
/// Returns a HashMap where keys are handle indices and values are vectors of names
/// (one handle can have multiple names/aliases)
fn build_hierarchy<R: BufRead + Seek>(
    fst_reader: &mut fst_reader::FstReader<R>,
    options: &NameOptions,
) -> Result<SignalNames, String> {
    let mut handle_to_names: SignalNames = HashMap::new();
    let mut scope_stack: Vec<String> = Vec::new();

    fst_reader
        .read_hierarchy(|entry| match entry {
            FstHierarchyEntry::Scope { name, .. } => {
                scope_stack.push(name.to_string());
            }
            FstHierarchyEntry::UpScope => {
                scope_stack.pop();
            }
            FstHierarchyEntry::Var { name, handle, .. } => {
                let mut full_path = scope_stack.join(".");
                if !full_path.is_empty() {
                    full_path.push('.');
                }
                let name = if options.no_range_space {
                    name.replace(" [", "[")
                } else {
                    name
                };
                full_path.push_str(&name);
                handle_to_names
                    .entry(handle.get_index())
                    .or_default()
                    .push(full_path);
            }
            _ => {}
        })
        .map_err(|e| format!("Failed to read hierarchy: {}", e))?;

    Ok(handle_to_names)
}

/// Write all variable names from the hierarchy to a writer
pub fn write_names<W: Write>(
    writer: &mut W,
    handle_to_names: &SignalNames,
    sort: bool,
) -> std::io::Result<()> {
    let mut entries: Vec<String> = handle_to_names
        .values()
        .flat_map(|names| names.iter().cloned())
        .collect();

    if sort {
        entries.sort();
    }

    for entry in entries {
        writeln!(writer, "{}", entry)?;
    }
    Ok(())
}

// ── VCD / unified API ────────────────────────────────────────────────────────

/// A streaming VCD reader that yields signal changes on demand
///
/// Wraps a `vcd::Parser` and the id-to-handle mapping built from the header.
/// Changes are read lazily — nothing is buffered beyond the parser's own
/// internal line buffer.
pub struct VcdData {
    parser: vcd::Parser<BufReader<File>>,
    id_to_idx: HashMap<vcd::IdCode, usize>,
    current_time: u64,
}

/// A reader backed by either an FST file (streamed via callbacks) or a VCD
/// file (streamed via an iterator-style parser)
pub enum WaveReader {
    Fst(Box<FstFileReader>),
    Vcd(VcdData),
}

/// Convert a VCD scalar Value to its character representation
fn vcd_value_char(v: vcd::Value) -> char {
    match v {
        vcd::Value::V0 => '0',
        vcd::Value::V1 => '1',
        vcd::Value::X => 'x',
        vcd::Value::Z => 'z',
    }
}

/// Read the next signal change from a VCD parser, skipping non-change commands
pub(crate) fn next_vcd_change(vcd_data: &mut VcdData) -> Option<(u64, usize, String)> {
    while let Some(Ok(cmd)) = vcd_data.parser.next() {
        match cmd {
            vcd::Command::Timestamp(t) => {
                vcd_data.current_time = t;
            }
            vcd::Command::ChangeScalar(id, val) => {
                if let Some(&idx) = vcd_data.id_to_idx.get(&id) {
                    return Some((vcd_data.current_time, idx, vcd_value_char(val).to_string()));
                }
            }
            vcd::Command::ChangeVector(id, ref vec) => {
                if let Some(&idx) = vcd_data.id_to_idx.get(&id) {
                    let s: String = vec.iter().map(vcd_value_char).collect();
                    return Some((vcd_data.current_time, idx, s));
                }
            }
            vcd::Command::ChangeReal(id, val) => {
                if let Some(&idx) = vcd_data.id_to_idx.get(&id) {
                    return Some((vcd_data.current_time, idx, val.to_string()));
                }
            }
            vcd::Command::ChangeString(id, ref s) => {
                if let Some(&idx) = vcd_data.id_to_idx.get(&id) {
                    return Some((vcd_data.current_time, idx, s.clone()));
                }
            }
            _ => {}
        }
    }
    None
}

/// Walk the VCD scope item tree and populate hierarchy maps.
/// `prefix` is the dot-joined path of parent scopes, cached to avoid repeated joins.
fn walk_vcd_items(
    items: &[vcd::ScopeItem],
    prefix: &str,
    handle_to_names: &mut SignalNames,
    id_to_idx: &mut HashMap<vcd::IdCode, usize>,
    options: &NameOptions,
) {
    for item in items {
        match item {
            vcd::ScopeItem::Scope(scope) => {
                let child_prefix = if prefix.is_empty() {
                    scope.identifier.clone()
                } else {
                    format!("{}.{}", prefix, scope.identifier)
                };
                walk_vcd_items(
                    &scope.items,
                    &child_prefix,
                    handle_to_names,
                    id_to_idx,
                    options,
                );
            }
            vcd::ScopeItem::Var(var) => {
                let next_idx = id_to_idx.len();
                let idx = *id_to_idx.entry(var.code).or_insert(next_idx);

                let ref_name = if options.no_range_space {
                    var.reference.replace(" [", "[")
                } else {
                    var.reference.clone()
                };
                let name = match &var.index {
                    Some(vcd::ReferenceIndex::BitSelect(n)) => {
                        format!("{} [{}]", ref_name, n)
                    }
                    Some(vcd::ReferenceIndex::Range(hi, lo)) => {
                        if options.no_range_space {
                            format!("{}[{}:{}]", ref_name, hi, lo)
                        } else {
                            format!("{} [{}:{}]", ref_name, hi, lo)
                        }
                    }
                    None => ref_name,
                };
                let full_path = if prefix.is_empty() {
                    name
                } else {
                    format!("{}.{}", prefix, name)
                };
                handle_to_names.entry(idx).or_default().push(full_path);
            }
            _ => {}
        }
    }
}

/// Build hierarchy from a parsed VCD header
fn build_vcd_hierarchy(
    header: &vcd::Header,
    options: &NameOptions,
) -> (SignalNames, HashMap<vcd::IdCode, usize>) {
    let mut handle_to_names: SignalNames = HashMap::new();
    let mut id_to_idx: HashMap<vcd::IdCode, usize> = HashMap::new();
    walk_vcd_items(
        &header.items,
        "",
        &mut handle_to_names,
        &mut id_to_idx,
        options,
    );
    (handle_to_names, id_to_idx)
}

/// Open a file as FST format
fn open_as_fst(
    buf: BufReader<File>,
    path: &Path,
    options: &NameOptions,
) -> Result<(WaveReader, SignalNames), String> {
    let mut fst_reader = fst_reader::FstReader::open(buf)
        .map_err(|e| format!("Failed to open FST file {}: {}", path.display(), e))?;
    let names = build_hierarchy(&mut fst_reader, options)
        .map_err(|e| format!("Failed to read hierarchy from {}: {}", path.display(), e))?;
    Ok((WaveReader::Fst(Box::new(fst_reader)), names))
}

/// Open a file as VCD format
fn open_as_vcd(
    buf: BufReader<File>,
    path: &Path,
    options: &NameOptions,
) -> Result<(WaveReader, SignalNames), String> {
    let mut parser = vcd::Parser::new(buf).with_gtkwave_extensions(true);
    let header = parser
        .parse_header()
        .map_err(|e| format!("Failed to parse VCD file {}: {}", path.display(), e))?;
    let (handle_to_names, id_to_idx) = build_vcd_hierarchy(&header, options);
    let vcd_data = VcdData {
        parser,
        id_to_idx,
        current_time: 0,
    };
    Ok((WaveReader::Vcd(vcd_data), handle_to_names))
}

/// Waveform file formats that can be forced via `--format`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveFormat {
    Fst,
    Vcd,
}

/// Open a waveform file (FST or VCD) and build its hierarchy
///
/// Format is detected from file contents: FST is checked first via magic bytes,
/// then VCD is attempted. Returns an error if neither format can parse the file.
pub fn open_wave_file(
    path: &Path,
    options: &NameOptions,
) -> Result<(WaveReader, SignalNames), String> {
    open_wave_file_with_format(path, options, None)
}

/// Open a waveform file, optionally forcing a specific format.
///
/// When `format` is `None`, the format is auto-detected from file contents.
/// When `format` is `Some(WaveFormat::Fst)` or `Some(WaveFormat::Vcd)`, that
/// format is used directly and the error message reflects the specific format.
pub fn open_wave_file_with_format(
    path: &Path,
    options: &NameOptions,
    format: Option<WaveFormat>,
) -> Result<(WaveReader, SignalNames), String> {
    let f = File::open(path).map_err(|e| format!("Failed to open {}: {}", path.display(), e))?;

    match format {
        Some(WaveFormat::Fst) => {
            let buf = BufReader::new(f);
            open_as_fst(buf, path, options)
        }
        Some(WaveFormat::Vcd) => {
            let buf = BufReader::new(f);
            open_as_vcd(buf, path, options)
        }
        None => {
            let mut buf = BufReader::new(f);
            if is_fst_file(&mut buf) {
                return open_as_fst(buf, path, options);
            }
            // Not FST — try VCD.
            buf.seek(std::io::SeekFrom::Start(0))
                .map_err(|e| format!("Failed to seek in {}: {}", path.display(), e))?;
            open_as_vcd(buf, path, options)
        }
    }
}
