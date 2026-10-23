// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT

use crate::{cat::merge_changes, vcd, VarEntry, WaveFormat, WaveHierarchy, WaveReader};
use fst_writer::{FstBodyWriter, FstSignalId, FstSignalType, FstVarDirection, FstVarType};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufWriter},
    path::Path,
};

// Flush buffered FST changes after approximately 64 MiB, at a time boundary.
const FST_FLUSH_THRESHOLD_BYTES: usize = 64 * 1024 * 1024;
// VCD files without a declared timescale are interpreted as 1 ns.
const DEFAULT_TIMESCALE_EXPONENT: i8 = -9;

type VcdWriter = vcd::Writer<BufWriter<File>>;
type FstWriter = FstBodyWriter<BufWriter<File>>;

fn err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

fn is_real(t: &str) -> bool {
    matches!(t, "real" | "realtime" | "shortreal" | "real_parameter")
}

fn timescale(reader: &WaveReader) -> io::Result<i8> {
    match reader {
        WaveReader::Fst(r) => Ok(r.get_header().timescale_exponent),
        WaveReader::Vcd(r) => {
            let Some((factor, unit)) = r.header.timescale else {
                return Ok(DEFAULT_TIMESCALE_EXPONENT);
            };
            let power = match factor {
                1 => 0,
                10 => 1,
                100 => 2,
                _ => return Err(err("Unsupported VCD timescale multiplier")),
            };
            Ok(power - unit.divisor().ilog10() as i8)
        }
    }
}

fn vcd_timescale(exponent: i8) -> io::Result<(u32, vcd::TimescaleUnit)> {
    use vcd::TimescaleUnit::*;
    for (base, unit) in [(-15, FS), (-12, PS), (-9, NS), (-6, US), (-3, MS), (0, S)] {
        if (base..=base + 2).contains(&exponent) {
            return Ok((10u32.pow((exponent - base) as u32), unit));
        }
    }
    Err(err(format!(
        "Cannot represent timescale 10^{exponent} seconds in VCD"
    )))
}

fn fst_type(t: &str) -> io::Result<FstVarType> {
    use FstVarType::*;
    Ok(match t {
        "integer" => Integer,
        "parameter" => Parameter,
        "reg" => Reg,
        "supply0" => Supply0,
        "supply1" => Supply1,
        "time" => Time,
        "tri" => Tri,
        "triand" => TriAnd,
        "trior" => TriOr,
        "trireg" => TriReg,
        "tri0" => Tri0,
        "tri1" => Tri1,
        "wand" => Wand,
        "wire" => Wire,
        "wor" => Wor,
        "bit" => Bit,
        "logic" => Logic,
        "int" => Int,
        "shortint" => ShortInt,
        "longint" => LongInt,
        "byte" => Byte,
        "enum" => Enum,
        _ => {
            return Err(err(format!(
                "fst-writer does not support exporting '{t}' signals; use --out-format vcd"
            )))
        }
    })
}

fn direction(d: &str) -> FstVarDirection {
    match d {
        "input" => FstVarDirection::Input,
        "output" => FstVarDirection::Output,
        "inout" => FstVarDirection::InOut,
        "buffer" => FstVarDirection::Buffer,
        "linkage" => FstVarDirection::Linkage,
        _ => FstVarDirection::Implicit,
    }
}

enum Body {
    Vcd(VcdWriter),
    Fst(Box<FstWriter>, HashMap<usize, FstSignalId>),
}
impl Body {
    fn begin_dumpvars(&mut self) -> io::Result<()> {
        if let Self::Vcd(writer) = self {
            writer.begin(vcd::SimulationCommand::Dumpvars)?;
        }
        Ok(())
    }

    fn end_dumpvars(&mut self) -> io::Result<()> {
        if let Self::Vcd(writer) = self {
            writer.end()?;
        }
        Ok(())
    }

