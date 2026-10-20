//! aipl eval -e (docs/design/FEEDBACK_TOOLS_PLAN.md F2, src/evalexpr.rs):
//! expressions evaluated and the last one's value printed with its type,
//! the same in the VM, under aipl-run (--wasm), and natively (--native);
//! inside a file's module (--with), its private functions included; with
//! the standard modules it names imported; and errors at -e:LINE:COL.

use std::path::PathBuf;
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn eval(backend: &str, with: Option<&str>, exprs: &str) -> Output {
    let mut c = Command::new(AIPL);
    c.arg("eval").current_dir(root());
    if !backend.is_empty() {
        c.arg(backend);
    }
    if let Some(w) = with {
        c.args(["--with", w]);
    }
    c.args(["-e", exprs]).output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

const ROWS: &str = "tests/aipl/eval/rows.aipl";

#[test]
fn values_print_with_their_types_in_every_backend() {
    let cases: &[(Option<&str>, &str, &str)] = &[
        (None, "(* 6 7)", "42 : i32\n"),
        (None, "(let x:f64 0.1) (+ x 0.2)", "0.30000000000000004 : f64\n"),
        (None, "(i64.extend_s -5)", "-5 : i64\n"),
        (None, "(lt 1 2)", "true : bool\n"),
        (None, "(call str.concat \"ab\" \"cd\")", "abcd : str\n"),
        // a standard module named in the expression is imported for it
        (None, "(call str.find (call str.from_str \"a,b\") (call str.from_str \",\"))", "1 : i32\n"),
        (None, "(call (vec.len i32) (call (vec.make i32) (call alloc.default) 4))", "0 : i32\n"),
        // void: the program's own output, no value
        (None, "(println \"n = {(+ 1 2)}\")", "n = 3\n"),
        (None, "(make (option.Option i32) some 5)", "some{value: 5} : (option.Option i32)\n"),
        (Some(ROWS), "(call make_row 2)", "Row{n: 2} : (ptr Row)\n"),
        // --with: the file's functions, private ones too; its main does not run
        (Some(ROWS), "(call parse_row \"a,b,c\")", "3 : i32\n"),
        (Some(ROWS), "(let r:(ptr Row) (call make_row 2))\n(get r Row.n)", "2 : i32\n"),
        (Some(ROWS), "(call shade 1)", "dark : Shade\n"),
    ];
    for backend in ["", "--wasm", "--native"] {
        for (with, exprs, want) in cases {
            let o = eval(backend, *with, exprs);
            assert_eq!(o.status.code(), Some(0), "{backend} {exprs}: {}", stderr(&o));
            assert_eq!(stdout(&o), *want, "{backend} {exprs}");
        }
    }
}

#[test]
fn a_struct_prints_its_fields_and_a_pointer_inside_it_its_address() {
    let o = eval("", None, "(call (vec.make i32) (call alloc.default) 4)");
    let out = stdout(&o);
    assert!(out.starts_with("(vec.Vec i32){items: (arr i32) @ ") && out.ends_with("} : (ptr (vec.Vec i32))\n"), "{out}");
}

#[test]
fn errors_are_at_the_expressions_own_lines_and_columns() {
    let o = eval("", None, "(let x:i32 1)\n  (+ x true)");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stderr(&o), "Error: -e:2:3: Type mismatch in binary op: i32 vs bool\n");
    let o = eval("", Some(ROWS), "(call parse_row 5)");
    assert_eq!(stderr(&o), "Error: -e:1:1: Arg 0 of 'parse_row' expects str, got i32\n");
    // a trap: its call chain, without the generated printer
    for (backend, code, prefix) in [("", 1, "Error: "), ("--wasm", 134, "-e: "), ("--native", 134, "-e: ")] {
        let o = eval(backend, None, "(arr.get i32 (arr.new i32 (call alloc.forever) 2) 5)");
        assert_eq!(o.status.code(), Some(code), "{backend}");
        assert_eq!(stderr(&o), format!("{prefix}Array index out of bounds: index 5 for array of length 2\n  at __eval (-e:1:1)\n"), "{backend}");
    }
}

#[test]
fn a_file_with_its_own_start_runs_only_in_the_vm() {
    let o = eval("--wasm", Some("aipl_src/aipl.aipl"), "1");
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("defines _start"), "{}", stderr(&o));
    assert_eq!(stdout(&eval("", Some("aipl_src/aipl.aipl"), "1")), "1 : i32\n");
}
