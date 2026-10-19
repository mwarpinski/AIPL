//! aipl doc (docs/design/FEEDBACK_TOOLS_PLAN.md F1, aipl_src/doc.aipl): a
//! module's public items with their signatures, qualified as a caller writes
//! them, and the comment directly above each (tests/aipl/doc/shapes.aipl
//! has every kind); one item; a search; and --check, which every standard
//! module passes, so the standard library stays documented. The self-hosted
//! aipl gives the same output (tests/test_bootstrap.rs).

use std::path::PathBuf;
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn doc(args: &[&str]) -> Output {
    Command::new(AIPL).arg("doc").args(args).current_dir(root()).output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn a_module_shows_each_public_item_with_its_comment() {
    let o = doc(&["tests/aipl/doc/shapes.aipl"]);
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(out(&o), std::fs::read_to_string(root().join("tests/aipl/doc/shapes.expected")).unwrap());
    // private items only on request; implementations never
    assert!(!out(&o).contains("helper"));
    let p = doc(&["--private", "tests/aipl/doc/shapes.aipl"]);
    assert!(out(&p).contains("fn shapes.helper [b:(ptr shapes.Box)] -> i32    (private)\n    Private: aipl doc shows it only with --private.\n"));
    assert!(!out(&p).contains("impl"));
}

#[test]
fn one_item_a_search_and_the_listing() {
    let o = doc(&["option.unwrap_or"]);
    assert_eq!(out(&o), "fn (option.unwrap_or T) [o:(option.Option T) fallback:T] -> T\n    The value, or `fallback` for none.\n");
    let s = doc(&["--find", "WHITESPACE"]);
    assert_eq!(s.status.code(), Some(0));
    assert!(out(&s).contains("fn str.trim [b:str.Bytes] -> str.Bytes\n"), "{}", out(&s));
    assert!(out(&s).contains("fn text.fields [a:alloc.Allocator b:str.Bytes] -> (ptr (vec.Vec str.Bytes))\n"), "{}", out(&s));
    let l = doc(&[]);
    for m in ["alloc", "stream", "text", "vec"] {
        assert!(out(&l).lines().any(|x| x.starts_with(&format!("  {m} "))), "{m} listed:\n{}", out(&l));
    }
}

#[test]
fn errors_name_what_is_missing() {
    let o = doc(&["no_such_module"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&o.stderr), "aipl doc: no module no_such_module\n");
    let i = doc(&["str.no_such_item"]);
    assert_eq!(i.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&i.stderr), "aipl doc: no public item named str.no_such_item\n");
    assert_eq!(doc(&["a", "b"]).status.code(), Some(2));
}

#[test]
fn check_names_each_undocumented_public_item() {
    let o = doc(&["--check", "tests/aipl/doc/shapes.aipl"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&o.stderr),
        "tests/aipl/doc/shapes.aipl:35: undocumented is (pub) but has no ;; comment directly above it\n"
    );
}

#[test]
fn every_public_item_of_the_standard_library_is_documented() {
    let mut files: Vec<String> = std::fs::read_dir(root().join("aipl_src/std"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "aipl"))
        .map(|p| p.strip_prefix(root()).unwrap().display().to_string())
        .collect();
    files.sort();
    let mut args = vec!["--check"];
    args.extend(files.iter().map(|s| s.as_str()));
    let o = doc(&args);
    assert_eq!(o.status.code(), Some(0), "document these (a ;; block directly above each):\n{}", String::from_utf8_lossy(&o.stderr));
}
