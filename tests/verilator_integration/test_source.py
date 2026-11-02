# SPDX-FileCopyrightText: Hudson River Trading
# SPDX-License-Identifier: MIT

from pathlib import Path
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).with_name("source.sh").resolve()
REQUIRED = ("configure.ac", "Makefile.in", "bin/verilator", "src/Makefile_obj.in",
            "python-dev-requirements.txt", "test_regress/driver.py")


class VerilatorSourceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repository = self.root / "origin"
        self.repository.mkdir()
        self.git("init", "-q")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.com")
        for name in REQUIRED:
            path = self.repository / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("AC_INIT([fixture], [1.0])\n" if name == "configure.ac" else "fixture\n")
        self.git("add", ".")
        self.git("-c", "commit.gpgsign=false", "commit", "-qm", "first")
        self.git("tag", "v1")
        (self.repository / "Makefile.in").write_text("second release\n")
        self.git("-c", "commit.gpgsign=false", "commit", "-qam", "second")
        self.git("tag", "v2")
        self.checkout = self.root / "cache"

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.repository), *args], text=True)

    def prepare(self, tag="v1"):
        return subprocess.run(
            ["bash", "-euc", 'source "$1"; prepare_verilator_source "$2" "$3" "$4"',
             "test", str(SOURCE), str(self.checkout), tag, self.repository.as_uri()],
            capture_output=True, text=True, timeout=20,
        )

    def test_partial_git_cache_is_replaced_and_reusable(self):
        (self.checkout / ".git").mkdir(parents=True)
        result = self.prepare()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.checkout / "configure.ac").is_file())
        result = self.prepare()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("Cloning", result.stderr)

    def test_deleted_sources_in_real_checkout_force_reclone(self):
        self.assertEqual(self.prepare().returncode, 0)
        (self.checkout / "configure.ac").unlink()
        binary = self.checkout / "bin/verilator_bin"
        binary.write_text("old binary")
        binary.chmod(0o755)
        result = self.prepare()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.checkout / "configure.ac").is_file())
        self.assertFalse(binary.exists())

    def test_tag_change_discards_old_build_and_checks_out_requested_release(self):
        self.assertEqual(self.prepare().returncode, 0)
        binary = self.checkout / "bin/verilator_bin"
        binary.write_text("old binary")
        result = self.prepare("v2")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.checkout / "Makefile.in").read_text(), "second release\n")
        self.assertFalse(binary.exists())

    def test_failed_clone_preserves_existing_cache(self):
        self.assertEqual(self.prepare().returncode, 0)
        original = (self.checkout / "Makefile.in").read_bytes()
        result = self.prepare("missing-tag")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.checkout / "Makefile.in").read_bytes(), original)
        self.assertEqual(list(self.root.glob("cache.clone-*")), [])

    def test_worktree_git_file_is_supported(self):
        self.git("worktree", "add", "--detach", str(self.checkout), "v1")
        result = self.prepare()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.checkout / ".git").is_file())
        self.assertNotIn("Cloning", result.stderr)


if __name__ == "__main__":
    unittest.main()
