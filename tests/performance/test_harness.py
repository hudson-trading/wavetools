# SPDX-FileCopyrightText: Hudson River Trading
# SPDX-License-Identifier: MIT

import argparse
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("harness", Path(__file__).with_name("run.py"))
harness = importlib.util.module_from_spec(spec)
spec.loader.exec_module(harness)


# Synthetic test versions, independent of the repository/package version.
RELEASE = "1.2.3"
OLDER_RELEASE = "1.2.2"


class HarnessTests(unittest.TestCase):
    def test_validation_failure_retains_stdout_and_stderr(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / "validation.stderr"
            with self.assertRaises(RuntimeError) as error:
                harness.execute(["/bin/sh", "-c",
                                 "echo '0 top.a 0 != 1'; echo diagnostic >&2; exit 1"],
                                log, 5, capture_stdout=True)
            for message in ("0 top.a 0 != 1", "diagnostic"):
                self.assertIn(message, log.read_text())
                self.assertIn(message, str(error.exception))

    def test_current_build_directory_and_report_threshold(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binaries = root / "target" / "release"
            binaries.mkdir(parents=True)
            for tool in harness.TOOLS:
                (binaries / tool).write_bytes(b"binary")
            args = argparse.Namespace(work_dir=root / "results", skip_build=False,
                baseline_self_check=False, current_bin=None, gtkwave=False,
                runs=1, size_mib=1, signals=1, seed=1, timeout=5)
            with patch.object(harness, "ROOT", root), \
                 patch.object(harness, "MAX_INCREASE", 0.125), \
                 patch.dict(harness.os.environ, {"CARGO_TARGET_DIR": str(root / "elsewhere")}), \
                 patch.object(harness, "prepare_baseline", return_value=binaries), \
                 patch.object(harness, "binary_version", return_value=RELEASE), \
                 patch.object(harness.subprocess, "check_output", return_value="metadata"), \
                 patch.object(harness.subprocess, "run") as build, \
                 patch.object(harness, "generate_vcd", side_effect=RuntimeError("stop after setup")):
                with self.assertRaisesRegex(RuntimeError, "stop after setup"):
                    harness.run_benchmarks(args, {"kind": "commit", "revision": "abc123"})
            command = build.call_args.args[0]
            self.assertEqual(command[command.index("--target-dir") + 1], str(root / "target"))
            report = json.loads((args.work_dir / "results.json").read_text())
            self.assertEqual(report["current_bin"], str(binaries))
            self.assertEqual(report["regression_limit_percent"], 12.5)

    def test_threshold_is_per_metric_and_strictly_over_five_percent(self):
        samples = {
            "baseline": [{"seconds": 100, "peak_rss_kib": 100}] * 2,
            "current": [{"seconds": 104, "peak_rss_kib": 105},
                        {"seconds": 106, "peak_rss_kib": 105}],
        }
        self.assertFalse(any(m["failed"] for m in harness.compare(samples).values()))
        samples["current"][1] = {"seconds": 107, "peak_rss_kib": 103}
        metrics = harness.compare(samples)
        self.assertTrue(metrics["seconds"]["failed"])
        self.assertFalse(metrics["peak_rss_kib"]["failed"])
        samples["current"] = [{"seconds": 90, "peak_rss_kib": 106}]
        self.assertTrue(harness.compare(samples)["peak_rss_kib"]["failed"])

    def test_summary_explains_signs_and_reports_memory_regressions(self):
        report = {"cases": {}}
        for name, seconds, rss in [("faster", 90, 1024), ("more-memory", 90, 2048)]:
            metrics = harness.compare({
                "baseline": [{"seconds": 100, "peak_rss_kib": 1024}],
                "current": [{"seconds": seconds, "peak_rss_kib": rss}],
            })
            report["cases"][name] = {"metrics": metrics}
        with patch.object(harness, "log") as log:
            harness.print_summary(report)
        lines = [call.args[0] for call in log.call_args_list]
        self.assertTrue(any("negative (-) is better" in line and
                            "positive (+) is worse" in line for line in lines))
        faster = next(line for line in lines if line.startswith("faster "))
        self.assertIn("100.000 -> 90.000", faster)
        self.assertIn("-10.00%", faster)
        self.assertIn("PASS", faster)
        memory = next(line for line in lines if line.startswith("more-memory "))
        self.assertIn("1.0 -> 2.0", memory)
        self.assertIn("+100.00%", memory)
        self.assertIn("FAIL", memory)

    def test_default_baseline_resolves_parent_commit(self):
        args = argparse.Namespace(baseline_version=None, baseline_ref=None)
        with patch.object(harness.subprocess, "check_output", return_value="abc123\n") as git:
            self.assertEqual(harness.resolve_baseline(args),
                             {"kind": "commit", "revision": "abc123"})
        self.assertEqual(git.call_args.args[0][-1], "HEAD^^{commit}")

    def test_explicit_git_and_release_baselines(self):
        args = argparse.Namespace(baseline_version=None, baseline_ref="HEAD~3")
        with patch.object(harness.subprocess, "check_output", return_value="abc123\n") as git:
            harness.resolve_baseline(args)
        self.assertEqual(git.call_args.args[0][-1], "HEAD~3^{commit}")
        args.baseline_version = RELEASE
        with patch.object(harness.subprocess, "check_output") as git:
            self.assertEqual(harness.resolve_baseline(args),
                             {"kind": "release", "version": RELEASE})
            git.assert_not_called()

    def test_git_baseline_build_and_cache(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def build(command, **kwargs):
                if command[0:3] == ["git", "worktree", "add"]:
                    Path(command[-2]).mkdir(parents=True)
                elif command[0] == "cargo":
                    target = Path(command[-1]) / "release"
                    target.mkdir(parents=True, exist_ok=True)
                    for tool in harness.TOOLS:
                        (target / tool).write_text("binary")
            with patch.object(harness, "BASELINE", root / "baseline"), \
                 patch.object(harness, "ROOT", root), \
                 patch.object(harness.subprocess, "check_output",
                              side_effect=lambda command, **kwargs: "compiler" if command[0] == "rustc" else "abc123" if command[1] == "rev-parse" else ""), \
                 patch.object(harness.subprocess, "run", side_effect=build) as run, \
                 patch.object(harness, "binary_version", return_value=RELEASE):
                spec = {"kind": "commit", "revision": "abc123"}
                path = harness.prepare_baseline(spec)
                self.assertTrue((path / "wavecat").is_file())
                self.assertFalse((path.parent / "source").exists())
                worktree = next(c.args[0][-2] for c in run.call_args_list
                                if c.args[0][:3] == ["git", "worktree", "add"])
                self.assertTrue(Path(worktree).is_relative_to(root / "target"))
                self.assertTrue(any(c.args[0][0] == "cargo" for c in run.call_args_list))
                run.reset_mock()
                self.assertEqual(harness.prepare_baseline(spec), path)
                run.assert_not_called()
                self.assertEqual(harness.prepare_baseline(
                    {"kind": "commit", "revision": "def456"}), path)
                self.assertTrue(run.called)
                (path.parent / "build.json").write_text(
                    '{"kind": "release", "version": "1.2.3"}')
                run.reset_mock()
                self.assertEqual(harness.prepare_baseline(spec), path)
                self.assertTrue(run.called)

    def test_failed_git_build_removes_temporary_worktree_without_caching(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def build(command, **kwargs):
                if command[:3] == ["git", "worktree", "add"]:
                    Path(command[-2]).mkdir(parents=True)
                elif command[0] == "cargo":
                    raise harness.subprocess.CalledProcessError(1, command)
            with patch.object(harness, "BASELINE", root / "baseline"), \
                 patch.object(harness, "ROOT", root), \
                 patch.object(harness.subprocess, "check_output", return_value="compiler"), \
                 patch.object(harness.subprocess, "run", side_effect=build) as run:
                with self.assertRaises(harness.subprocess.CalledProcessError):
                    harness.prepare_baseline({"kind": "commit", "revision": "abc123"})
                command = run.call_args.args[0]
                self.assertEqual(command[:3], ["git", "worktree", "remove"])
                self.assertFalse(Path(command[-1]).exists())
                self.assertFalse(list(root.rglob("build.json")))

    def test_latest_version_comes_from_registry_metadata(self):
        with patch.object(harness.urllib.request, "urlopen") as request:
            response = request.return_value.__enter__.return_value
            response.read.return_value = b'{"crate": {"max_stable_version": "23.45.67"}}'
            self.assertEqual(harness.latest_version(), "23.45.67")

    def test_release_cache_and_switch_from_git(self):
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(harness, "BASELINE", Path(directory)), \
             patch.object(harness, "binary_version", return_value=RELEASE), \
             patch.object(harness.subprocess, "check_output", return_value="compiler") as compiler, \
             patch.object(harness.subprocess, "run") as install:
            marker = Path(directory) / "build.json"
            marker.write_text('{"kind": "commit", "revision": "abc123"}')
            harness.install_baseline(RELEASE)
            command = install.call_args.args[0]
            self.assertIn(f"={RELEASE}", command)
            self.assertIn(str(harness.BASELINE), command)
            self.assertIn("--locked", command)
            install.reset_mock()
            harness.install_baseline(RELEASE)
            install.assert_not_called()
            compiler.return_value = "new compiler"
            harness.install_baseline(RELEASE)
            install.assert_called_once()
            install.reset_mock()
            with patch.dict(harness.os.environ, {"RUSTFLAGS": "-C opt-level=1"}):
                harness.install_baseline(RELEASE)
            install.assert_called_once()

    def test_missing_or_wrong_binary_requires_install(self):
        for versions in [(RELEASE, None), (OLDER_RELEASE, RELEASE)]:
            with patch.object(harness, "binary_version", side_effect=versions):
                self.assertFalse(harness.baseline_ready(Path("unused"), RELEASE))

    def test_generated_workload_is_deterministic_and_large_enough(self):
        with tempfile.TemporaryDirectory() as directory:
            a, b = Path(directory) / "a.vcd", Path(directory) / "b.vcd"
            harness.generate_vcd(a, 8192, 16, 42)
            harness.generate_vcd(b, 8192, 16, 42)
            self.assertEqual(a.read_bytes(), b.read_bytes())
            self.assertGreaterEqual(a.stat().st_size, 8192)
            self.assertEqual(a.read_text().count("$var wire 64"), 16)
        self.assertEqual(len({harness.id_code(i) for i in range(20000)}), 20000)

    def test_failed_command_is_not_a_performance_result(self):
        with tempfile.TemporaryDirectory() as directory:
            stderr = Path(directory) / "error"
            with self.assertRaisesRegex(RuntimeError, "Command failed"):
                harness.execute(["/bin/sh", "-c", "echo failure >&2; exit 7"], stderr, 5)
            self.assertIn("failure", stderr.read_text())

    def test_timeout_stops_the_command(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(RuntimeError, "Timed out"):
                harness.execute(["/bin/sh", "-c", "sleep 30"], Path(directory) / "error", 0.05)


if __name__ == "__main__":
    unittest.main()
