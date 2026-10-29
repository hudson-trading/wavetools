# Repository Guidelines

## Project Structure & Module Organization

wavetools is a Rust package using the 2021 edition, with a shared library and two command-line tools for FST and VCD waveforms. `src/bin/` contains `wavecat` and `wavediff`; `src/lib.rs` provides shared readers and hierarchy handling. `cat.rs` and `diff.rs` implement display and comparison; `output.rs` and `output_header.rs` implement export. `src/vcd/` contains vendored VCD code.

Integration tests live in `tests/`, with small waveform fixtures and expected output in `tests/data/`. `tests/performance/` generates large benchmark inputs; `tests/verilator_integration/` exercises Verilator trace tests. Build products belong in `target/`.

## Build, Test, and Development Commands

Use stable Rust; the declared minimum Rust version is 1.85.

- `cargo build --release --locked --bins`: build optimized tools in `target/release/`.
- `cargo run --bin wavecat -- --help`: inspect local CLI usage.
- `cargo test --locked`: run unit, integration, data-driven, and documentation tests.
- `cargo test --test diff_test`: run comparison integration tests.
- `scripts/rustfmt-non-vendored --check`: check formatting; omit `--check` to format.
- `cargo clippy --all-targets --all-features -- -D warnings`: lint production and test code strictly.
- `python3 -m unittest discover -s tests/performance -p 'test_*.py'`: test the benchmark harness.

Install configured hooks with `pre-commit install`.

## Coding Style & Naming Conventions

Use four-space Rust indentation, `snake_case` functions/modules, `PascalCase` types, and `SCREAMING_SNAKE_CASE` constants. Follow existing SPDX copyright/license headers in owned Rust files. Use the repository formatter rather than blanket `cargo fmt`, which reformats vendored code. Keep vendored changes focused and intentional. Source-hygiene tests require ASCII-only text and SPDX headers in owned Rust files.

## Testing Guidelines

Use Rust `#[test]` functions with descriptive behavior-based names. Golden-output tests use `datatest-stable`; keep expected files consistent with their waveform fixtures. Add regression coverage for correctness fixes, especially aliases, repeated timestamps, metadata, and cross-format behavior. No numerical coverage threshold is configured.

For performance-sensitive changes, run `python3 tests/performance/run.py`. It builds release binaries and compares mean runtime and peak memory against `HEAD^`; increases above 5% fail. One baseline is cached in `target/performance/baselines/`, replaced when its identity or build settings change. Use `--baseline-ref HEAD` to measure uncommitted changes, `--baseline-version VERSION` for a published release, or `--gtkwave` for informational converter comparisons. Reports and diagnostics live in `target/performance/`; generated waveforms are temporary. Do not commit large waveforms. See [the performance README](tests/performance/README.md) for controls and interpretation.

## Commit & Pull Request Guidelines

History uses short descriptive subjects, sometimes with a scope such as `CI:`; no uniform Conventional Commits scheme is apparent. Prefer focused, action-oriented commits. Describe the problem, resulting behavior, relevant issue links, and validation in PRs. Include benchmark evidence for performance changes and update CLI documentation when options or behavior change.
