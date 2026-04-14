//------------------------------------------------------------------------------
// lib.rs
// Shared waveform library: file opening, hierarchy building, VCD streaming
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

mod cat;
mod diff;
#[allow(dead_code, unused_imports, clippy::manual_repeat_n, mismatched_lifetime_syntaxes)]
mod vcd;

pub use cat::{write_signals_wave, SignalOutputOptions};
pub use diff::{compare_signal_names, compare_signal_meta, diff_waves, open_and_read_waves};

use fst_reader::{
    is_fst_file, FstHierarchyEntry, FstReader, FstVarDirection, FstVarType,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, Write};
use std::path::Path;

/// Mapping from signal handle indices to their fully qualified hierarchical names
pub type SignalNames = HashMap<usize, Vec<String>>;

/// Direction string for signals with no explicit direction
const IMPLICIT_DIRECTION: &str = "implicit";

/// Variable metadata normalized across FST and VCD formats
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarMeta {
    /// Canonical type string: "wire", "reg", "integer", "real", etc.
    pub var_type: String,
    /// Bit width
    pub size: u32,
    /// Direction: "implicit", "input", "output", "inout", "buffer", "linkage"
    pub direction: String,
}

/// A single variable declaration: name, metadata, and optional attributes.
/// Aliased signals (same handle/id code) each get their own entry since
/// they can have different declared types.
#[derive(Debug, Clone)]
pub struct VarEntry {
    pub name: String,
    pub meta: VarMeta,
    /// Formatted attribute strings (enum tables, array types, etc.)
    pub attrs: Vec<String>,
}

/// Per-handle signal info: one or more variable declarations that share
/// the same underlying signal data (aliases).
#[derive(Debug, Clone)]
pub struct SignalInfo {
    pub vars: Vec<VarEntry>,
}

/// Mapping from signal handle indices to their full info
pub type SignalMap = HashMap<usize, SignalInfo>;

/// Extract just the names from a SignalMap, for use in streaming code
pub fn names_only(map: &SignalMap) -> SignalNames {
    map.iter()
        .map(|(&k, info)| (k, info.vars.iter().map(|v| v.name.clone()).collect()))
        .collect()
}

/// An FST reader over a buffered file
pub(crate) type FstFileReader = FstReader<BufReader<File>>;

/// Options for formatting variable names
#[derive(Debug, Clone, Default)]
pub struct NameOptions {
    /// Remove space before range brackets (e.g., "dat[3:0]" vs "dat [3:0]")
    pub no_range_space: bool,
}

/// Convert an FST variable type to its canonical VCD string
fn fst_var_type_str(t: FstVarType) -> &'static str {
    match t {
        FstVarType::Event => "event",
        FstVarType::Integer => "integer",
        FstVarType::Parameter => "parameter",
        FstVarType::Real => "real",
        FstVarType::RealParameter => "real_parameter",
        FstVarType::Reg => "reg",
        FstVarType::Supply0 => "supply0",
        FstVarType::Supply1 => "supply1",
        FstVarType::Time => "time",
        FstVarType::Tri => "tri",
        FstVarType::TriAnd => "triand",
        FstVarType::TriOr => "trior",
        FstVarType::TriReg => "trireg",
        FstVarType::Tri0 => "tri0",
        FstVarType::Tri1 => "tri1",
        FstVarType::Wand => "wand",
        FstVarType::Wire => "wire",
        FstVarType::Wor => "wor",
        FstVarType::Port => "port",
        FstVarType::SparseArray => "sparray",
        FstVarType::RealTime => "realtime",
        FstVarType::GenericString => "string",
        FstVarType::Bit => "bit",
        FstVarType::Logic => "logic",
        FstVarType::Int => "int",
        FstVarType::ShortInt => "shortint",
        FstVarType::LongInt => "longint",
        FstVarType::Byte => "byte",
        FstVarType::Enum => "enum",
        FstVarType::ShortReal => "shortreal",
    }
}

/// Convert an FST variable direction to its canonical string
fn fst_direction_str(d: FstVarDirection) -> &'static str {
    match d {
        FstVarDirection::Implicit => IMPLICIT_DIRECTION,
        FstVarDirection::Input => "input",
        FstVarDirection::Output => "output",
        FstVarDirection::InOut => "inout",
        FstVarDirection::Buffer => "buffer",
        FstVarDirection::Linkage => "linkage",
    }
}

