//! `aipl repl`: expressions and definitions read from standard input. With the input not a terminal there
//! are no prompts, so what comes out is a transcript of values and errors.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// Feeds `input` to `aipl repl`; (status, standard output, standard error).
fn repl(input: &str) -> (i32, String, String) {
    let mut child = Command::new(AIPL)
        .arg("repl")
        .current_dir(root())
        .env("AIPL_STD", root().join("aipl_src/std/"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    (o.status.code().unwrap(), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string())
}

#[test]
fn an_expression_shows_its_value_and_type() {
    let (status, out, err) = repl("(+ 1 2)\n(eq 1 1)\n");
    assert_eq!((status, out.as_str(), err.as_str()), (0, "3 : i32\ntrue : bool\n", ""));
}

#[test]
fn a_definition_is_kept_for_the_lines_after_it() {
    let (_, out, err) = repl("(fn double [x:i32] -> i32 (* x 2))\n(const LIMIT:i32 10)\n(+ LIMIT (call double 4))\n:show\n");
    assert_eq!(out, "18 : i32\n(fn double [x:i32] -> i32 (* x 2))\n(const LIMIT:i32 10)\n");
    assert_eq!(err, "");
}

#[test]
fn a_new_definition_replaces_the_old_one_of_its_name() {
    let (_, out, _) = repl("(fn f [] -> i32 1)\n(fn f [] -> i32 2)\n(call f)\n");
    assert_eq!(out, "2 : i32\n");
}

#[test]
fn a_form_may_take_several_lines() {
    let (_, out, err) = repl("(fn triple [x:i32] -> i32\n  ;; a comment (with a bracket\n  (* x 3))\n(call triple\n  5)\n");
    assert_eq!((out.as_str(), err.as_str()), ("15 : i32\n", ""));
}

#[test]
fn an_error_is_reported_and_the_session_goes_on() {
    let (status, out, err) = repl("(fn bad [] -> i32 \"x\")\n(+ 1 true)\n)\n(fn good [] -> i32 7)\n(call good)\n:nope\n");
    assert_eq!(status, 0);
    // the bad definition was not kept; the good one was
    assert_eq!(out, "7 : i32\n");
    assert!(err.starts_with("Error: repl: 2:3: Function 'bad' expects return type i32, but body returned str\n  2 |   (fn bad [] -> i32 \"x\")\n"), "{err}");
    assert!(err.contains("Error: -e:1:1: Type mismatch in binary op"), "{err}");
    assert!(err.contains("Error: a bracket closes what was never opened\n"), "{err}");
    assert!(err.contains("Error: :nope: not a command (:help lists them)\n"), "{err}");
}

#[test]
fn a_line_is_definitions_or_expressions_and_reset_forgets() {
    let (_, out, err) = repl("(fn f [] -> i32 1) (call f)\n(fn f [] -> i32 1)\n:reset\n:show\n(call f)\n:quit\n(+ 1 1)\n");
    assert_eq!(out, "");
    assert!(err.starts_with("Error: a line is either definitions or expressions, not both\n"), "{err}");
    assert!(err.contains("Function 'f' is not defined") || err.contains("'f'"), "{err}");
}
