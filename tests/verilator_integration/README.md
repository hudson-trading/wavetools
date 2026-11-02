# Verilator integration tests

Run `tests/verilator_integration/run.sh` to build Verilator and run the tests
that use `trace_identical`, with this checkout's release wavediff binary.
The script documents its environment variables and required Python version.

Managed sources and build products live in `.verilator-integration/verilator/`.
Keep them outside Cargo's `target/`: `Swatinem/rust-cache` can retain Git metadata
while pruning Verilator source files. CI uses a separate, versioned cache keyed
by the Verilator tag, operating system, and architecture.

The runner validates required source files, the requested tag, and tracked-file
changes before reusing its managed checkout. An invalid managed checkout is
replaced only after a fresh clone succeeds. The default tag is the latest
release; set `VERILATOR_TAG` to choose one explicitly.

Set `VERILATOR_DIR` to use an existing complete source tree instead. The runner
never replaces that directory. If `VERILATOR_TAG` is also supplied, the checkout
must match it. A missing `bin/verilator_bin` triggers a build.

Run `python3 -m pytest tests/verilator_integration` for source-cache regression
tests. These use temporary local Git repositories and require no network or
Verilator build.