/// Build a SignalMap from an FST hierarchy
fn build_hierarchy<R: BufRead + Seek>(
    fst_reader: &mut fst_reader::FstReader<R>,
    options: &NameOptions,
) -> Result<SignalMap, String> {
    let mut signal_map: SignalMap = HashMap::new();
    let mut scope_stack: Vec<String> = Vec::new();
    let mut last_handle: Option<usize> = None;
    let mut enum_tables: HashMap<u64, (String, Vec<(String, String)>)> = HashMap::new();

    fst_reader
        .read_hierarchy(|entry| match entry {
            FstHierarchyEntry::Scope { name, .. } => {
                scope_stack.push(name.to_string());
                last_handle = None;
            }
            FstHierarchyEntry::UpScope => {
                scope_stack.pop();
                last_handle = None;
            }
            FstHierarchyEntry::Var {
                tpe,
                direction,
                name,
                length,
                handle,
                ..
            } => {
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
                let idx = handle.get_index();
                last_handle = Some(idx);
                // FST stores real sizes in bytes (8); normalize to bits (64)
                // to match VCD convention.
                let size = match tpe {
                    FstVarType::Real | FstVarType::RealParameter | FstVarType::RealTime => 64,
                    _ => length,
                };
                let entry = signal_map.entry(idx).or_insert_with(|| SignalInfo {
                    vars: Vec::new(),
                });
                entry.vars.push(VarEntry {
                    name: full_path,
                    meta: VarMeta {
                        var_type: fst_var_type_str(tpe).to_string(),
                        size,
                        direction: fst_direction_str(direction).to_string(),
                    },
                    attrs: Vec::new(),
                });
            }
            FstHierarchyEntry::EnumTable {
                name,
                handle,
                mapping,
            } => {
                enum_tables.insert(handle, (name, mapping));
            }
            FstHierarchyEntry::EnumTableRef { handle } => {
                if let Some(var_idx) = last_handle {
                    if let Some((name, mapping)) = enum_tables.get(&handle) {
                        push_attr(&mut signal_map, var_idx, format_enum_attr(name, mapping));
                    }
                }
            }
            _ => {}
        })
        .map_err(|e| format!("Failed to read hierarchy: {}", e))?;

    Ok(signal_map)
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

/// Registry of enum table definitions, keyed by handle ID.
/// Each entry stores the table name and its (value_name, encoding) pairs.
type EnumTableRegistry = HashMap<i64, (String, Vec<(String, String)>)>;

/// Parse a VCD enum table definition from the name field of a `misc 07` attribute.
/// Format: `<name> <count> <val1> ... <valN> <enc1> ... <encN>`
/// Returns the table name and key=value pairs matching the FST enum format.
fn parse_vcd_enum_table(name: &str) -> Option<(String, Vec<(String, String)>)> {
    let parts: Vec<&str> = name.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let table_name = parts[0].to_string();
    let count: usize = parts[1].parse().ok()?;
    if parts.len() < 2 + count * 2 {
        return None;
    }
    let values = &parts[2..2 + count];
    let encodings = &parts[2 + count..2 + count * 2];
    let mapping: Vec<(String, String)> = values
        .iter()
        .zip(encodings.iter())
        .map(|(v, e)| (v.to_string(), e.to_string()))
        .collect();
    Some((table_name, mapping))
}

/// Format an enum table as a string matching the FST enum attr format.
fn format_enum_attr(name: &str, mapping: &[(String, String)]) -> String {
    let pairs: Vec<String> = mapping.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
    format!("enum {}: {}", name, pairs.join(" "))
}

/// Push an attribute string onto the last VarEntry for a given handle.
fn push_attr(signal_map: &mut SignalMap, handle: usize, attr: String) {
    if let Some(info) = signal_map.get_mut(&handle) {
        if let Some(var_entry) = info.vars.last_mut() {
            var_entry.attrs.push(attr);
        }
    }
}

/// Walk the VCD scope item tree and populate hierarchy maps.
/// `prefix` is the dot-joined path of parent scopes, cached to avoid repeated joins.
fn walk_vcd_items(
    items: &[vcd::ScopeItem],
    prefix: &str,
    signal_map: &mut SignalMap,
    id_to_idx: &mut HashMap<vcd::IdCode, usize>,
    enum_tables: &mut EnumTableRegistry,
    options: &NameOptions,
) {
    let mut last_idx: Option<usize> = None;
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
                    signal_map,
                    id_to_idx,
                    enum_tables,
                    options,
                );
                last_idx = None;
            }
            vcd::ScopeItem::Var(var) => {
                let next_idx = id_to_idx.len();
                let idx = *id_to_idx.entry(var.code).or_insert(next_idx);
                last_idx = Some(idx);

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
                let entry = signal_map.entry(idx).or_insert_with(|| SignalInfo {
                    vars: Vec::new(),
                });
                entry.vars.push(VarEntry {
                    name: full_path,
                    meta: VarMeta {
                        var_type: var.var_type.to_string(),
                        size: var.size,
                        direction: IMPLICIT_DIRECTION.to_string(),
                    },
                    attrs: Vec::new(),
                });
            }
            vcd::ScopeItem::Attribute(attr) => {
                // For misc 07 (enum table): distinguish definitions from references.
                // Definitions have a non-empty, non-"" name with the full enum details.
                // References have "" as the name and the handle as arg.
                let is_enum_table = attr.attr_type == vcd::AttributeType::Misc
                    && attr.subtype == "07";
                if is_enum_table {
                    let name_trimmed = attr.name.trim_matches('"');
                    if !name_trimmed.is_empty() {
                        // Enum table definition — register it and attach to current signal
                        if let Some(parsed) = parse_vcd_enum_table(&attr.name) {
                            enum_tables.insert(attr.arg, parsed.clone());
                            if let Some(idx) = last_idx {
                                push_attr(signal_map, idx, format_enum_attr(&parsed.0, &parsed.1));
                            }
                        }
                    } else {
                        // Enum table reference — resolve from registry
                        if let Some(idx) = last_idx {
                            if let Some((name, mapping)) = enum_tables.get(&attr.arg) {
                                push_attr(signal_map, idx, format_enum_attr(name, mapping));
                            }
                        }
                    }
                } else if let Some(idx) = last_idx {
                    let attr_str = format!("{} {}: {} {}", attr.attr_type, attr.subtype, attr.name, attr.arg);
                    push_attr(signal_map, idx, attr_str);
                }
            }
            _ => {}
        }
    }
}

