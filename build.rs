use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    // Watch HEAD (changes on branch switch / detach) and logs/HEAD (changes on
    // every commit, checkout, reset, rebase via reflog updates). Resolve via
    // `git rev-parse --git-path` so we pick the right paths under worktrees,
    // submodules, or any other non-standard gitdir layout.
    for spec in ["HEAD", "logs/HEAD"] {
        if let Some(path) = git_path(spec) {
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }

    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let rev = sha.unwrap_or_else(|| "UNKNOWN".to_string());

    println!("cargo:rustc-env=WAVETOOLS_GIT_REV={rev}");
}

fn git_path(spec: &str) -> Option<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--git-path", spec])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}
