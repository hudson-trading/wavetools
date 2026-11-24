//------------------------------------------------------------------------------
// cat.rs
// Waveform signal output: reading and formatting signal values
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use std::io::{self, BufRead, Seek, Write};

use crossbeam_channel as channel;
use fst_reader::{FstFilter, FstSignalHandle, FstSignalValue};

use crate::{next_vcd_change, SignalNames, WaveReader};

/// Max queued signal changes per channel. Bounds memory while keeping producers
/// busy: at ~150 bytes/change this caps each channel at ~10 MB of backlog.
const CHANNEL_BOUND: usize = 64 * 1024;

/// Options for outputting signals
#[derive(Debug, Clone, Default)]
pub struct SignalOutputOptions {
    /// Prefix times with #
    pub time_pound: bool,
    /// Sort signals by name within each time
    pub sort: bool,
    /// Show only the last value of each signal at each time, leaving out
    /// the changes before it (delta cycles)
    pub no_delta: bool,
}

fn write_signal_line<W: Write>(
    writer: &mut W,
    time: u64,
    delta: Option<usize>,
    name: &str,
    value: &str,
    time_pound: bool,
) -> std::io::Result<()> {
    let pound = if time_pound { "#" } else { "" };
    match delta {
        Some(delta) => writeln!(writer, "{}{}({}) {} {}", pound, time, delta, name, value),
        None => writeln!(writer, "{}{} {} {}", pound, time, name, value),
    }
}

