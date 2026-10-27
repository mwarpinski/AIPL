//! The optimizer as `aipl compile` runs it by default (docs/design/OPT_PLAN.md):
//! a trap in an inlined function reported at the call, `--no-opt` giving the
//! frame back, natively as under aipl-run, and the module cache
//! (src/selfhost.rs) surviving a damaged entry and being turned off.
//! tools/opt_diff.py compares optimized and unoptimized builds of every
//! program in the repository and of generated ones.

use std::path::{Path, PathBuf};
use std::process::Command;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_opt_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn compile(args: &[&str], src: &Path, out: &Path, cache: Option<&Path>) {
    let mut c = Command::new(AIPL);
    c.arg("compile").args(args).arg(src).arg("-o").arg(out);
    if let Some(dir) = cache {
        c.env("AIPL_CACHE", dir);
    }
    let o = c.output().unwrap();
    assert!(o.status.success(), "compile {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

/// The trap's message and call chain, the file name before it left out.
fn trap(cmd: &mut Command) -> String {
    let o = cmd.output().unwrap();
    #[cfg(unix)]
    let sig = std::os::unix::process::ExitStatusExt::signal(&o.status);
    #[cfg(not(unix))]
    let sig: Option<i32> = None;
    assert_eq!(
        o.status.code(),
        Some(134),
        "command {:?}: expected status 134, got {:?} (signal: {:?}), stdout: {:?}, stderr: {:?}",
        cmd,
        o.status,
        sig,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    err.split_once(": ").unwrap().1.to_string()
}

/// tests/aipl/opt/inline_trap.aipl: `at` is inlined into `main`, so its trap
/// is reported at the call (line 13), in main's frame, never at `at`'s line
/// under main's name; `--no-opt` keeps `at`'s frame. Alike under aipl-run and
/// natively.
#[test]
fn an_inlined_function_traps_at_its_call() {
    let dir = scratch("inline");
    let src = root().join("tests/aipl/opt/inline_trap.aipl");
    let msg = "Array index out of bounds: index 12 for array of length 3\n";
    let optimized = format!("{msg}  at main (inline_trap.aipl:13:5)\n");
    let plain = format!("{msg}  at at (inline_trap.aipl:9:5)\n  at main (inline_trap.aipl:13:5)\n");
    for (args, want) in [(vec![], &optimized), (vec!["--no-opt"], &plain)] {
        let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
        compile(&args, &src, &wasm, None);
        compile(&[args.as_slice(), &["--exe"]].concat(), &src, &exe, None);
        assert_eq!(&trap(Command::new(RUNNER).arg(&wasm)), want, "aipl-run {args:?}");
        assert_eq!(&trap(&mut Command::new(&exe)), want, "native {args:?}");
    }
}

/// A cache entry that is not a module wasmtime made is compiled again and
/// replaced, giving the same bytes; an empty AIPL_CACHE turns the cache off.
#[test]
fn a_damaged_module_cache_is_compiled_again() {
    let dir = scratch("cache");
    let cache = dir.join("cache");
    let src = root().join("examples/word_count.aipl");
    let (first, second, uncached) = (dir.join("a.wasm"), dir.join("b.wasm"), dir.join("c.wasm"));
    compile(&[], &src, &first, Some(&cache));
    let entries: Vec<PathBuf> = std::fs::read_dir(&cache).unwrap().map(|e| e.unwrap().path()).collect();
    let driver: Vec<&PathBuf> = entries.iter().filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("driver-")).collect();
    assert_eq!(driver.len(), 1, "one driver module in {entries:?}");
    std::fs::write(driver[0], b"not a module").unwrap();
    compile(&[], &src, &second, Some(&cache));
    assert_eq!(std::fs::read(&first).unwrap(), std::fs::read(&second).unwrap());
    assert_ne!(std::fs::read(driver[0]).unwrap(), b"not a module", "the entry is written again");
    compile(&[], &src, &uncached, Some(Path::new("")));
    assert_eq!(std::fs::read(&first).unwrap(), std::fs::read(&uncached).unwrap());
}
