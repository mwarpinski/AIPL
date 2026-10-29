//! aipl eval -e (docs/design/FEEDBACK_TOOLS_PLAN.md F2, src/evalexpr.rs):
//! expressions evaluated and the last one's value printed with its type,
//! the same in the VM, under aipl-run (--wasm), and natively (--native);
//! inside a file's module (--with), its private functions included; with
//! the standard modules it names imported; and errors at -e:LINE:COL.

use std::path::PathBuf;
use std::process::{Command, Output};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");

/// The native backend targets Linux x86-64 only (AIPL_SPEC.md 6.6).
const NATIVE: bool = cfg!(all(target_os = "linux", target_arch = "x86_64"));

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
    let backends: &[&str] = if NATIVE {
        &["", "--wasm", "--native"]
    } else {
        &["", "--wasm"]
    };
    for backend in backends {
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
    assert_eq!(stderr(&o), "Error: -e:2:3: Type mismatch in binary op: i32 vs bool; both operands must have one type: convert one (i64.extend_s, f64.convert_i32_s, ...) or write the literal in that type (5i64, 1.0)\n");
    let o = eval("", Some(ROWS), "(call parse_row 5)");
    assert_eq!(stderr(&o), "Error: -e:1:1: Arg 0 of 'parse_row' expects str, got i32\n");
    // a trap: its call chain, without the generated printer
    let trap_cases: &[(&str, i32, &str)] = if NATIVE {
        &[("", 1, "Error: "), ("--wasm", 134, "-e: "), ("--native", 134, "-e: ")]
    } else {
        &[("", 1, "Error: "), ("--wasm", 134, "-e: ")]
    };
    for (backend, code, prefix) in trap_cases {
        let (backend, code, prefix) = (*backend, *code, *prefix);
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

/// `aipl eval` in the self-hosted aipl (aipl_src/evalcmd.aipl) prints what the
/// Rust command prints with `--native`: the value and its type for each kind
/// of expression, inside a file's module with `--with`, errors at -e:LINE:COL,
/// a trap's call chain without the generated printer, and the same statuses.
/// A trap's status is 134, which a program cannot exit with (a status of 126
/// or more is an error, with 134), so the self-hosted command says so on a
/// line more. What needs the VM or aipl-run is refused, saying so; a file runs
/// as `aipl run` runs it.
#[test]
fn the_self_hosted_aipl_evaluates_as_the_rust_one_does_natively() {
    if !NATIVE {
        return;
    }
    let dir = std::env::temp_dir().join(format!("aipl_eval_self_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let me = dir.join("aipl");
    let o = Command::new(AIPL).args(["compile", "--exe", "--allow-run", "*", "aipl_src/aipl.aipl", "-o"]).arg(&me).current_dir(root()).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let tmp = dir.join("tmp");
    std::fs::create_dir(&tmp).unwrap();
    let run = |exe: &std::path::Path, args: &[&str]| {
        Command::new(exe).args(args).current_dir(root()).env("AIPL_STD", root().join("aipl_src/std/")).env("TMPDIR", &tmp).output().unwrap()
    };
    let cases: &[(Option<&str>, &str)] = &[
        (None, "(* 6 7)"),
        (None, "(let x:f64 0.1) (+ x 0.2)"),
        (None, "(i64.extend_s -5)"),
        (None, "(lt 1 2)"),
        (None, "(call str.concat \"ab\" \"cd\")"),
        (None, "(call str.find (call str.from_str \"a,b\") (call str.from_str \",\"))"),
        (None, "(call (vec.len i32) (call (vec.make i32) (call alloc.default) 4))"),
        (None, "(println \"n = {(+ 1 2)}\")"),
        (None, "(make (option.Option i32) some 5)"),
        (None, "(call (vec.make i32) (call alloc.default) 4)"),
        (Some(ROWS), "(call make_row 2)"),
        (Some(ROWS), "(call parse_row \"a,b,c\")"),
        (Some(ROWS), "(let r:(ptr Row) (call make_row 2))\n(get r Row.n)"),
        (Some(ROWS), "(call shade 1)"),
        // errors, at the expressions' own lines and columns
        (None, "(let x:i32 1)\n  (+ x true)"),
        (Some(ROWS), "(call parse_row 5)"),
        (Some(ROWS), "(call no_such_function 1)"),
        // a value with no way to print it, and one with no value
        (Some(ROWS), "(ref parse_row)"),
    ];
    for (with, exprs) in cases {
        let mut args = vec!["eval", "-e", *exprs, "--native"];
        if let Some(w) = with {
            args.extend(["--with", *w]);
        }
        let r = run(std::path::Path::new(AIPL), &args);
        let a = run(&me, &args);
        assert_eq!((a.status.code(), stdout(&a), stderr(&a)), (r.status.code(), stdout(&r), stderr(&r)), "{with:?} {exprs}");
    }
    // a trap: the call chain at the expressions, the status 134 (and the line more)
    let trap = ["eval", "-e", "(arr.get i32 (arr.new i32 (call alloc.forever) 2) 5)", "--native"];
    let (r, a) = (run(std::path::Path::new(AIPL), &trap), run(&me, &trap));
    assert_eq!((a.status.code(), r.status.code()), (Some(134), Some(134)));
    assert!(stderr(&a).starts_with(&stderr(&r)), "{}\n{}", stderr(&a), stderr(&r));
    assert!(stderr(&a).contains("exit with invalid exit status"), "{}", stderr(&a));
    // arguments after `--` reach the program
    let o = run(&me, &["eval", "-e", "(call os.arg_count)", "--", "x", "y"]);
    assert_eq!((o.status.code(), stdout(&o)), (Some(0), "3 : i32\n".to_string()), "{}", stderr(&o));
    // a file with its own start cannot be wrapped
    let o = run(&me, &["eval", "-e", "1", "--with", "aipl_src/aipl.aipl"]);
    assert!(o.status.code() == Some(1) && stderr(&o).contains("defines _start"), "{}", stderr(&o));
    // a file runs, as aipl run runs it: its output and status, its arguments after `--`
    let o = run(&me, &["eval", "tests/aipl/sys/run_target.aipl", "--", "a", "b c"]);
    assert_eq!((o.status.code(), stdout(&o)), (Some(0), "args 3\narg a\narg b c\nstdin \n".to_string()), "{}", stderr(&o));
    let o = run(&me, &["eval", "tests/aipl/sys/run_target.aipl", "--", "seven"]);
    assert_eq!(o.status.code(), Some(7));
    // what needs the VM or aipl-run
    for args in [&["eval", "-e", "1", "--wasm"][..], &["eval", "-f", "other", ROWS], &["eval", "--wasm", ROWS]] {
        let o = run(&me, args);
        assert!(o.status.code() == Some(1) && stderr(&o).contains("Rust aipl"), "{args:?}: {}", stderr(&o));
    }
    // usage
    for args in [&["eval"][..], &["eval", "-e"], &["eval", "-e", "1", "--no-such-flag"]] {
        assert_eq!(run(&me, args).status.code(), Some(2), "{args:?}");
    }
    assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0, "a temporary executable was left");
    let _ = std::fs::remove_dir_all(&dir);
}
