# Performance regression harness

Run from any directory on Linux with Python 3.9+, Rust/Cargo, and GNU
`/usr/bin/time` installed:

```sh
python3 tests/performance/run.py
```

The harness builds the checkout with `cargo build --release --locked --bins`,
explicitly setting `--target-dir` to the checkout's `target/` directory.
By default it compares against **the previous commit (`HEAD^`)**, built in an
isolated Git worktree. A single baseline is cached in `target/performance/baselines/bin/`.
Metadata records the release version or Git commit, compiler, and build environment.
A matching baseline skips the build; a different baseline replaces it.
Worktrees and Cargo build products stay in `target/performance-baseline-build/`;
only portable executables and build metadata need caching between CI jobs.
Uncommitted changes in the current checkout are included in the current build.
Both builds use release optimization; timing excludes compilation.

```sh
python3 tests/performance/run.py --baseline-ref HEAD~3 --runs 10 --size-mib 512
python3 tests/performance/run.py --baseline-ref HEAD  # measure uncommitted changes
```

To compare against a published release, use `--baseline-version VERSION` or
`--baseline-version latest`. This checks cached `target/performance/baselines/bin/` executable
versions before running `cargo install wavetools --version =VERSION --root
target/performance/baselines --locked --force`. Release lookup is never needed for Git baselines.
`--print-baseline-id` resolves the selected commit/version without building.
Git comparisons require that revision in local history; fetch it if the checkout
is shallow. A baseline must support `wavecat --out`.

Add `--gtkwave` to compare conversion against `fst2vcd -e` and `vcd2fst` on PATH.
These additional comparisons are informational, not 5% regression gates. They
use the same warm-ups and alternating order and record output file sizes.
Generated outputs are checked with baseline wavediff outside timing.

## Workload and cases

A deterministic generator streams an approximately 256 MiB VCD containing 8,192
64-bit wire signals by default. Each timestamp assigns seeded pseudorandom data
to every signal. This avoids a highly compressible repeating waveform; expect
an FST of roughly tens of MiB, with actual sizes recorded in the report. The
baseline wavecat converts the VCD into FST once, outside measurements. Increasing
`--size-mib` scales the change data; `--signals` scales declaration count and
hierarchy/state overhead. `--seed` selects the data sequence.

The five cases are:

- `wavediff input.fst identical.fst`
- `wavediff input.vcd identical.vcd`
- `wavediff input.vcd input.fst`
- `wavecat input.fst --out output.vcd`
- `wavecat input.vcd --out output.fst`

The same-format comparisons use separate byte-for-byte copies. The mixed-format
comparison uses the generated VCD and its baseline-converted FST.
Conversion output goes to real files, not `/dev/null`. Commands must exit
successfully. Each conversion's warm-up output is also compared against its
input with the baseline wavediff, outside measurements. Validation stdout and
stderr are retained together in the validation `.stderr` log, including waveform
differences on failure. These are digital-only
workloads; real/string/event behavior and alias-heavy hierarchies remain covered
by correctness tests, not this performance workload.

Waveforms live in a temporary subdirectory of `target/performance` and are
removed when the harness finishes, including normal error exits. SIGKILL can
leave the scratch directory behind. No large input files are committed.

## Measurements and failures

Each binary gets one unmeasured warm-up per case, followed by **10 measured
runs** (configurable with `--runs`). Runs are sequential, alternating which
binary runs first. Input generation, installation, compilation, copying,
correctness validation, and deletion are outside timed regions. Tool stdout is
discarded; stderr is recorded for diagnostics. No terminal output is expected
from successful comparisons or conversions.

GNU time measures command wall time and peak resident memory in KiB, including
the tool's worker threads. Its elapsed-time resolution is 0.01 seconds, so use
large workloads; zero-duration samples are rejected. For **each case and each
metric**, compare the arithmetic mean across measured runs. If the checkout's
mean exceeds the baseline mean by **more than 5%**, the harness reports FAIL and
returns exit status 1. It never averages away a failure across cases. Setup,
command, validation, and timeout failures return 2. The per-command timeout is
600 seconds, configurable with `--timeout`.

This measures warm-cache conversion to buffered files, including file close but
without an explicit fsync. It does not measure cold storage or durable write
throughput. Shared GitHub runners can be noisy at a 5% threshold; inspect raw
samples and rerun suspected noisy failures.

The final table shows baseline and current means, percentage changes, and pass/fail
for each case. Negative changes are better (faster or less memory); positive
changes are worse (slower or more memory). A positive change fails only when it
exceeds 5%. The threshold is not relaxed for CI.
Use an otherwise idle machine for local comparisons.

`target/performance/results.json` contains individual samples, averages, growth
percentages, tool versions and binary hashes, checkout commit/dirty status, workload parameters, input sizes,
and pass/fail results. Stderr logs are retained beside it. Use `--work-dir` to
choose another scratch/report directory and `--skip-build` to use an existing
`target/release` build (the caller is responsible for its freshness).

## CI and harness tests

The Performance workflow runs on pull requests and pushes to main, and supports
manual runs with configurable repetitions, size, and baseline revision or release. It caches
`target/performance/baselines` by resolved revision/platform and still checks executable versions on
every invocation. Git history is fetched for revision baselines. JSON results and stderr diagnostics are uploaded even when
comparisons fail. Installation and build happen in the same job before timing.

Run the harness unit tests without network access:

```sh
python3 -m unittest discover -s tests/performance -p 'test_*.py'
```

## Baseline self-check

To measure variability without any binary or build differences, run the same
baseline executables on both sides:

```sh
python3 tests/performance/run.py --baseline-self-check --work-dir target/performance-self-check
```

This skips building the checkout but keeps the same workloads, warm-ups,
alternating execution order, measurements, and 5% threshold. Any reported
regression in this mode is a false positive caused by measurement variability.
Use a separate work directory to preserve the original comparison report.

## Benchmarking a separate build

Use `--current-bin /path/to/release` to benchmark separately built binaries.
This skips building current binaries and records their directory in the report;
the caller is responsible for their freshness and release profile.
