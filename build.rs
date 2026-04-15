use std::process::Command;

fn main() {
    // Re-run if git HEAD changes (covers commits, checkouts, rebases)
    println!("cargo:rerun-if-changed=.git/HEAD");

    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty());

    let rev = match sha {
        Some(s) => {
            let suffix = if dirty.unwrap_or(false) { " mod" } else { "" };
            format!("{s}{suffix}")
        }
        None => "UNKNOWN".to_string(),
    };

    println!("cargo:rustc-env=WAVETOOLS_GIT_REV={rev}");
}
