# wavetools

Command-line tools for digital simulation waveform analysis -- read, filter, and diff
[FST](https://gtkwave.sourceforge.net/) and [VCD](https://en.wikipedia.org/wiki/Value_change_dump) files.

## Building

```
cargo build --release
```

Binaries are written to `target/release/`.

## wavecat

Read and display waveform files. Auto-detects FST or VCD format from file contents.

### Usage

```
wavecat [OPTIONS] <FILE>
```

### Options

| Option | Description |
|--------|-------------|
| `-s, --start <TIME>` | Starting time |
| `-e, --end <TIME>` | Ending time |
| `-n, --names` | Print variable names only |
| `-a, --attrs` | Print variable attributes (type, size, direction, attributes) |
| `--sort` | Sort entries lexically |
| `--time-pound` | Prefix times with `#` |
| `--no-range-space` | Remove space before range brackets (e.g. `dat[3:0]` instead of `dat [3:0]`) |
| `--format <FORMAT>` | Force file format (`fst` or `vcd`) instead of auto-detecting |
| `-f, --filter <PATTERN>` | Filter signals by glob pattern; may be repeated or space-separated |

### Examples

Dump all signals:

```
$ wavecat sim.fst
0 t.the_sub.cyc_plus_one 00000000000000000000000000000001
0 t.the_sub.cyc 00000000000000000000000000000000
0 t.cyc 00000000000000000000000000000000
0 t.clk 0
10 t.clk 1
10 t.cyc 00000000000000000000000000000001
10 t.the_sub.cyc 00000000000000000000000000000001
10 t.the_sub.cyc_plus_one 00000000000000000000000000000010
20 t.clk 0
30 t.clk 1
30 t.the_sub.cyc_plus_one 00000000000000000000000000000011
30 t.the_sub.cyc 00000000000000000000000000000010
30 t.cyc 00000000000000000000000000000010
40 t.clk 0
50 t.clk 1
```

List signal names (sorted):

```
$ wavecat --names --sort sim.fst
t.clk
t.cyc
t.the_sub.cyc
t.the_sub.cyc_plus_one
```

Filter signals by glob pattern:

```
$ wavecat --filter '*.clk' sim.fst
0 t.clk 0
10 t.clk 1
20 t.clk 0
30 t.clk 1
40 t.clk 0
50 t.clk 1
```

Multiple filters:

```
wavecat --filter "*.clk" --filter "*.reset" sim.fst
```

Dump a specific time range, sorted:

```
$ wavecat --start 20 --end 40 --sort sim.fst
20 t.clk 0
30 t.clk 1
30 t.cyc 00000000000000000000000000000010
30 t.the_sub.cyc 00000000000000000000000000000010
30 t.the_sub.cyc_plus_one 00000000000000000000000000000011
40 t.clk 0
```

VCD-style time format:

```
wavecat --time-pound sim.fst
```

Force VCD parsing:

```
wavecat --format vcd sim.out
```

## wavediff

Compare two waveform files and report differences. Supports any combination of
FST and VCD files (e.g. comparing an FST against a VCD).

### Usage

```
wavediff [OPTIONS] <FILE1> <FILE2>
```

### Options

| Option | Description |
|--------|-------------|
| `-s, --start <TIME>` | Start time for comparison |
| `-e, --end <TIME>` | End time for comparison |
| `--epsilon <VALUE>` | Epsilon for comparing real-valued signals (absolute tolerance) |
| `--no-attrs` | Skip metadata comparison (type, size, direction, attributes) |

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Files are identical |
| 1 | Differences found |
| 2 | Error (e.g. file not found, parse failure) |

### Examples

A signal has a different value at time 10:

```
$ wavediff golden.fst test.fst
10 t.the_sub.cyc_plus_one 00000000000000000000000000000010 != 00000000000000000000000000000100
```

The second file ends earlier, so time 50 is missing:

```
$ wavediff golden.fst short.fst
50 t.clk 1 (missing time in file2)
```

A clock edge moved from time 20 to time 21:

```
$ wavediff golden.fst shifted.fst
20 t.clk 0 (missing time in file2)
21 t.clk 0 (only in file2)
```

Compare an FST file against a VCD file:

```
wavediff golden.fst test.vcd
```

Compare a specific time range:

```
wavediff --start 100 --end 500 golden.fst test.fst
```

Compare with tolerance for real-valued signals:

```
wavediff --epsilon 0.001 golden.fst test.fst
```

Skip metadata comparison (only compare signal values):

```
wavediff --no-attrs golden.fst test.fst
```

## Supported formats

### FST

The FST (Fast Signal Trace) binary format, developed as part of
[GTKWave](https://gtkwave.sourceforge.net/). FST files are compact and support
fast random access to signal data.

### VCD

The VCD (Value Change Dump) text format, defined by IEEE 1364 (Verilog). Wavetools
also supports [GTKWave's extensions](https://gtkwave.sourceforge.net/gtkwave.pdf)
to the VCD format, including additional variable types (`int`, `shortint`,
`longint`, `logic`, `bit`, `byte`, `enum`, `shortreal`), scope types (`struct`,
`union`, `class`, `interface`, `package`), and attributes (`$attrbegin` /
`$attrend`).

## Related projects

- [Surfer](https://surfer-project.org/) -- waveform viewer with VCD and FST support
- [GTKWave](https://gtkwave.sourceforge.net/) -- waveform viewer, originator of the FST format
