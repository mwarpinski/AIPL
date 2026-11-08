//! `aipl fmt` (aipl_src/fmtcmd.aipl): what it settles, what it leaves alone, that a second run changes nothing, and
//! that no program means anything else afterwards. The self-hosted `aipl` must say the same (referee/tests/test_bootstrap.rs).

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

fn run(args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut child = Command::new(AIPL)
        .args(args)
        .current_dir(root())
        .env("AIPL_STD", root().join("aipl_src/std/"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    (o.status.code().unwrap(), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string())
}

/// `source` formatted (standard input to standard output).
fn fmt(source: &str) -> String {
    let (status, out, err) = run(&["fmt", "-"], source);
    assert_eq!((status, err.as_str()), (0, ""), "{source}");
    out
}

/// A file under target/fmt.
fn file(name: &str, source: &str) -> String {
    let dir = root().join("target").join("fmt");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), source).unwrap();
    format!("target/fmt/{name}")
}

#[test]
fn white_space_that_means_nothing_is_settled() {
    // tabs, trailing spaces, runs of spaces, a space inside a bracket, CRs, the blank lines at both ends and between
    let messy = "\n\n(module m\r\n\t(fn main [] -> i32   ( + 1   2 )  \n\n\n\n    0))   \n\n\n";
    assert_eq!(fmt(messy), "(module m\n  (fn main [] -> i32 (+ 1 2)\n\n    0))\n");
    // a file with no newline at its end gets one, and an empty file stays empty
    assert_eq!(fmt("(module m (fn main [] -> i32 0))"), "(module m (fn main [] -> i32 0))\n");
    assert_eq!(fmt(""), "");
    assert_eq!(fmt("\n\n"), "");
}

#[test]
fn indentation_follows_the_form() {
    // a body is two columns in; a clause and an else, one; other forms under their first argument
    let src = "(module m\n(fn f [a:i32] -> i32\n(cond\n((gt a 0)\n1)\n(else\n2))))\n";
    assert_eq!(fmt(src), "(module m\n  (fn f [a:i32] -> i32\n    (cond\n      ((gt a 0)\n       1)\n      (else\n       2))))\n");
    let args = "(module m\n  (fn f [a:i32] -> i32\n    (when (gt a 0)\n(return 1))\n    (call g a\nb)))\n";
    assert_eq!(fmt(args), "(module m\n  (fn f [a:i32] -> i32\n    (when (gt a 0)\n          (return 1))\n    (call g a\n          b)))\n");
    // a line that begins with a close bracket stands under its open one
    assert_eq!(fmt("(module m\n  (fn f [] -> i32\n    0\n      )\n)\n"), "(module m\n  (fn f [] -> i32\n    0\n  )\n)\n");
}

#[test]
fn a_line_at_an_accepted_indentation_stays() {
    // under the first argument, or one to four columns in: the author's choice
    for pad in [6, 8, 10] {
        let src = format!("(module m\n  (fn f [a:i32] -> i32\n    (when (gt a 0)\n{}(return 1))\n    0))\n", " ".repeat(pad));
        assert_eq!(fmt(&src), src, "{pad}");
    }
    // an (unsafe at the end of a line takes the enclosing form's body
    let un = "(module m\n  (fn main [] -> i32 (unsafe\n    (let a:i32 (mem.alloc 1))\n    0)))\n";
    assert_eq!(fmt(un), un);
}

#[test]
fn comments_and_strings_are_not_touched() {
    let src = "(module m\n  ;; a doc comment\n  (fn main [] -> i32\n    (println \"a   b  ;; not a comment\")   ;; but this is\n    0))\n";
    assert_eq!(fmt(src), "(module m\n  ;; a doc comment\n  (fn main [] -> i32\n    (println \"a   b  ;; not a comment\")   ;; but this is\n    0))\n");
    // a comment on its own stands with the code under it; one at column 0 stays there
    let lost = "(module m\n        ;; misplaced\n  (fn main [] -> i32 0)\n;; a banner\n  (fn other [] -> i32 1))\n";
    assert_eq!(fmt(lost), "(module m\n  ;; misplaced\n  (fn main [] -> i32 0)\n;; a banner\n  (fn other [] -> i32 1))\n");
}

