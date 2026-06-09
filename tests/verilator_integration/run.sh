#!/usr/bin/env bash
# Run Verilator's transitive trace_identical integration tests against this
# repo's wavediff binary. Mirrors what the verilator-integration GitHub Actions
# workflow does so the same suite can be reproduced locally.
#
# Environment variables (all optional):
#   WAVEDIFF        Path to wavediff binary (default: build from this repo)
#   VERILATOR_DIR   Pre-existing verilator source dir (skip clone+build if its
#                   bin/verilator_bin already exists)
#   VERILATOR_TAG   Release tag to clone (default: latest v* tag on origin)
#   WORK_DIR        Scratch dir for clone/build/venv (default: target/verilator-integration)
#   JOBS            Driver parallelism (default: 0 = all cores)
#   SCENARIO        Driver scenario flag (default: --vlt)
#   PYTHON3         Python 3.10+ interpreter to use for the venv (default: probe
#                   python3.13, python3.12, python3.11, python3.10, python3 in
#                   that order). Verilator's driver.py uses match-case (3.10+),
#                   and its python-dev-requirements pin packages that require
#                   3.10+, so older interpreters will fail at pip-install time.
#
# Modes:
#   --print-latest-tag   Print the latest verilator v* tag and exit. Used by the
#                        GitHub Actions workflow to key its build caches before
#                        invoking the runner proper.

set -euo pipefail

log() { printf '==> %s\n' "$*" >&2; }

resolve_latest_tag() {
    local tag
    tag="$(git ls-remote --tags --refs --sort=-v:refname \
        https://github.com/verilator/verilator.git 'refs/tags/v*' \
        | head -n1 | sed -E 's@.*refs/tags/@@')"
    if [[ -z "$tag" ]]; then
        echo "error: could not determine latest verilator tag" >&2
        return 1
    fi
    printf '%s\n' "$tag"
}

if [[ "${1:-}" == "--print-latest-tag" ]]; then
    resolve_latest_tag
    exit
fi

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"

WAVEDIFF="${WAVEDIFF:-$REPO_ROOT/target/release/wavediff}"
VERILATOR_DIR="${VERILATOR_DIR:-}"
VERILATOR_TAG="${VERILATOR_TAG:-}"
WORK_DIR="${WORK_DIR:-$REPO_ROOT/target/verilator-integration}"
JOBS="${JOBS:-0}"
SCENARIO="${SCENARIO:---vlt}"

# Pick a Python >= 3.10. Verilator's driver.py needs match-case syntax (3.10+),
# and its python-dev-requirements pin packages with the same floor.
pick_python() {
    local p
    if [[ -n "${PYTHON3:-}" ]]; then
        if "$PYTHON3" -c 'import sys; sys.exit(0 if sys.version_info >= (3,10) else 1)' 2>/dev/null; then
            printf '%s\n' "$PYTHON3"
            return
        fi
        echo "error: PYTHON3=$PYTHON3 is not Python >= 3.10" >&2
        return 1
    fi
    for p in python3.13 python3.12 python3.11 python3.10 python3; do
        if command -v "$p" >/dev/null 2>&1 && \
           "$p" -c 'import sys; sys.exit(0 if sys.version_info >= (3,10) else 1)' 2>/dev/null; then
            printf '%s\n' "$p"
            return
        fi
    done
    echo "error: need Python >= 3.10 on PATH (verilator's driver.py uses match-case)" >&2
    echo "       set PYTHON3=/path/to/python3.1x and re-run" >&2
    return 1
}
PYTHON3="$(pick_python)"
log "Using $PYTHON3 ($("$PYTHON3" --version 2>&1))"

mkdir -p "$WORK_DIR"

# ---------------------------------------------------------------------------
# 1. Build wavediff if missing.
# ---------------------------------------------------------------------------
if [[ ! -x "$WAVEDIFF" ]]; then
    log "Building wavediff (cargo build --release --bin wavediff)"
    (cd "$REPO_ROOT" && cargo build --release --bin wavediff)
