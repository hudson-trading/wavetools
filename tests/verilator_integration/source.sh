#!/usr/bin/env bash
# SPDX-FileCopyrightText: Hudson River Trading
# SPDX-License-Identifier: MIT

# A binary or .git directory alone does not establish a usable source tree.
verilator_source_complete() {
    local directory="$1" file
    for file in configure.ac Makefile.in bin/verilator src/Makefile_obj.in \
                python-dev-requirements.txt test_regress/driver.py; do
        [[ -f "$directory/$file" ]] || return 1
    done
}

verilator_at_tag() {
    local directory="$1" tag="$2" wanted actual
    [[ -e "$directory/.git" ]] || return 1
    wanted="$(git -C "$directory" rev-parse --verify "refs/tags/$tag^{commit}" 2>/dev/null)" || return 1
    actual="$(git -C "$directory" rev-parse --verify HEAD 2>/dev/null)" || return 1
    [[ "$actual" == "$wanted" ]]
}

# Only call this for the runner-owned checkout, never a supplied VERILATOR_DIR.
prepare_verilator_source() {
    local directory="$1" tag="$2"
    local repository="${3:-https://github.com/verilator/verilator.git}"
    if verilator_source_complete "$directory" && verilator_at_tag "$directory" "$tag" && \
       git -C "$directory" diff --quiet HEAD --; then
        return
    fi
    printf '==> Recreating managed Verilator checkout at %s (%s)\n' "$directory" "$tag" >&2
    # Clone before replacing the cache so a failed download preserves it.
    local staging
    staging="$(mktemp -d "${directory}.clone-XXXXXX")" || return 1
    if ! git clone --depth=1 --branch "$tag" "$repository" "$staging"; then
        rm -rf -- "$staging"
        return 1
    fi
    if ! verilator_source_complete "$staging" || ! verilator_at_tag "$staging" "$tag"; then
        printf 'error: incomplete Verilator source for %s\n' "$tag" >&2
        rm -rf -- "$staging"
        return 1
    fi
    rm -rf -- "$directory"
    mv -- "$staging" "$directory"
}