#[test]
fn formatting_twice_changes_nothing_and_no_token_moves() {
    for path in ["aipl_src/lint.aipl", "aipl_src/std/vec.aipl", "aipl_src/resolver.aipl", "examples/accounts.aipl"] {
        let src = std::fs::read_to_string(root().join(path)).unwrap();
        let once = fmt(&src);
        assert_eq!(fmt(&once), once, "{path}: a second run changed it");
        let words = |t: &str| -> Vec<String> {
            t.lines().filter(|l| !l.trim_start().starts_with(';')).flat_map(|l| l.split_whitespace().map(str::to_string)).collect()
        };
        // spaces around brackets differ, so compare the text with all white space gone
        let squash = |t: &str| words(t).concat().replace(' ', "");
        assert_eq!(squash(&once), squash(&src), "{path}: the program changed");
    }
}

#[test]
fn the_repository_is_formatted() {
    // every program a test or a tool asserts positions in is left alone (the ones with @reject or @syntax)
    let (_, listed, _) = {
        let o = Command::new("git").args(["ls-files", "*.aipl"]).current_dir(root()).output().unwrap();
        (0, String::from_utf8_lossy(&o.stdout).to_string(), ())
    };
    let files: Vec<String> = listed
        .lines()
        .filter(|p| {
            let t = std::fs::read_to_string(root().join(p)).unwrap_or_default();
            !t.contains("@reject") && !t.contains("@syntax")
        })
        .map(str::to_string)
        .collect();
    let mut args = vec!["fmt", "--check"];
    args.extend(files.iter().map(String::as_str));
    let (status, out, err) = run(&args, "");
    assert_eq!((status, out.as_str(), err.as_str()), (0, "", ""), "run `aipl fmt` on the files named above");
}

#[test]
fn check_names_the_files_and_fails_and_write_rewrites() {
    let ugly = file("ugly.aipl", "(module ugly\n (fn main [] -> i32   0))\n");
    let fine = file("fine.aipl", "(module fine\n  (fn main [] -> i32 0))\n");
    let (status, out, err) = run(&["fmt", "--check", &fine, &ugly], "");
    assert_eq!((status, out.as_str(), err.as_str()), (1, format!("aipl fmt: {ugly} needs formatting\n").as_str(), ""));
    let (status, out, err) = run(&["fmt", "--check", &fine], "");
    assert_eq!((status, out.as_str(), err.as_str()), (0, "", ""));
    let (status, out, err) = run(&["fmt", &ugly, &fine], "");
    assert_eq!((status, out.as_str(), err.as_str()), (0, "", ""));
    assert_eq!(std::fs::read_to_string(root().join(&ugly)).unwrap(), "(module ugly\n  (fn main [] -> i32 0))\n");
}

#[test]
fn a_file_that_does_not_read_is_reported_and_left_alone() {
    let text = "(module bad\n  (fn main [] -> i32 (println \"oops)))\n";
    let bad = file("bad.aipl", text);
    let (status, out, err) = run(&["fmt", &bad], "");
    assert_eq!(status, 1);
    assert_eq!(out, "");
    assert!(err.contains(&format!("{bad}: 2:")) && err.contains("Unterminated string"), "{err}");
    assert_eq!(std::fs::read_to_string(root().join(&bad)).unwrap(), text);
    let (status, _, err) = run(&["fmt", "target/fmt/not_there.aipl"], "");
    assert_eq!(status, 1);
    assert!(err.contains("cannot read target/fmt/not_there.aipl"), "{err}");
}

#[test]
fn a_command_line_that_asks_for_nothing_prints_the_usage() {
    let (status, _, err) = run(&["fmt"], "");
    assert_eq!(status, 2);
    assert!(err.starts_with("usage: aipl fmt"), "{err}");
}
