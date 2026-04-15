//------------------------------------------------------------------------------
// cat.rs
// Waveform signal output: reading and formatting signal values
//
// SPDX-FileCopyrightText: Hudson River Trading
// SPDX-License-Identifier: MIT
//------------------------------------------------------------------------------

use std::io::{BufRead, Seek, Write};
use std::sync::mpsc;

use fst_reader::{FstFilter, FstSignalHandle, FstSignalValue};

use crate::{next_vcd_change, SignalNames, WaveReader};

/// Options for outputting signals
#[derive(Debug, Clone, Default)]
pub struct SignalOutputOptions {
    /// Prefix times with #
    pub time_pound: bool,
    /// Sort signals by name within each time
    pub sort: bool,
}

fn write_signal_line<W: Write>(
    writer: &mut W,
    time: u64,
    name: &str,
    value: &str,
    time_pound: bool,
) -> std::io::Result<()> {
    let pound = if time_pound { "#" } else { "" };
    writeln!(writer, "{}{} {} {}", pound, time, name, value)
}

fn flush_signal_batch<W: Write>(
    writer: &mut W,
    time: u64,
    batch: &mut Vec<(String, String)>,
    time_pound: bool,
) -> std::io::Result<()> {
    batch.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (name, value) in batch.drain(..) {
        write_signal_line(writer, time, &name, &value, time_pound)?;
    }
    Ok(())
}

/// Read and write signal values to a writer
fn write_signals<R: BufRead + Seek, W: Write>(
    writer: &mut W,
    fst_reader: &mut fst_reader::FstReader<R>,
    handle_to_names: &SignalNames,
    filter: &fst_reader::FstFilter,
    options: &SignalOutputOptions,
) -> std::io::Result<()> {
    let mut current_time: Option<u64> = None;
    let mut batch: Vec<(String, String)> = Vec::new();
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

            if let Some(names) = handle_to_names.get(&handle.get_index()) {
                for name in names {
                    if options.sort {
                        if let Some(prev_time) = current_time {
                            if prev_time != time {
                                if let Err(e) =
                                    flush_signal_batch(writer, prev_time, &mut batch, options.time_pound)
                                {
                                    write_error = Some(e);
                                    return;
                                }
                            }
                        }
                        current_time = Some(time);
                        batch.push((name.clone(), value_str.clone()));
                    } else if let Err(e) =
                        write_signal_line(writer, time, name, &value_str, options.time_pound)
                    {
                        write_error = Some(e);
                        return;
                    }
                }
            } else {
                eprintln!("Warning: unknown FST handle {}", handle.get_index());
            }
        })
        .map_err(|e| std::io::Error::other(format!("Failed to read signals: {}", e)))?;

    if let Some(e) = write_error {
        return Err(e);
    }

    if options.sort {
        if let Some(time) = current_time {
            flush_signal_batch(writer, time, &mut batch, options.time_pound)?;
        }
    }

    Ok(())
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
struct CatChange {
    time: u64,
    handle: usize,
    value: String,
}

/// Send signal changes from a single WaveReader through a channel.
/// Sends raw handle indices (offset-adjusted); name resolution happens on the consumer side.
fn send_cat_changes(
    mut reader: WaveReader,
    handle_offset: usize,
    start: u64,
    end: Option<u64>,
    tx: mpsc::Sender<CatChange>,
) {
    match &mut reader {
        WaveReader::Fst(fst_reader) => {
            let filter = FstFilter {
                start,
                end,
                include: None,
            };
            let _ = fst_reader.read_signals(&filter, |time, handle, value| {
                if time < filter.start {
                    return;
                }
                if let Some(e) = filter.end {
                    if time > e {
                        return;
                    }
                }
                let value_str = match value {
                    FstSignalValue::String(s) => {
                        std::str::from_utf8(s).unwrap_or("<invalid utf8>").to_string()
                    }
                    FstSignalValue::Real(r) => r.to_string(),
                };
                let _ = tx.send(CatChange {
                    time,
                    handle: handle.get_index() + handle_offset,
                    value: value_str,
                });
            });
        }
        WaveReader::Vcd(vcd_data) => {
            while let Some((time, handle_idx, value_str)) = next_vcd_change(vcd_data) {
                if time < start {
                    continue;
                }
                if let Some(e) = end {
                    if time > e {
                        break;
                    }
                }
                let _ = tx.send(CatChange {
                    time,
                    handle: handle_idx + handle_offset,
                    value: value_str,
                });
            }
        }
    }
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

    // Spawn a thread per reader, each sending CatChanges to its own channel.
    // Threads send raw handle+value; name resolution happens here on the consumer side,
    // avoiding a full handle_to_names clone per thread.
    let mut rxs = Vec::with_capacity(readers.len());
    let mut threads = Vec::new();

    for (reader, &offset) in readers.into_iter().zip(offsets.iter()) {
        let (tx, rx) = mpsc::channel();
        threads.push(std::thread::spawn(move || {
            send_cat_changes(reader, offset, start, end, tx);
        }));
        rxs.push(rx);
    }

    let mut current_time: Option<u64> = None;
    let mut batch: Vec<(String, String)> = Vec::new();

    crate::kway_merge_channels(&rxs, |c| c.time, |change| {
        if options.sort {
            if let Some(prev_time) = current_time {
                if prev_time != change.time {
                    flush_signal_batch(writer, prev_time, &mut batch, options.time_pound)?;
                }
            }
            current_time = Some(change.time);
        }
        if let Some(names) = handle_to_names.get(&change.handle) {
            for name in names {
                if options.sort {
                    batch.push((name.clone(), change.value.clone()));
                } else {
                    write_signal_line(
                        writer,
                        change.time,
                        name,
                        &change.value,
                        options.time_pound,
                    )?;
                }
            }
        }
        Ok(())
    })?;

    if options.sort {
        if let Some(time) = current_time {
            flush_signal_batch(writer, time, &mut batch, options.time_pound)?;
        }
    }

    for t in threads {
        t.join().unwrap();
    }

    Ok(())
}

fn write_vcd_signals<W: Write>(
    writer: &mut W,
    vcd_data: &mut crate::VcdData,
    handle_to_names: &SignalNames,
    start: u64,
    end: Option<u64>,
    options: &SignalOutputOptions,
) -> std::io::Result<()> {
    let mut current_batch_time: Option<u64> = None;
    let mut batch: Vec<(String, String)> = Vec::new();

    while let Some((time, handle_idx, value_str)) = next_vcd_change(vcd_data) {
        if time < start {
            continue;
        }
        if let Some(e) = end {
            if time > e {
                break;
            }
        }
        if let Some(names) = handle_to_names.get(&handle_idx) {
            for name in names {
                if options.sort {
                    if let Some(prev) = current_batch_time {
                        if prev != time {
                            flush_signal_batch(writer, prev, &mut batch, options.time_pound)?;
                        }
                    }
                    current_batch_time = Some(time);
                    batch.push((name.clone(), value_str.clone()));
                } else {
                    write_signal_line(writer, time, name, &value_str, options.time_pound)?;
                }
            }
        }
    }

    if options.sort {
        if let Some(time) = current_batch_time {
            flush_signal_batch(writer, time, &mut batch, options.time_pound)?;
        }
    }

    Ok(())
}
