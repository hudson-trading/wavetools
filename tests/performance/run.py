#!/usr/bin/env python3
# SPDX-FileCopyrightText: Hudson River Trading
# SPDX-License-Identifier: MIT
"""Compare local release binaries with a Git revision or published release (Linux)."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import random
import re
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
BASELINE = ROOT / "target" / "performance" / "baselines"
TOOLS = ("wavecat", "wavediff")
MAX_INCREASE = 0.05


def log(message):
    print(message, flush=True)


def latest_version():
    request = urllib.request.Request(
        "https://crates.io/api/v1/crates/wavetools",
        headers={"User-Agent": "wavetools-performance-harness"},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        version = json.load(response)["crate"]["max_stable_version"]
    return release_version(version)


def release_version(value):
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", value):
        raise ValueError(f"Expected a stable release version, got {value!r}")
    return value


def binary_version(path):
    try:
        result = subprocess.run(
            [str(path), "--version"], capture_output=True, text=True, timeout=10,
            check=True,
        )
        return result.stdout.split()[1]
    except (OSError, subprocess.SubprocessError, IndexError):
        return None


def baseline_ready(directory, version):
    return all(binary_version(directory / "bin" / tool) == version for tool in TOOLS)


def build_fingerprint():
    return {
        "compiler": subprocess.check_output(["rustc", "-vV"], text=True),
        "environment": {k: v for k, v in os.environ.items()
                        if k.startswith(("CARGO_", "RUST", "CC", "CXX", "CFLAGS", "LDFLAGS"))},
    }


def install_baseline(version):
    marker = BASELINE / "build.json"
    spec = {"kind": "release", "version": version, **build_fingerprint()}
    if (marker.exists() and json.loads(marker.read_text()) == spec
            and baseline_ready(BASELINE, version)):
        log(f"Reusing wavetools {version} in {BASELINE}")
        return
    log(f"Installing crates.io wavetools {version} in {BASELINE}")
    BASELINE.mkdir(parents=True, exist_ok=True)
    marker.unlink(missing_ok=True)
    subprocess.run([
        "cargo", "install", "wavetools", "--version", f"={version}",
        "--root", str(BASELINE), "--locked", "--force",
    ], check=True, cwd=ROOT)
    if not baseline_ready(BASELINE, version):
        raise RuntimeError("Baseline install did not produce both requested tool versions")
    marker.write_text(json.dumps(spec, indent=2) + "\n")


def resolve_baseline(args):
    if args.baseline_version:
        version = latest_version() if args.baseline_version == "latest" else release_version(args.baseline_version)
        return {"kind": "release", "version": version}
    revision = subprocess.check_output(
        ["git", "rev-parse", "--verify", f"{args.baseline_ref or 'HEAD^'}^{{commit}}"],
        cwd=ROOT, text=True,
    ).strip()
    return {"kind": "commit", "revision": revision}


def prepare_baseline(spec):
    if spec["kind"] == "release":
        install_baseline(spec["version"])
        return BASELINE / "bin"
    revision = spec["revision"]
    fingerprint = {
        "kind": "commit",
        "revision": revision,
        **build_fingerprint(),
    }
    directory = BASELINE
    marker = directory / "build.json"
    if marker.exists() and json.loads(marker.read_text()) == fingerprint and all(
            binary_version(directory / "bin" / tool) for tool in TOOLS):
        log(f"Reusing commit {revision[:12]} in {directory}")
        return directory / "bin"
    directory.mkdir(parents=True, exist_ok=True)
    marker.unlink(missing_ok=True)
    # Cache only portable binaries/metadata. Worktree .git files point into this
    # checkout and would become stale when a CI cache is restored elsewhere.
    target = ROOT / "target" / "performance-baseline-build"
    target.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="source-", dir=target) as scratch:
        source = Path(scratch) / "checkout"
        subprocess.run(["git", "worktree", "add", "--detach", str(source), revision],
                       cwd=ROOT, check=True)
        try:
            subprocess.run(["cargo", "build", "--release", "--locked", "--bins",
                            "--target-dir", str(target / "build")], cwd=source, check=True)
        finally:
            subprocess.run(["git", "worktree", "remove", "--force", str(source)],
                           cwd=ROOT, check=True)
    (directory / "bin").mkdir(exist_ok=True)
    for tool in TOOLS:
        shutil.copy2(target / "build" / "release" / tool, directory / "bin" / tool)
    marker.write_text(json.dumps(fingerprint, indent=2) + "\n")
    return directory / "bin"


def id_code(index):
    result = ""
    while True:
        result += chr(33 + index % 94)
        index //= 94
        if not index:
            return result


def generate_vcd(path, size_bytes, signals, seed):
    """Bounded-memory, deterministic 64-bit digital stimulus with little repetition."""
    rng = random.Random(seed)
    ids = [id_code(i) for i in range(signals)]
    with path.open("wb") as output:
        output.write(b"$timescale 1ns $end\n$scope module top $end\n")
        for i, code in enumerate(ids):
            output.write(f"$var wire 64 {code} signal_{i} [63:0] $end\n".encode())
        output.write(b"$upscope $end\n$enddefinitions $end\n")
        timestamp = 0
        while output.tell() < size_bytes or timestamp == 0:
            output.write(f"#{timestamp}\n".encode())
            if timestamp == 0:
                output.write(b"$dumpvars\n")
            output.writelines(
                f"b{rng.getrandbits(64):064b} {code}\n".encode() for code in ids
            )
            if timestamp == 0:
                output.write(b"$end\n")
            timestamp += 10


def execute(command, stderr_path, timeout, timing_path=None, capture_stdout=False):
    """Run quietly, killing the whole subprocess group on timeout."""
    if timing_path is not None:
        command = ["/usr/bin/time", "-f", "%e %M", "-o", str(timing_path), *command]
    with stderr_path.open("wb") as stderr:
        process = subprocess.Popen(
            [str(arg) for arg in command],
            stdout=stderr if capture_stdout else subprocess.DEVNULL, stderr=stderr,
            start_new_session=True,
        )
        try:
            status = process.wait(timeout=timeout)
        except (subprocess.TimeoutExpired, KeyboardInterrupt) as error:
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()
            if isinstance(error, KeyboardInterrupt):
                raise
            raise RuntimeError(f"Timed out after {timeout}s: {command}") from None
    if status:
        with stderr_path.open("rb") as error:
            error.seek(max(0, stderr_path.stat().st_size - 4096))
            details = error.read().decode(errors="replace")
        raise RuntimeError(f"Command failed ({status}): {command}\n{details}")
    if timing_path is None:
        return None
    seconds, rss = timing_path.read_text().split()
    elapsed, rss = float(seconds), int(rss)
    if not math.isfinite(elapsed) or elapsed <= 0:
        raise RuntimeError("Workload too short for GNU time; increase --size-mib")
    if rss <= 0:
        raise RuntimeError("GNU time did not report a positive peak RSS")
    return {"seconds": elapsed, "peak_rss_kib": rss}


def compare(samples):
    metrics = {}
    for metric in ("seconds", "peak_rss_kib"):
        baseline = statistics.mean(s[metric] for s in samples["baseline"])
        current = statistics.mean(s[metric] for s in samples["current"])
        metrics[metric] = {
            "baseline_mean": baseline, "current_mean": current,
            "growth_percent": (current / baseline - 1) * 100,
            "failed": current > baseline * (1 + MAX_INCREASE),
        }
    return metrics


def print_summary(report):
    log("\nResults (arithmetic means; baseline -> current)")
    log("Change: negative (-) is better: faster / less memory; positive (+) is worse.")
    log(f"FAIL means runtime or memory increased by more than {MAX_INCREASE:.0%}; GTKWave comparisons are INFO only.")
    rows = [("Case", "Time (s)", "Change", "Peak RSS (MiB)", "Change", "Result")]
    for name, case in report["cases"].items():
        time = case["metrics"]["seconds"]
        rss = case["metrics"]["peak_rss_kib"]
        rows.append((
            name,
            f"{time['baseline_mean']:.3f} -> {time['current_mean']:.3f}",
            f"{time['growth_percent']:+.2f}%",
            f"{rss['baseline_mean'] / 1024:.1f} -> {rss['current_mean'] / 1024:.1f}",
            f"{rss['growth_percent']:+.2f}%",
            "INFO" if not case.get("gated", True) else "FAIL" if time["failed"] or rss["failed"] else "PASS",
        ))
    widths = [max(len(cell) for cell in column) for column in zip(*rows)]
    for index, row in enumerate(rows):
        log(" | ".join(cell.ljust(width) for cell, width in zip(row, widths)))
        if index == 0:
            log("-+-".join("-" * width for width in widths))

def parse_cpu_list(value):
    cpus = set()
    for part in value.strip().split(","):
        if part:
            bounds = [int(v) for v in part.split("-")]
            cpus.update(range(bounds[0], bounds[-1] + 1))
    return cpus


def select_cpus(allowed, system=Path("/sys/devices/system")):
    """Prefer up to three physical cores sharing L3, within existing affinity."""
    def read(path):
        try:
            return path.read_text().strip()
        except OSError:
            return ""

    cores = {}
    groups = set()
    for cpu in sorted(allowed):
        directory = system / "cpu" / f"cpu{cpu}"
        socket = read(directory / "topology/physical_package_id")
        core = read(directory / "topology/core_id")
        cores[cpu] = (socket, core) if socket and core else ("unknown", str(cpu))
        for cache in (directory / "cache").glob("index*"):
            if read(cache / "level") == "3":
                shared = parse_cpu_list(read(cache / "shared_cpu_list")) & allowed
                if shared:
                    groups.add(tuple(sorted(shared)))
    source = "L3"
    if not groups:
        source = "NUMA"
        for node in (system / "node").glob("node[0-9]*"):
            shared = parse_cpu_list(read(node / "cpulist")) & allowed
            if shared:
                groups.add(tuple(sorted(shared)))
    if not groups:
        source = "allowed CPUs"
        groups.add(tuple(sorted(allowed)))

    def representatives(group):
        unique = {}
        for cpu in group:
            unique.setdefault(cores[cpu], cpu)
        return sorted(unique.values())[:3]

    group = min(groups, key=lambda g: (-len(representatives(g)), g))
    return representatives(group), {"source": source, "shared_cpus": list(group)}


def cache_neighbors(allowed, system=Path("/sys/devices/system")):
    """Report cache-sharing groups intersected with the inherited affinity."""
    groups = set()
    for cpu in sorted(allowed):
        for cache in (system / "cpu" / f"cpu{cpu}" / "cache").glob("index*"):
            try:
                level = int((cache / "level").read_text())
                kind = (cache / "type").read_text().strip()
                cpus = parse_cpu_list((cache / "shared_cpu_list").read_text()) & allowed
            except (OSError, ValueError):
                continue
            if cpus:
                groups.add((level, kind, tuple(sorted(cpus))))
    return [{"level": level, "type": kind, "cpus": list(cpus)}
            for level, kind, cpus in sorted(groups)]


def pin_cpus(disabled):
    allowed = os.sched_getaffinity(0)
    selected, topology = (sorted(allowed), {"source": "disabled"}) if disabled else select_cpus(allowed)
    if not disabled:
        os.sched_setaffinity(0, selected)
    caches = cache_neighbors(allowed)
    log(f"Inherited CPU affinity: {sorted(allowed)}")
    log(f"Benchmark CPU affinity: {selected} ({topology['source']})")
    log("Cache neighbors within inherited affinity:")
    for cache in caches:
        used = sorted(set(cache["cpus"]) & set(selected))
        log(f"  L{cache['level']} {cache['type']}: {cache['cpus']}; benchmark CPUs: {used}")
    if not caches:
        log("  Cache topology unavailable")
    return {"enabled": not disabled, "allowed_cpus": sorted(allowed),
            "selected_cpus": selected, "cache_neighbors": caches, **topology}


def benchmark_cases(scratch, gtkwave=False):
    vcd, fst = scratch / "input.vcd", scratch / "input.fst"
    cases = [
        ("wavediff-fst", "wavediff", fst, scratch / "identical.fst", None),
        ("wavediff-vcd", "wavediff", vcd, scratch / "identical.vcd", None),
        ("wavediff-vcd-fst", "wavediff", vcd, fst, None),
        ("wavecat-fst-to-vcd", "wavecat", fst, scratch / "output.vcd", None),
        ("wavecat-vcd-to-fst", "wavecat", vcd, scratch / "output.fst", None),
    ]
    if gtkwave:
        cases += [
            ("wavecat-vs-fst2vcd", "wavecat", fst, scratch / "output.vcd", "fst2vcd"),
            ("wavecat-vs-vcd2fst", "wavecat", vcd, scratch / "output.fst", "vcd2fst"),
        ]
    return cases


def shard_cases(cases, num_shards, shard):
    if not 1 <= num_shards <= len(cases):
        raise ValueError(f"--num-shards must be between 1 and {len(cases)}")
    if not 0 <= shard < num_shards:
        raise ValueError("--shard must be between 0 and --num-shards - 1")
    return cases[shard::num_shards]


def run_benchmarks(args, baseline_spec):
    if platform.system() != "Linux" or not Path("/usr/bin/time").is_file():
        raise RuntimeError("This harness requires Linux and GNU /usr/bin/time")
    args.work_dir.mkdir(parents=True, exist_ok=True)
    report_path = args.work_dir / "results.json"
    report_path.unlink(missing_ok=True)
    baseline = prepare_baseline(baseline_spec)
    if not args.skip_build and not args.baseline_self_check and not args.current_bin:
        subprocess.run(["cargo", "build", "--release", "--locked", "--bins",
                        "--target-dir", str(ROOT / "target")], cwd=ROOT, check=True)
    current = baseline if args.baseline_self_check else (args.current_bin or ROOT / "target" / "release").resolve()
    selected_cases = shard_cases(benchmark_cases(Path("."), args.gtkwave), args.num_shards, args.shard)
    converters = {}
    if args.gtkwave:
        for tool in sorted({case[4] for case in selected_cases if case[4]}):
            converters[tool] = shutil.which(tool)
            if not converters[tool]:
                raise RuntimeError(f"--gtkwave requires {tool} on PATH")
    for tool in TOOLS:
        if binary_version(current / tool) is None:
            raise RuntimeError(f"Missing release binary: {current / tool}")
    pinning = pin_cpus(args.no_pin)
    report = {
        "baseline": baseline_spec,
        "baseline_version": binary_version(baseline / "wavecat"),
        "baseline_self_check": args.baseline_self_check,
        "current_bin": str(current),
        "current_versions": {t: binary_version(current / t) for t in TOOLS},
        "current_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "current_dirty": bool(subprocess.check_output(
            ["git", "status", "--porcelain"], cwd=ROOT, text=True)),
        "binary_sha256": {
            label: {tool: hashlib.sha256((directory / tool).read_bytes()).hexdigest()
                    for tool in TOOLS}
            for label, directory in (("baseline", baseline), ("current", current))
        },
        "platform": platform.platform(), "runs": args.runs,
        "cpu_affinity": sorted(os.sched_getaffinity(0)),
        "pinning": pinning,
        "num_shards": args.num_shards, "shard": args.shard,
        "selected_cases": [case[0] for case in selected_cases],
        "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
        "size_mib": args.size_mib, "signals": args.signals, "seed": args.seed,
        "timeout_seconds": args.timeout, "regression_limit_percent": MAX_INCREASE * 100,
        "converters": converters,
        "cases": {},
    }
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    with tempfile.TemporaryDirectory(prefix="workload-", dir=args.work_dir) as scratch:
        scratch = Path(scratch)
        vcd, fst = scratch / "input.vcd", scratch / "input.fst"
        log(f"Generating {args.size_mib} MiB VCD ({args.signals} signals)")
        generate_vcd(vcd, args.size_mib * 2**20, args.signals, args.seed)
        # Use the fixed baseline, not the code under test, to prepare FST input.
        execute([baseline / "wavecat", vcd, "--out", fst],
                args.work_dir / "prepare.stderr", args.timeout)
        report["input_bytes"] = {"vcd": vcd.stat().st_size, "fst": fst.stat().st_size}
        log(f"Input sizes: VCD {vcd.stat().st_size / 2**20:.1f} MiB; FST {fst.stat().st_size / 2**20:.1f} MiB")
        cases = shard_cases(benchmark_cases(scratch, args.gtkwave), args.num_shards, args.shard)
        for _, tool, source, destination, _ in cases:
            if tool == "wavediff" and destination.name.startswith("identical"):
                shutil.copyfile(source, destination)
        for name, tool, source, destination, converter in cases:
            samples = {"baseline": [], "current": []}
            for iteration in range(args.runs + 1):
                # One warm-up per binary, then alternate ordering to limit drift.
                order = ("baseline", "current") if iteration % 2 == 0 else ("current", "baseline")
                for label in order:
                    binary = (baseline if label == "baseline" else current) / tool
                    if tool == "wavecat":
                        destination.unlink(missing_ok=True)
                        command = [binary, source, "--out", destination]
                        if label == "baseline" and converter == "fst2vcd":
                            command = [converters[converter], "-e", source, "-o", destination]
                        elif label == "baseline" and converter == "vcd2fst":
                            command = [converters[converter], source, destination]
                    else:
                        command = [binary, source, destination]
                    sample = execute(command, args.work_dir / f"{name}-{label}.stderr",
                                     args.timeout, args.work_dir / "time.txt")
                    if tool == "wavecat":
                        sample["output_bytes"] = destination.stat().st_size
                    if iteration:
                        samples[label].append(sample)
                    elif tool == "wavecat":
                        # Untimed correctness check prevents fast, incomplete output from winning.
                        execute([baseline / "wavediff", source, destination],
                                args.work_dir / f"{name}-{label}-validation.stderr", args.timeout,
                                capture_stdout=True)
                    if tool == "wavecat":
                        destination.unlink()
                log(f"{name}: {'warm-up' if iteration == 0 else f'{iteration}/{args.runs}'}")
            metrics = compare(samples)
            report["cases"][name] = {"samples": samples, "metrics": metrics, "gated": converter is None, "reference": converter or "baseline"}
            report_path.write_text(json.dumps(report, indent=2) + "\n")
            for metric, values in metrics.items():
                log(f"{name} {metric}: baseline={values['baseline_mean']:.3f} current={values['current_mean']:.3f} ({values['growth_percent']:+.2f}%) {'INFO' if converter else 'FAIL' if values['failed'] else 'PASS'}")
    failed = any(m["failed"] for c in report["cases"].values() if c.get("gated", True) for m in c["metrics"].values())
    report["failed"] = failed
    report_path.write_text(json.dumps(report, indent=2) + "\n")
    print_summary(report)
    log(f"Report: {report_path}")
    return 1 if failed else 0


def positive_int(value):
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return number


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runs", type=positive_int, default=10)
    parser.add_argument("--size-mib", type=positive_int, default=256, help="approximate VCD size (default: 256 MiB)")
    parser.add_argument("--signals", type=positive_int, default=8192)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--timeout", type=positive_int, default=600, help="seconds per command")
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--baseline-ref", help="Git baseline revision (default: HEAD^)")
    selection.add_argument("--baseline-version", help="published version, or 'latest', instead of a Git baseline")
    parser.add_argument("--print-baseline-id", action="store_true", help="resolve baseline without building")
    parser.add_argument("--print-baseline-version", action="store_true", help="resolve the version without installing or benchmarking")
    parser.add_argument("--baseline-self-check", action="store_true", help="compare the cached baseline against itself; do not build the checkout")
    parser.add_argument("--gtkwave", action="store_true", help="also measure against fst2vcd and vcd2fst (informational)")
    parser.add_argument("--current-bin", type=Path, help="use separately built release binaries; skip current build")
    parser.add_argument("--skip-build", action="store_true", help="use already built target/release binaries")
    parser.add_argument("--work-dir", type=Path, default=ROOT / "target/performance")
    parser.add_argument("--no-pin", action="store_true", help="keep inherited CPU affinity")
    parser.add_argument("--num-shards", type=positive_int, default=1, help="number of case shards")
    parser.add_argument("--shard", type=int, default=0, help="zero-based shard index")
    parser.add_argument("--print-shard-matrix", action="store_true", help="print CI matrix with one case per shard")
    args = parser.parse_args()
    original_affinity = None
    try:
        cases = benchmark_cases(Path("."), args.gtkwave)
        if args.print_shard_matrix:
            print(json.dumps({"include": [
                {"shard": index, "num_shards": len(cases), "case": case[0]}
                for index, case in enumerate(cases)]}))
            return 0
        shard_cases(cases, args.num_shards, args.shard)
        if args.num_shards > 1:
            args.work_dir = args.work_dir / f"shard-{args.shard}"
        spec = resolve_baseline(args)
        if args.print_baseline_version and spec["kind"] != "release":
            raise ValueError("--print-baseline-version requires --baseline-version VERSION or latest")
        if args.print_baseline_id or args.print_baseline_version:
            print(spec.get("revision", spec.get("version")))
            return 0
        original_affinity = os.sched_getaffinity(0) if platform.system() == "Linux" else None
        return run_benchmarks(args, spec)
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"Performance harness error: {error}", file=sys.stderr)
        return 2
    finally:
        if original_affinity is not None:
            os.sched_setaffinity(0, original_affinity)


if __name__ == "__main__":
    sys.exit(main())