fi
WAVEDIFF="$(readlink -f "$WAVEDIFF")"
log "Using wavediff: $WAVEDIFF"

# ---------------------------------------------------------------------------
# 2. Clone the latest Verilator release tag (unless caller supplied a dir).
# ---------------------------------------------------------------------------
if [[ -z "$VERILATOR_DIR" ]]; then
    VERILATOR_DIR="$WORK_DIR/verilator"
fi
VERILATOR_DIR="$(readlink -f "$VERILATOR_DIR" 2>/dev/null || echo "$VERILATOR_DIR")"

if [[ ! -d "$VERILATOR_DIR/.git" ]] && [[ ! -x "$VERILATOR_DIR/bin/verilator_bin" ]]; then
    if [[ -z "$VERILATOR_TAG" ]]; then
        log "Resolving latest Verilator release tag"
        VERILATOR_TAG="$(resolve_latest_tag)"
    fi
    log "Cloning verilator@$VERILATOR_TAG into $VERILATOR_DIR"
    git clone --depth=1 --branch "$VERILATOR_TAG" \
        https://github.com/verilator/verilator.git "$VERILATOR_DIR"
fi

# ---------------------------------------------------------------------------
# 3. Build verilator.
# ---------------------------------------------------------------------------
if [[ ! -x "$VERILATOR_DIR/bin/verilator_bin" ]]; then
    log "Building verilator in $VERILATOR_DIR"
    (
        cd "$VERILATOR_DIR"
        autoconf
        ./configure
        make -j "$(nproc)"
    )
fi
export VERILATOR_ROOT="$VERILATOR_DIR"
export PATH="$VERILATOR_DIR/bin:$PATH"

# ---------------------------------------------------------------------------
# 4. Set up a venv with verilator's python test dependencies.
# ---------------------------------------------------------------------------
VENV_DIR="$WORK_DIR/venv"
# Discard an existing venv if it was built with too-old a Python.
if [[ -x "$VENV_DIR/bin/python3" ]] && \
   ! "$VENV_DIR/bin/python3" -c 'import sys; sys.exit(0 if sys.version_info >= (3,10) else 1)' 2>/dev/null; then
    log "Existing venv uses Python < 3.10; recreating with $PYTHON3"
    rm -rf "$VENV_DIR"
fi
if [[ ! -x "$VENV_DIR/bin/python3" ]]; then
    log "Creating venv at $VENV_DIR with $PYTHON3"
    "$PYTHON3" -m venv "$VENV_DIR"
fi
# shellcheck disable=SC1091
source "$VENV_DIR/bin/activate"
log "Installing verilator python-dev-requirements"
pip install --quiet --upgrade pip
pip install --quiet -r "$VERILATOR_DIR/python-dev-requirements.txt"

# ---------------------------------------------------------------------------
# 5. Discover trace_identical tests.
# ---------------------------------------------------------------------------
log "Discovering trace_identical tests"
mapfile -t TESTS < <(python3 "$SCRIPT_DIR/discover_tests.py" "$VERILATOR_DIR/test_regress")
log "Found ${#TESTS[@]} tests"

# Driver expects test paths relative to its CWD (test_regress/).
TESTS_REL=()
for t in "${TESTS[@]}"; do
    TESTS_REL+=("${t#"$VERILATOR_DIR/test_regress/"}")
done

# ---------------------------------------------------------------------------
# 6. Run them with WAVEDIFF pointed at our binary.
# ---------------------------------------------------------------------------
export WAVEDIFF
log "Running driver.py $SCENARIO -j $JOBS (WAVEDIFF=$WAVEDIFF)"
cd "$VERILATOR_DIR/test_regress"
exec python3 driver.py "$SCENARIO" -j "$JOBS" "${TESTS_REL[@]}"
