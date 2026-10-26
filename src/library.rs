//! The AIPL library this binary uses: what aipl_src/ holds, the standard
//! library (std/) and the self-hosted toolchain's sources (driver.aipl for
//! the optimizer, native/ for `--exe`, doc.aipl for `aipl doc`), which the
//! commands read at run time (docs/LAUNCH_CHECKLIST.md, "A binary that
//! works wherever it is").
//!
//! A binary run from its own checkout's target/ (one being developed)
//! reads that checkout's aipl_src/, so an edit there is seen without a
//! rebuild. Anywhere else (copied, installed, unpacked from a kit) it uses
//! the copy built into it (build.rs), written once to the module cache's
//! directory as library-HASH/ (src/selfhost.rs cache_dir; the temporary
//! directory when there is no cache). `$AIPL_STD`, when set, is the
//! standard library for the programs it compiles and runs; the toolchain's
//! own modules always use the library's.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/library.rs"));

/// The library's directory (aipl_src/, or the built-in copy written out).
pub fn dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| checkout().unwrap_or_else(|| unpack().unwrap_or_else(|e| panic!("the AIPL library: {e}"))))
}

/// The standard library for programs: `$AIPL_STD` when set, else the
/// library's std/.
pub fn std_dir() -> PathBuf {
    match std::env::var_os("AIPL_STD").filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => dir().join("std"),
    }
}

/// Whether `path` is one of the library's own files (the toolchain's
/// modules), which take their imports from the library's std/.
pub fn contains(path: &Path) -> bool {
    match (path.canonicalize(), dir().canonicalize()) {
        (Ok(p), Ok(d)) => p.starts_with(d),
        _ => false,
    }
}

/// The checkout's aipl_src/, when this binary runs from the checkout's target/.
fn checkout() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = root.join("aipl_src");
    let target = root.join("target").canonicalize().ok()?;
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    (exe.starts_with(&target) && src.is_dir()).then_some(src)
}

/// The built-in copy, written out once (whole or not at all: into a
/// temporary directory, then renamed) and used from there.
fn unpack() -> Result<PathBuf, String> {
    let base = crate::selfhost::cache_dir().unwrap_or_else(std::env::temp_dir);
    let dir = base.join(format!("library-{HASH}"));
    if dir.join(".complete").exists() {
        return Ok(dir);
    }
    let tmp = base.join(format!(".library-{HASH}.{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    for (rel, bytes) in FILES {
        let file = tmp.join(rel);
        std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
        std::fs::write(&file, bytes).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
    }
    std::fs::write(tmp.join(".complete"), HASH).map_err(|e| e.to_string())?;
    if std::fs::rename(&tmp, &dir).is_err() {
        // another process wrote it first
        let _ = std::fs::remove_dir_all(&tmp);
        if !dir.join(".complete").exists() {
            return Err(format!("cannot write {}", dir.display()));
        }
    }
    Ok(dir)
}