/// Build hierarchy from a parsed VCD header
fn build_vcd_hierarchy(
    header: &vcd::Header,
    options: &NameOptions,
) -> (SignalMap, HashMap<vcd::IdCode, usize>) {
    let mut signal_map: SignalMap = HashMap::new();
    let mut id_to_idx: HashMap<vcd::IdCode, usize> = HashMap::new();
    let mut enum_tables: EnumTableRegistry = HashMap::new();
    walk_vcd_items(
        &header.items,
        "",
        &mut signal_map,
        &mut id_to_idx,
        &mut enum_tables,
        options,
    );
    (signal_map, id_to_idx)
}

/// Write all signal attributes from the hierarchy to a writer
pub fn write_attrs<W: Write>(
    writer: &mut W,
    signal_map: &SignalMap,
    sort: bool,
) -> std::io::Result<()> {
    let mut entries: Vec<(&str, &VarMeta, &[String])> = signal_map
        .values()
        .flat_map(|info| {
            info.vars
                .iter()
                .map(|v| (v.name.as_str(), &v.meta, v.attrs.as_slice()))
        })
        .collect();

    if sort {
        entries.sort_by_key(|(name, _, _)| *name);
    }

    for (name, meta, attrs) in entries {
        if meta.direction != IMPLICIT_DIRECTION {
            writeln!(writer, "{}  {}  {}  {}", name, meta.var_type, meta.size, meta.direction)?;
        } else {
            writeln!(writer, "{}  {}  {}", name, meta.var_type, meta.size)?;
        }
        for attr in attrs {
            writeln!(writer, "  {}", attr)?;
        }
    }
    Ok(())
}

/// Open a file as FST format
fn open_as_fst(
    buf: BufReader<File>,
    path: &Path,
    options: &NameOptions,
) -> Result<(WaveReader, SignalMap), String> {
    let mut fst_reader = fst_reader::FstReader::open(buf)
        .map_err(|e| format!("Failed to open FST file {}: {}", path.display(), e))?;
    let signal_map = build_hierarchy(&mut fst_reader, options)
        .map_err(|e| format!("Failed to read hierarchy from {}: {}", path.display(), e))?;
    Ok((WaveReader::Fst(Box::new(fst_reader)), signal_map))
}

/// Open a file as VCD format
fn open_as_vcd(
    buf: BufReader<File>,
    path: &Path,
    options: &NameOptions,
) -> Result<(WaveReader, SignalMap), String> {
    let mut parser = vcd::Parser::new(buf).with_gtkwave_extensions(true);
    let header = parser
        .parse_header()
        .map_err(|e| format!("Failed to parse VCD file {}: {}", path.display(), e))?;
    let (signal_map, id_to_idx) = build_vcd_hierarchy(&header, options);
    let vcd_data = VcdData {
        parser,
        id_to_idx,
        current_time: 0,
    };
    Ok((WaveReader::Vcd(vcd_data), signal_map))
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
) -> Result<(WaveReader, SignalMap), String> {
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
) -> Result<(WaveReader, SignalMap), String> {
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