/// Writes the signal listing one time step at a time. A signal can change
/// more than once at the same time (delta cycles): its last change is shown
/// at `TIME`, and the ones before it at `TIME(DELTA)`, counting from 0.
struct StepWriter<'a, W: Write> {
    writer: &'a mut W,
    options: &'a SignalOutputOptions,
    time: Option<u64>,
    /// The changes at `time`: handle, names and value.
    changes: Vec<(usize, &'a [String], String)>,
    /// The delta of each change in `changes`, or `None` for the last change
    /// of its handle.
    deltas: Vec<Option<usize>>,
    /// Changes of each handle at `time`, and how many of them were seen,
    /// indexed by handle.
    counts: Vec<(usize, usize)>,
    /// Name and position in `changes` of each row, for `--sort`.
    sorted: Vec<(&'a str, usize)>,
}

impl<'a, W: Write> StepWriter<'a, W> {
    fn new(writer: &'a mut W, options: &'a SignalOutputOptions) -> Self {
        StepWriter {
            writer,
            options,
            time: None,
            changes: Vec::new(),
            deltas: Vec::new(),
            counts: Vec::new(),
            sorted: Vec::new(),
        }
    }

    /// Add a change at `time`; `None` only marks the time.
    fn change(
        &mut self,
        time: u64,
        change: Option<(usize, &'a [String])>,
        value: String,
    ) -> io::Result<()> {
        if self.time != Some(time) {
            self.flush()?;
            self.time = Some(time);
        }
        if let Some((handle, names)) = change {
            self.changes.push((handle, names, value));
        }
        Ok(())
    }

    /// Write the changes buffered for the current time.
    fn flush(&mut self) -> io::Result<()> {
        let Some(time) = self.time else {
            return Ok(());
        };
        for &(handle, _, _) in &self.changes {
            if handle >= self.counts.len() {
                self.counts.resize(handle + 1, (0, 0));
            }
            self.counts[handle].0 += 1;
        }
        self.deltas.clear();
        for &(handle, _, _) in &self.changes {
            let (count, seen) = &mut self.counts[handle];
            self.deltas.push((*seen + 1 < *count).then_some(*seen));
            *seen += 1;
        }
        for &(handle, _, _) in &self.changes {
            self.counts[handle] = (0, 0);
        }

        let options = self.options;
        if options.sort {
            self.sorted.clear();
            for (i, &(_, names, _)) in self.changes.iter().enumerate() {
                if self.deltas[i].is_none() || !options.no_delta {
                    self.sorted
                        .extend(names.iter().map(|name| (name.as_str(), i)));
                }
            }
            // A stable sort keeps the changes of a signal in order.
            self.sorted.sort_by(|a, b| a.0.cmp(b.0));
            for &(name, i) in &self.sorted {
                write_signal_line(
                    self.writer,
                    time,
                    self.deltas[i],
                    name,
                    &self.changes[i].2,
                    options.time_pound,
                )?;
            }
        } else {
            for (i, (_, names, value)) in self.changes.iter().enumerate() {
                if self.deltas[i].is_some() && options.no_delta {
                    continue;
                }
                for name in names.iter() {
                    write_signal_line(
                        self.writer,
                        time,
                        self.deltas[i],
                        name,
                        value,
                        options.time_pound,
                    )?;
                }
            }
        }
        self.changes.clear();
        Ok(())
    }
}

/// Read and write signal values to a writer
fn write_signals<R: BufRead + Seek, W: Write>(
    writer: &mut W,
    fst_reader: &mut fst_reader::FstReader<R>,
    handle_to_names: &SignalNames,
    filter: &fst_reader::FstFilter,
    options: &SignalOutputOptions,
) -> std::io::Result<()> {
    let mut step = StepWriter::new(writer, options);
    let mut write_error: Option<std::io::Error> = None;

    fst_reader
        .read_signals(filter, |time, handle, value| {
            if write_error.is_some() {
                return;
            }
            // FstFilter narrows data blocks but may include out-of-range times
            if time < filter.start {
                return;
            }
            if let Some(e) = filter.end {
                if time > e {
                    return;
                }
            }

            let value_str = match value {
                FstSignalValue::String(s) => std::str::from_utf8(s)
                    .unwrap_or("<invalid utf8>")
                    .to_string(),
                FstSignalValue::Real(r) => r.to_string(),
            };

            let handle = handle.get_index();
            if let Some(names) = handle_to_names.get(&handle) {
                if let Err(e) = step.change(time, Some((handle, names.as_slice())), value_str) {
                    write_error = Some(e);
                }
            } else {
                eprintln!("Warning: unknown FST handle {}", handle);
            }
        })
        .map_err(|e| std::io::Error::other(format!("Failed to read signals: {}", e)))?;

    if let Some(e) = write_error {
        return Err(e);
    }

    step.flush()
}

/// Write signal values from a WaveReader to a writer
///
/// Works for both FST and VCD readers. The `start`/`end` times bound the output window.
pub fn write_signals_wave<W: Write>(
    writer: &mut W,
    reader: &mut WaveReader,
    handle_to_names: &SignalNames,
    start: u64,
    end: Option<u64>,
    options: &SignalOutputOptions,
) -> std::io::Result<()> {
    match reader {
        WaveReader::Fst(fst_reader) => {
            let include: Vec<FstSignalHandle> = handle_to_names
                .keys()
                .map(|&idx| FstSignalHandle::from_index(idx))
                .collect();
            let include = if include.is_empty() {
                None
            } else {
                Some(include)
            };
            let filter = FstFilter {
                start,
                end,
                include,
            };
            write_signals(writer, fst_reader, handle_to_names, &filter, options)
        }
        WaveReader::Vcd(vcd_data) => {
            write_vcd_signals(writer, vcd_data, handle_to_names, start, end, options)
        }
    }
}

/// A signal change sent through a channel for multi-reader merging
pub(crate) struct CatChange {
    pub time: u64,
    /// None marks a final timestamp without a value change.
    pub handle: Option<usize>,
    pub value: String,
}

/// Send signal changes from a single WaveReader through a channel.
/// Sends raw handle indices (offset-adjusted); name resolution happens on the consumer side.
fn send_cat_changes(
    mut reader: WaveReader,
    handle_offset: usize,
    start: u64,
    end: Option<u64>,
    tx: channel::Sender<CatChange>,
    include: Option<std::collections::HashSet<usize>>,
) -> io::Result<()> {
    match &mut reader {
        WaveReader::Fst(fst_reader) => {
            let filter = FstFilter {
                start,
                end,
                include: include.as_ref().map(|handles| {
                    handles
                        .iter()
                        .copied()
                        .map(fst_reader::FstSignalHandle::from_index)
                        .collect()
                }),
            };
            let read_result = fst_reader.read_signals(&filter, |time, handle, value| {
                if time < filter.start {
                    return;
                }
                if let Some(e) = filter.end {
                    if time > e {
                        return;
                    }
                }
                let value_str = match value {
                    FstSignalValue::String(s) => std::str::from_utf8(s)
                        .unwrap_or("<invalid utf8>")
                        .to_string(),
                    FstSignalValue::Real(r) => r.to_string(),
                };
                let _ = tx.send(CatChange {
                    time,
                    handle: Some(handle.get_index() + handle_offset),
                    value: value_str,
                });
            });
            read_result.map_err(|e| io::Error::other(format!("Failed to read signals: {}", e)))?;
        }
        WaveReader::Vcd(vcd_data) => {
            while let Some((time, handle_idx, value_str)) = next_vcd_change(vcd_data)? {
                if time < start {
                    continue;
                }
                if let Some(e) = end {
                    if time > e {
                        break;
                    }
                }
                if include
                    .as_ref()
                    .is_some_and(|handles| !handles.contains(&handle_idx))
                {
                    continue;
                }
                if tx
                    .send(CatChange {
                        time,
                        handle: Some(handle_idx + handle_offset),
                        value: value_str,
                    })
                    .is_err()
                {
                    break;
                }
            }
        }
    }
    // Carry the final timestamp even when no signal changes there.
    let final_time = match &reader {
        WaveReader::Fst(r) => r.get_header().end_time,
        WaveReader::Vcd(r) => r.current_time,
    };
    let _ = tx.send(CatChange {
        time: end.map_or(final_time, |end| end.min(final_time)).max(start),
        handle: None,
        value: String::new(),
    });
    Ok(())
}

/// Write signal values from multiple WaveReaders, merging their outputs in time order.
///
/// Each reader's handles are offset by the corresponding entry in `offsets`.
/// For a single reader, delegates to `write_signals_wave` for zero overhead.
pub fn write_signals_wave_multi<W: Write>(
    writer: &mut W,
    mut readers: Vec<WaveReader>,
    offsets: &[usize],
    handle_to_names: &SignalNames,
    start: u64,
    end: Option<u64>,
    options: &SignalOutputOptions,
) -> std::io::Result<()> {
    if readers.len() == 1 && offsets[0] == 0 {
        return write_signals_wave(
            writer,
            &mut readers[0],
            handle_to_names,
            start,
            end,
            options,
        );
    }

    let mut step = StepWriter::new(writer, options);
    let include = handle_to_names.keys().copied().collect();
    merge_changes(readers, offsets, start, end, Some(&include), |change| {
        let names = change
            .handle
            .and_then(|handle| Some((handle, handle_to_names.get(&handle)?.as_slice())));
        step.change(change.time, names, change.value)
    })?;
    step.flush()
}

fn write_vcd_signals<W: Write>(
    writer: &mut W,
    vcd_data: &mut crate::VcdData,
    handle_to_names: &SignalNames,
    start: u64,
    end: Option<u64>,
    options: &SignalOutputOptions,
) -> std::io::Result<()> {
    let mut step = StepWriter::new(writer, options);
    while let Some((time, handle_idx, value_str)) = next_vcd_change(vcd_data)? {
        if time < start {
            continue;
        }
        if let Some(e) = end {
            if time > e {
                break;
            }
        }
        if let Some(names) = handle_to_names.get(&handle_idx) {
            step.change(time, Some((handle_idx, names.as_slice())), value_str)?;
        }
    }
    step.flush()
}

/// Stream merged changes with bounded queues and propagate reader errors.
pub(crate) fn merge_changes(
    readers: Vec<WaveReader>,
    offsets: &[usize],
    start: u64,
    end: Option<u64>,
    include: Option<&std::collections::HashSet<usize>>,
    on_change: impl FnMut(CatChange) -> io::Result<()>,
) -> io::Result<()> {
    // Spawn a thread per reader, each sending CatChanges to its own channel.
    // Threads send raw handle+value; name resolution happens here on the consumer side,
    // avoiding a full handle_to_names clone per thread.
    let mut rxs = Vec::with_capacity(readers.len());
    let mut threads = Vec::new();

    for (reader, &offset) in readers.into_iter().zip(offsets.iter()) {
        let count = match &reader {
            WaveReader::Fst(r) => r.get_header().max_handle as usize,
            WaveReader::Vcd(r) => r.id_to_idx.len(),
        };
        let selected = include.and_then(|handles| {
            let local: std::collections::HashSet<_> = handles
                .iter()
                .copied()
                .filter(|handle| (offset..offset + count).contains(handle))
                .map(|handle| handle - offset)
                .collect();
            (local.len() != count).then_some(local)
        });
        let (tx, rx) = channel::bounded(CHANNEL_BOUND);
        threads.push(std::thread::spawn(move || {
            send_cat_changes(reader, offset, start, end, tx, selected)
        }));
        rxs.push(rx);
    }

    let merge_result = crate::kway_merge_channels(&rxs, |c| c.time, on_change);
    if merge_result.is_err() {
        drop(rxs);
    }

    let mut thread_result = Ok(());
    for t in threads {
        match t.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                if thread_result.is_ok() {
                    thread_result = Err(e);
                }
            }
            Err(_) => {
                if thread_result.is_ok() {
                    thread_result = Err(io::Error::other("wavecat reader thread panicked"));
                }
            }
        }
    }

    merge_result?;
    thread_result?;

    Ok(())
}
