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
const VCD_OUTPUT_BUFFER_BYTES: usize = 2 * 1024 * 1024;
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

/// Values remain borrowed during direct FST conversion. Only the initial state
/// needed for a time slice owns a copy; real values stay binary until written.
enum OutputValue<'a> {
    Bytes(std::borrow::Cow<'a, [u8]>),
    Real(f64),
}

impl OutputValue<'_> {
    fn into_owned(self) -> OutputValue<'static> {
        match self {
            Self::Bytes(value) => OutputValue::Bytes(value.into_owned().into()),
            Self::Real(value) => OutputValue::Real(value),
        }
    }
}

fn visit_changes(
    mut readers: Vec<WaveReader>,
    offsets: &[usize],
    hierarchy: &WaveHierarchy,
    end: Option<u64>,
    mut emit: impl FnMut(u64, Option<usize>, OutputValue<'_>) -> io::Result<()>,
) -> io::Result<()> {
    if readers.len() != 1 {
        let include = hierarchy.signal_map.keys().copied().collect();
        return merge_changes(readers, offsets, 0, end, Some(&include), |change| {
            emit(
                change.time,
                change.handle,
                OutputValue::Bytes(change.value.into_bytes().into()),
            )
        });
    }
    let offset = offsets[0];
    let final_time = match readers.pop().unwrap() {
        WaveReader::Fst(mut reader) => {
            let header = reader.get_header();
            let include = (hierarchy.signal_map.len() != header.max_handle as usize).then(|| {
                hierarchy
                    .signal_map
                    .keys()
                    .filter_map(|handle| handle.checked_sub(offset))
                    .map(fst_reader::FstSignalHandle::from_index)
                    .collect()
            });
            let filter = fst_reader::FstFilter {
                start: 0,
                end,
                include,
            };
            let mut result = Ok(());
            reader
                .read_signals(&filter, |time, handle, value| {
                    if result.is_ok() && end.is_none_or(|last| time <= last) {
                        let value = match value {
                            fst_reader::FstSignalValue::String(bytes) => {
                                OutputValue::Bytes(bytes.into())
                            }
                            fst_reader::FstSignalValue::Real(value) => OutputValue::Real(value),
                        };
                        result = emit(time, Some(handle.get_index() + offset), value);
                    }
                })
                .map_err(err)?;
            result?;
            header.end_time
        }
        WaveReader::Vcd(mut reader) => {
            let all_signals = hierarchy.signal_map.len() == reader.id_to_idx.len();
            for command in reader.parser.by_ref() {
                let command = command?;
                let id = match &command {
                    vcd::Command::Timestamp(time) => {
                        reader.current_time = *time;
                        if end.is_some_and(|last| *time > last) {
                            break;
                        }
                        continue;
                    }
                    vcd::Command::ChangeScalar(id, _)
                    | vcd::Command::ChangeVector(id, _)
                    | vcd::Command::ChangeReal(id, _)
                    | vcd::Command::ChangeString(id, _) => id,
                    _ => continue,
                };
                let Some(local) = reader.id_to_idx.get(id) else {
                    continue;
                };
                let handle = *local + offset;
                if !all_signals && !hierarchy.signal_map.contains_key(&handle) {
                    continue;
                }
                let scalar;
                let value = match command {
                    vcd::Command::ChangeScalar(_, value) => {
                        scalar = [crate::vcd_value_char(value) as u8];
                        OutputValue::Bytes(scalar.as_slice().into())
                    }
                    vcd::Command::ChangeVector(_, vector) => {
                        let bytes: Vec<u8> = Vec::<vcd::Value>::from(vector)
                            .into_iter()
                            .map(|value| crate::vcd_value_char(value) as u8)
                            .collect();
                        OutputValue::Bytes(bytes.into())
                    }
                    vcd::Command::ChangeReal(_, value) => OutputValue::Real(value),
                    vcd::Command::ChangeString(_, value) => {
                        OutputValue::Bytes(value.into_bytes().into())
                    }
                    _ => unreachable!(),
                };
                emit(reader.current_time, Some(handle), value)?;
            }
            reader.current_time
        }
    };
    emit(
        end.map_or(final_time, |last| final_time.min(last)),
        None,
        OutputValue::Bytes((&[][..]).into()),
    )
}

enum Body {
    Vcd(VcdWriter),
    Fst(Box<FstWriter>, HashMap<usize, FstSignalId>, Vec<u8>),
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
            Self::Fst(w, _, _) => {
                // fst-writer requires flushes to occur immediately before advancing time.
                if w.size() > FST_FLUSH_THRESHOLD_BYTES {
                    w.flush().map_err(err)?;
                }
                w.time_change(time).map_err(err)
            }
        }
    }
    fn change(&mut self, handle: usize, value: OutputValue<'_>, var: &VarEntry) -> io::Result<()> {
        if let OutputValue::Real(value) = value {
            return match self {
                Self::Vcd(w) if is_real(var.meta.var_type) => {
                    w.change_real((handle as u64).into(), value)
                }
                _ => Err(err("Real value on an unsupported signal type")),
            };
        }
        let OutputValue::Bytes(value) = value else {
            unreachable!()
        };
        let value = value.as_ref();
        match self {
            Self::Vcd(w) => {
                let id = vcd::IdCode::from(handle as u64);
                if is_real(var.meta.var_type) {
                    return w.change_real(
                        id,
                        std::str::from_utf8(value)
                            .map_err(err)?
                            .parse::<f64>()
                            .map_err(err)?,
                    );
                }
                if var.meta.var_type == "string" {
                    let value = std::str::from_utf8(value).map_err(err)?;
                    if value.is_empty() || value.chars().any(char::is_whitespace) {
                        return Err(err(
                            "VCD strings must be nonempty and contain no whitespace",
                        ));
                    }
                    return w.change_string(id, value);
                }
                if value.is_empty() || value.len() > var.meta.size as usize {
                    return Err(err("Invalid bit-vector width"));
                }
                if var.meta.size == 1 {
                    let bit = match value[0] {
                        b'0' => vcd::Value::V0,
                        b'1' => vcd::Value::V1,
                        b'x' | b'X' => vcd::Value::X,
                        b'z' | b'Z' => vcd::Value::Z,
                        _ => return Err(err("Invalid four-state signal value")),
                    };
                    w.change_scalar(id, bit)
                } else {
                    w.change_vector_bytes(id, value)
                }
            }
            Self::Fst(w, ids, scratch) => {
                if value.is_empty()
                    || value.len() > var.meta.size as usize
                    || !value
                        .iter()
                        .all(|c| matches!(c, b'0' | b'1' | b'x' | b'z' | b'X' | b'Z'))
                {
                    return Err(err("Invalid four-state signal value or width"));
                }
                if value.len() == var.meta.size as usize
                    && !value.iter().any(u8::is_ascii_uppercase)
                {
                    return w.signal_change(ids[&handle], value).map_err(err);
                }
                // Short VCD vectors extend 0/1 with zero, and x/z with that state.
                let fill = match value[0].to_ascii_lowercase() {
                    b'x' => b'x',
                    b'z' => b'z',
                    _ => b'0',
                };
                scratch.clear();
                scratch.resize(var.meta.size as usize - value.len(), fill);
                scratch.extend(value.iter().map(u8::to_ascii_lowercase));
                w.signal_change(ids[&handle], scratch).map_err(err)
            }
        }
    }
    fn finish(self) -> io::Result<()> {
        match self {
            Self::Vcd(mut w) => w.flush(),
            Self::Fst(w, _, _) => w.finish().map(|_| ()).map_err(err),
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
            let mut writer = vcd::Writer::new(BufWriter::with_capacity(
                VCD_OUTPUT_BUFFER_BYTES,
                File::create(path)?,
            ));
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
    // Most readers use dense handles. Avoid per-change hashing in that case,
    // without allocating a huge table when a filter selects sparse handles.
    let dense = hierarchy
        .signal_map
        .keys()
        .max()
        .copied()
        .filter(|&last| last < hierarchy.signal_map.len().saturating_mul(2))
        .map(|last| {
            let mut vars = vec![None; last + 1];
            for (&handle, info) in &hierarchy.signal_map {
                vars[handle] = info.vars.first();
            }
            vars
        });
    let signal = |handle: usize| match &dense {
        Some(vars) => vars.get(handle).copied().flatten(),
        None => hierarchy
            .signal_map
            .get(&handle)
            .and_then(|info| info.vars.first()),
    };
    let mut initial = HashMap::new();
    let mut current = None;
    let mut initial_open = false;
    let mut final_time = start;
    visit_changes(readers, offsets, hierarchy, end, |time, handle, value| {
        final_time = final_time.max(time);
        let Some(handle) = handle else {
            return Ok(());
        };
        let Some(var) = signal(handle) else {
            return Ok(());
        };
        if time < start {
            initial.insert(handle, value.into_owned());
            return Ok(());
        }
        if current.is_none() {
            let first_time = if initial.is_empty() { time } else { start };
            body.time(first_time)?;
            body.begin_dumpvars()?;
            initial_open = true;
            for (handle, value) in initial.drain() {
                body.change(handle, value, signal(handle).unwrap())?;
            }
            current = Some(first_time);
        }
        if current.is_some_and(|previous| time < previous) {
            return Err(err("Waveform timestamps must not decrease"));
        }
        if current != Some(time) {
            if initial_open {
                body.end_dumpvars()?;
                initial_open = false;
            }
            body.time(time)?;
            current = Some(time);
        }
        body.change(handle, value, var)
    })?;
    if current.is_none() {
        body.time(start)?;
        body.begin_dumpvars()?;
        initial_open = true;
        for (handle, value) in initial {
            body.change(handle, value, signal(handle).unwrap())?;
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
    Ok(Body::Fst(
        Box::new(writer.finish().map_err(err)?),
        ids,
        Vec::new(),
    ))
}
