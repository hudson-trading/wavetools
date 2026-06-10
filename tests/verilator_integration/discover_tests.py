#!/usr/bin/env python3
"""Discover Verilator test_regress entry-points that call trace_identical().

Verilator's trace_identical() helper (driver.py) dispatches to vcd_identical /
fst_identical, both of which shell out to the wavediff binary specified by the
WAVEDIFF environment variable.
"""

import argparse
import re
import sys
from pathlib import Path

TRACE_IDENTICAL_RE = re.compile(r"\btrace_identical\(")
IMPORT_RE = re.compile(r"^\s*(?:from\s+(\S+)\s+import|import\s+(\S+))", re.MULTILINE)
BOOTSTRAP_RE = re.compile(r"^\s*import\s+vltest_bootstrap\b", re.MULTILINE)


def discover(t_dir: Path) -> list[Path]:
    sources: dict[str, tuple[Path, str]] = {}
    for path in sorted(t_dir.glob("*.py")):
        sources[path.stem] = (path, path.read_text(encoding="utf-8"))

    tainted = {
        name for name, (_, text) in sources.items() if TRACE_IDENTICAL_RE.search(text)
    }

    changed = True
    while changed:
        changed = False
        for name, (_, text) in sources.items():
            if name in tainted:
                continue
            for from_mod, plain_mod in IMPORT_RE.findall(text):
                if (from_mod or plain_mod) in tainted:
                    tainted.add(name)
                    changed = True
                    break

    return [
        path
        for name, (path, text) in sorted(sources.items())
        if name in tainted and BOOTSTRAP_RE.search(text)
    ]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "test_regress_dir", type=Path, help="Path to verilator's test_regress directory"
    )
    args = parser.parse_args()

    t_dir = args.test_regress_dir / "t"
    if not t_dir.is_dir():
        print(f"error: not a directory: {t_dir}", file=sys.stderr)
        return 2

    tests = discover(t_dir)
    if not tests:
        print(
            "error: no tests discovered (looked for trace_identical callers)",
            file=sys.stderr,
        )
        return 1
    for test in tests:
        print(test)
    return 0


if __name__ == "__main__":
    sys.exit(main())
