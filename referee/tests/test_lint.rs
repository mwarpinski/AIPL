//! `aipl lint FILE.aipl...` (aipl_src/lint.aipl): what each rule finds, what it leaves alone, how a finding is
//! silenced, and the exit status. The self-hosted `aipl` must say the same (referee/tests/test_bootstrap.rs).

use std::path::PathBuf;
use std::process::{Command, Stdio};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");

fn root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// One finding as `aipl lint` prints it: the line, then the source line under it with a caret.
fn finding(file: &str, line: usize, col: usize, rule: &str, text: &str, source: &str) -> String {
    let n = line.to_string();
    format!("{file}: {line}:{col}: warning[{rule}]: {text}\n  {n} | {source}\n  {} | {}^\n", " ".repeat(n.len()), " ".repeat(col - 1))
}

/// Writes `source` to target/lint/NAME.aipl and lints it from the repository root; (status, standard output, standard error).
fn lint(name: &str, source: &str) -> (i32, String, String) {
    let dir = root().join("target").join("lint");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}.aipl")), source).unwrap();
    let o = Command::new(AIPL)
        .args(["lint", &format!("target/lint/{name}.aipl")])
        .current_dir(root())
        .env("AIPL_STD", root().join("aipl_src/std/"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    (o.status.code().unwrap(), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string())
}

fn clean(name: &str, source: &str) {
    let (status, out, err) = lint(name, source);
    assert_eq!((status, out.as_str(), err.as_str()), (0, "lint: no findings\n", ""), "{name}");
}

fn finds(name: &str, source: &str, want: &[(usize, usize, &str, &str, &str)]) {
    let (status, out, err) = lint(name, source);
    let file = format!("target/lint/{name}.aipl");
    let want: String = want.iter().map(|(l, c, rule, text, src)| finding(&file, *l, *c, rule, text, src)).collect();
    assert_eq!(out, want, "{name}");
    assert_eq!((status, err.as_str()), (1, ""), "{name}");
}

#[test]
fn code_that_is_fine_has_no_findings() {
    clean("clean", "(module clean\n  (fn main [] -> i32\n    (let n:i32 2)\n    (return n)))\n");
    // a name read only inside a format string is read
    clean("in_format", "(module in_format\n  (fn main [] -> i32\n    (let n:i32 2)\n    (println \"n is {n}\")\n    0))\n");
    // a leading underscore says that a name is not meant to be read or called
    clean("underscore", "(module underscore\n  (fn f [_a:i32] -> i32\n    (let _n:i32 1)\n    0)\n  (fn _kept [] -> i32 0)\n  (fn main [] -> i32 (call f 1)))\n");
    // (pub) is used from elsewhere
    clean("shown", "(module shown\n  (fn shown [] -> i32 (pub) 0)\n  (fn main [] -> i32 0))\n");
}

#[test]
fn a_variable_or_parameter_that_is_never_read() {
    finds(
        "unused",
        "(module unused\n  (fn f [a:i32 b:i32] -> i32\n    (let n:i32 0)\n    (set! n 5)\n    a)\n  (fn main [] -> i32 (call f 1 2)))\n",
        &[
            (2, 16, "unused-parameter", "parameter 'b' is never read", "  (fn f [a:i32 b:i32] -> i32"),
            (3, 10, "unused-variable", "variable 'n' is never read", "    (let n:i32 0)"),
        ],
    );
}

#[test]
fn code_after_a_return_is_never_reached() {
    finds(
        "unreach",
        "(module unreach\n  (fn main [] -> i32\n    (return 1)\n    (return 2)))\n",
        &[(3, 5, "unreachable-code", "the code after this is never reached", "    (return 1)")],
    );
    // a branch that returns leaves the code after the if alone
    clean("branch", "(module branch\n  (fn main [] -> i32\n    (when (lt 1 2) (return 1))\n    2))\n");
}

#[test]
fn format_and_concat_in_a_loop_leak() {
    finds(
        "loops",
        "(module loops\n  (import io)\n  (fn main [] -> i32\n    (for i 0 3\n      (call io.println (format \"n={i}\")))\n    (while false\n      (call io.println (call str.concat \"a\" \"b\")))\n    0))\n",
        &[
            (5, 24, "leaky-in-loop", "format makes a new str on every turn of the loop, and nothing frees it; write into a buf with format_to", "      (call io.println (format \"n={i}\")))"),
            (7, 24, "leaky-in-loop", "str.concat makes a new str on every turn of the loop, and nothing frees it; push onto a buf instead", "      (call io.println (call str.concat \"a\" \"b\")))"),
        ],
    );
}

#[test]
fn what_a_return_makes_in_a_loop_is_made_once() {
    clean(
        "exits",
        "(module exits\n  (fn f [n:i32] -> (result i32 str)\n    (for i 0 n\n      (when (eq i 2) (return (err:i32 (format \"stopped at {i}\")))))\n    (ok:str 0))\n  (fn main [] -> i32 (match_result (call f 3) (ok v v) (err e 1))))\n",
    );
}

#[test]
fn a_comparison_with_true_and_an_empty_else() {
    finds(
        "bools",
        "(module bools\n  (fn f [a:bool] -> bool\n    (when (eq a true) (return false))\n    (when (neq a false) (return (not (not a))))\n    (if a true false))\n  (fn main [] -> i32 (if (call f true) 1 (block))))\n",
        &[
            (3, 11, "redundant-bool", "comparing with true or false; use the condition itself (or `not`)", "    (when (eq a true) (return false))"),
            (4, 11, "redundant-bool", "comparing with true or false; use the condition itself (or `not`)", "    (when (neq a false) (return (not (not a))))"),
            (4, 33, "redundant-bool", "`not` of `not` is the condition itself", "    (when (neq a false) (return (not (not a))))"),
            (5, 5, "redundant-bool", "an `if` that gives true or false is the condition itself (or `not`)", "    (if a true false))"),
            (6, 42, "empty-else", "the else is empty; write (when condition ...)", "  (fn main [] -> i32 (if (call f true) 1 (block))))"),
        ],
    );
}

#[test]
fn sys_print_already_ends_the_line() {
    finds(
        "sysprint",
        "(module sysprint\n  (fn main [] -> i32\n    (sys.print \"one\\n\")\n    (sys.print \"two\")\n    0))\n",
        &[(3, 16, "sys-print-newline", "sys.print ends the line itself, so this prints two newlines; drop the \\n (the form print does not end the line)", "    (sys.print \"one\\n\")")],
    );
}

#[test]
fn names_follow_the_conventions() {
    finds(
        "names",
        "(module names\n  (const Limit:i32 3)\n  (struct point [x:i32])\n  (enum color [red])\n  (fn myFn [] -> i32 0)\n  (fn main [] -> i32 (call myFn)))\n",
        &[
            (2, 10, "naming", "constant 'Limit' should be written in capitals", "  (const Limit:i32 3)"),
            (3, 11, "naming", "type 'point' should begin with a capital letter", "  (struct point [x:i32])"),
            (4, 9, "naming", "type 'color' should begin with a capital letter", "  (enum color [red])"),
            (5, 7, "naming", "function 'myFn' should be written in snake_case", "  (fn myFn [] -> i32 0)"),
        ],
    );
}

#[test]
fn an_import_or_function_nothing_uses() {
    finds(
        "dead",
        "(module dead\n  (import vec)\n  (import io)\n  (fn helper [] -> i32 0)\n  (fn main [] -> i32 (call io.println \"x\") 0))\n",
        &[
            (2, 3, "unused-import", "import 'vec' is never used", "  (import vec)"),
            (4, 7, "unused-function", "function 'helper' is private and nothing names it", "  (fn helper [] -> i32 0)"),
        ],
    );
}

#[test]
fn a_finding_is_silenced_by_a_comment_naming_its_rule() {
    // on the line above, at the end of the line, `all`; the comment names a rule or the next finding is not silenced
    finds(
        "allow",
        "(module allow\n  (fn main [] -> i32\n    ;; lint:allow unused-variable\n    (let a:i32 1)\n    (let b:i32 2) ;; lint:allow all\n    (let x:i32 9)\n    (let c:i32 3)\n    ;; lint:allow naming\n    (let d:i32 4)\n    (println \"{x}\")\n    0))\n",
        &[
            (7, 10, "unused-variable", "variable 'c' is never read", "    (let c:i32 3)"),
            (9, 10, "unused-variable", "variable 'd' is never read", "    (let d:i32 4)"),
        ],
    );
}

#[test]
fn a_file_that_does_not_read_is_reported_and_fails() {
    let (status, out, _err) = lint("broken", "(module broken\n  (fn main [] -> i32 (return \"oops))\n");
    assert_eq!(status, 1);
    assert_eq!(out, "target/lint/broken.aipl: 2:30: Unterminated string literal\n  2 |   (fn main [] -> i32 (return \"oops))\n    |                              ^\n");
    let o = Command::new(AIPL).args(["lint", "target/lint/nothing_here.aipl"]).current_dir(root()).env("AIPL_STD", root().join("aipl_src/std/")).stdin(Stdio::null()).output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&o.stderr), "aipl lint: cannot read target/lint/nothing_here.aipl\n");
    // no file named: usage
    let o = Command::new(AIPL).arg("lint").current_dir(root()).env("AIPL_STD", root().join("aipl_src/std/")).stdin(Stdio::null()).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
}