    fn time(&mut self, time: u64) -> io::Result<()> {
        match self {
            Self::Vcd(w) => w.timestamp(time),
            Self::Fst(w, _) => {
                // fst-writer requires flushes to occur immediately before advancing time.
                if w.size() > FST_FLUSH_THRESHOLD_BYTES {
                    w.flush().map_err(err)?;
                }
                w.time_change(time).map_err(err)
            }
        }
    }
    fn change(&mut self, handle: usize, value: &str, var: &VarEntry) -> io::Result<()> {
        match self {
            Self::Vcd(w) => {
                let id = vcd::IdCode::from(handle as u64);
                if is_real(var.meta.var_type) {
                    return w.change_real(id, value.parse::<f64>().map_err(err)?);
                }
                if var.meta.var_type == "string" {
                    if value.is_empty() || value.chars().any(char::is_whitespace) {
                        return Err(err(
                            "VCD strings must be nonempty and contain no whitespace",
                        ));
                    }
                    return w.change_string(id, value);
                }
                let bits = value
                    .bytes()
                    .map(|c| match c {
                        b'0' => Ok(vcd::Value::V0),
                        b'1' => Ok(vcd::Value::V1),
                        b'x' | b'X' => Ok(vcd::Value::X),
                        b'z' | b'Z' => Ok(vcd::Value::Z),
                        _ => Err(err("Invalid four-state signal value")),
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                if bits.is_empty() || bits.len() > var.meta.size as usize {
                    return Err(err("Invalid bit-vector width"));
                }
                if var.meta.size == 1 {
                    w.change_scalar(id, bits[0])
                } else {
                    w.change_vector(id, bits)
                }
            }
            Self::Fst(w, ids) => {
                let value = value.to_ascii_lowercase();
                if value.is_empty()
                    || value.len() > var.meta.size as usize
                    || !value
                        .bytes()
                        .all(|c| matches!(c, b'0' | b'1' | b'x' | b'z'))
                {
                    return Err(err("Invalid four-state signal value or width"));
                }
                // VCD permits shortened vectors: zero extend 0/1, sign extend x/z.
                let fill = match value.as_bytes()[0] {
                    b'x' => 'x',
                    b'z' => 'z',
                    _ => '0',
                };
                let mut padded = fill
                    .to_string()
                    .repeat(var.meta.size as usize - value.len());
                padded.push_str(&value);
                w.signal_change(ids[&handle], padded.as_bytes())
                    .map_err(err)
            }
        }
    }
    fn finish(self) -> io::Result<()> {
        match self {
            Self::Vcd(mut w) => w.flush(),
            Self::Fst(w, _) => w.finish().map_err(err),
        }
    }
}

/// Options for waveform file output.
#[derive(Debug, Default)]
pub struct WaveOutputOptions {
    /// First simulation timestamp to include.
    pub start: u64,
    /// Last simulation timestamp to include (inclusive).
    pub end: Option<u64>,
    /// Omit the recording date; simulation timestamps are unaffected.
    pub no_date: bool,
}

/// Export a filtered hierarchy and merged signal changes as VCD or FST.
/// Times remain absolute. At `start`, emit the most recent known value of each
/// signal, followed by changes through the inclusive `end`.
/// VCD output preserves source scopes and GTKWave annotations.
/// FST output emits module scopes and omits extended annotations.
/// FST output currently supports fixed-width four-state signals only;
/// events, real values, and variable-length strings require changes to `fst-writer`.
pub fn write_wave_file(
    path: &Path,
    format: WaveFormat,
    mut readers: Vec<WaveReader>,
    offsets: &[usize],
    hierarchy: &WaveHierarchy,
    options: &WaveOutputOptions,
) -> io::Result<()> {
    let WaveOutputOptions {
        start,
        end,
        no_date,
    } = *options;
    if readers.is_empty() || readers.len() != offsets.len() {
        return Err(err("Invalid reader offsets"));
    }
    if end.is_some_and(|end| start > end) {
        return Err(err("Start time exceeds end time"));
    }
    let exponent = timescale(&readers[0])?;
    for reader in &readers[1..] {
        if timescale(reader)? != exponent {
            return Err(err("Cannot export an overlay with different timescales"));
        }
    }
    // File-wide descriptive metadata comes from the first input.
    let (date, version) = match &readers[0] {
        WaveReader::Fst(reader) => {
            let header = reader.get_header();
            (Some(header.date), Some(header.version))
        }
        WaveReader::Vcd(reader) => (reader.header.date.clone(), reader.header.version.clone()),
    };
    let date = date.filter(|value| !no_date && !value.trim().is_empty());
    let mut body = match format {
        WaveFormat::Vcd => {
            let (factor, unit) = vcd_timescale(exponent)?;
            let mut writer = vcd::Writer::new(BufWriter::new(File::create(path)?));
            if let Some(date) = date.as_deref() {
                writer.date(date)?;
            }
            if let Some(version) = version.filter(|value| !value.trim().is_empty()) {
                writer.version(&version)?;
            }
            writer.timescale(factor, unit)?;
            crate::output_header::write_header(&mut writer, &mut readers, offsets, hierarchy)?;
            Body::Vcd(writer)
        }
        WaveFormat::Fst => {
            create_fst_body(path, exponent, hierarchy, date.as_deref().unwrap_or(""))?
        }
    };
    let mut initial = HashMap::new();
    let mut current = None;
    let mut initial_open = false;
    let mut final_time = start;
    merge_changes(readers, offsets, 0, end, |change| {
        final_time = final_time.max(change.time);
        let Some(handle) = change.handle else {
            return Ok(());
        };
        let Some(info) = hierarchy.signal_map.get(&handle) else {
            return Ok(());
        };
        if change.time < start {
            initial.insert(handle, change.value);
            return Ok(());
        }
        if current.is_none() {
            let first_time = if initial.is_empty() {
                change.time
            } else {
                start
            };
            body.time(first_time)?;
            body.begin_dumpvars()?;
            initial_open = true;
            for (handle, value) in initial.drain() {
                body.change(handle, &value, &hierarchy.signal_map[&handle].vars[0])?;
            }
            current = Some(first_time);
        }
        if current.is_some_and(|previous| change.time < previous) {
            return Err(err("Waveform timestamps must not decrease"));
        }
        if current != Some(change.time) {
            if initial_open {
                body.end_dumpvars()?;
                initial_open = false;
            }
            body.time(change.time)?;
            current = Some(change.time);
        }
        body.change(handle, &change.value, &info.vars[0])
    })?;
    if current.is_none() {
        body.time(start)?;
        body.begin_dumpvars()?;
        initial_open = true;
        for (handle, value) in initial {
            body.change(handle, &value, &hierarchy.signal_map[&handle].vars[0])?;
        }
    }
    if initial_open {
        body.end_dumpvars()?;
    }
    // fst-writer snapshots initial values on the first positive timestamp.
    let last = end.unwrap_or(final_time);
    let last = if format == WaveFormat::Fst {
        last.max(1)
    } else {
        last
    };
    if current.unwrap_or(start) < last {
        body.time(last)?;
    }
    body.finish()
}

/// FST uses a reconstructed, sorted hierarchy; VCD follows the source header.
fn create_fst_body(
    path: &Path,
    exponent: i8,
    hierarchy: &WaveHierarchy,
    date: &str,
) -> io::Result<Body> {
    let mut vars: Vec<_> = hierarchy
        .signal_map
        .iter()
        .flat_map(|(&handle, info)| {
            info.vars
                .iter()
                .map(move |var| (hierarchy.names.segments(var.name), handle, var))
        })
        .collect();
    vars.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, _, var) in &vars {
        fst_type(var.meta.var_type)?;
        if var.meta.size == 0 || var.meta.size == u32::MAX {
            return Err(err("Unsupported FST signal width"));
        }
    }
    let mut writer = fst_writer::open_fst(
        path,
        &fst_writer::FstInfo {
            start_time: 0,
            timescale_exponent: exponent,
            version: "wavetools".into(),
            date: date.trim().into(),
            file_type: fst_writer::FstFileType::Verilog,
        },
    )
    .map_err(err)?;
    let mut scopes: Vec<&str> = Vec::new();
    let mut ids = HashMap::new();
    for (parts, handle, var) in vars {
        let (name, parents) = parts.split_last().ok_or_else(|| err("Empty signal name"))?;
        let common = scopes
            .iter()
            .zip(parents)
            .take_while(|(a, b)| a == b)
            .count();
        for _ in common..scopes.len() {
            writer.up_scope().map_err(err)?;
        }
        scopes.truncate(common);
        for scope in &parents[common..] {
            writer
                .scope(scope, "", fst_writer::FstScopeType::Module)
                .map_err(err)?;
            scopes.push(scope);
        }
        let id = writer
            .var(
                name,
                FstSignalType::bit_vec(var.meta.size),
                fst_type(var.meta.var_type)?,
                direction(var.meta.direction),
                ids.get(&handle).copied(),
            )
            .map_err(err)?;
        ids.insert(handle, id);
    }
    for _ in scopes {
        writer.up_scope().map_err(err)?;
    }
    Ok(Body::Fst(Box::new(writer.finish().map_err(err)?), ids))
}
