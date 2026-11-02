// Records the commit the toolchain is built from as AIPL_COMMIT, so
// `aipl --version` reads `aipl 0.1.0 (5e29a53)` and a copy of the docs
// stamped with the same line shows when it is stale; a hash of src/ as
// AIPL_SRC_HASH, part of the key of src/selfhost.rs's module cache; and
// aipl_src/ built into the binary (OUT_DIR/library.rs, src/library.rs), so
// a copied or installed aipl needs nothing beside it.
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

/// Every file below `dir`, by path.
fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn main() {
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=AIPL_COMMIT={commit}");
    // AIPL_SRC_HASH names the Rust sources the toolchain is built from: the
    // key of a compiled toolchain module, cached between runs (src/selfhost.rs),
    // changes with the code generator that compiled it
    use std::hash::{Hash, Hasher};
    let mut all = Vec::new();
    files(std::path::Path::new("src"), &mut all);
    all.sort();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in &all {
        f.hash(&mut h);
        std::fs::read(f).unwrap_or_default().hash(&mut h);
    }
    println!("cargo:rustc-env=AIPL_SRC_HASH={:016x}", h.finish());
    println!("cargo:rerun-if-changed=src");

    // aipl_src/'s .aipl files, by path below it, and a hash of them all
    let root = std::fs::canonicalize("aipl_src").expect("aipl_src");
    let mut lib = Vec::new();
    files(&root, &mut lib);
    lib.retain(|p| p.extension().is_some_and(|x| x == "aipl"));
    lib.sort();
    let mut lh = std::collections::hash_map::DefaultHasher::new();
    let mut table = String::from("/// aipl_src/'s files, by path below it (build.rs).\npub static FILES: &[(&str, &[u8])] = &[\n");
    for f in &lib {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        rel.hash(&mut lh);
        std::fs::read(f).unwrap().hash(&mut lh);
        table.push_str(&format!("    ({rel:?}, include_bytes!({:?})),\n", f.to_string_lossy()));
    }
    table.push_str("];\n");
    table.push_str(&format!("/// A hash of FILES, naming the copy written to the cache.\npub const HASH: &str = \"{:016x}\";\n", lh.finish()));
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("library.rs");
    std::fs::write(out, table).unwrap();
    println!("cargo:rerun-if-changed=aipl_src");
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
