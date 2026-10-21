// Records the commit the toolchain is built from as AIPL_COMMIT, so
// `aipl --version` reads `aipl 0.1.0 (5e29a53)` and a copy of the docs
// stamped with the same line shows when it is stale.
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn main() {
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=AIPL_COMMIT={commit}");
    // A new commit moves the branch's ref (or HEAD itself, on a checkout).
    if let Some(head) = git(&["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head}");
    }
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git(&["rev-parse", "--git-path", &branch]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    if let Some(packed) = git(&["rev-parse", "--git-path", "packed-refs"]) {
        println!("cargo:rerun-if-changed={packed}");
    }
}
