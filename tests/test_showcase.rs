//! The showcase program, examples/jsonpp.aipl, a JSON pretty-printer
//! written as an agent would write it from the guide: its own tests pass
//! (`aipl test`); built as wasm (under aipl-run) and as an executable, it
//! lays out forty generated documents as Python's json.dumps does, in three
//! layouts, from a file or from standard input (tools/json_vectors.py); and
//! its options and every way it fails say the same things.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_showcase_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// The program built two ways: (name, how to start it, with these arguments).
struct Built {
    wasm: PathBuf,
    exe: PathBuf,
}

impl Built {
    fn new(dir: &Path) -> Built {
        let src = root().join("examples/jsonpp.aipl");
        let (wasm, exe) = (dir.join("jsonpp.wasm"), dir.join("jsonpp"));
        let o = Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let o = Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        Built { wasm, exe }
    }

    /// Runs it in `dir` with `args`, `stdin` on standard input.
    fn run(&self, which: &str, dir: &Path, args: &[&str], stdin: &[u8]) -> Output {
        let mut cmd = if which == "wasm" {
            let mut c = Command::new(RUNNER);
            c.arg(&self.wasm);
            c
        } else {
            Command::new(&self.exe)
        };
        let mut child = cmd.args(args).current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        // the program may stop before reading all of its input
        let _ = child.stdin.take().unwrap().write_all(stdin);
        child.wait_with_output().unwrap()
    }
}

#[test]
fn its_own_tests_pass() {
    let o = Command::new(AIPL).args(["test", "examples/jsonpp.aipl"]).current_dir(root()).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success() && out.contains("10 tests: 10 passed, 0 failed"), "{out}{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn it_lays_documents_out_as_pythons_json_does() {
    let dir = scratch("vectors");
    let v = Command::new("python3").arg(root().join("tools/json_vectors.py")).arg(&dir).args(["--cases", "40"]).output().unwrap();
    assert!(v.status.success(), "{}", String::from_utf8_lossy(&v.stderr));
    let built = Built::new(&dir);
    for which in ["wasm", "exe"] {
        for k in 0..40 {
            let input = std::fs::read(dir.join(format!("{k}.json"))).unwrap();
            for (ext, args) in [("pretty", vec![]), ("sorted4", vec!["--sort", "--indent", "4"]), ("compact", vec!["--compact"])] {
                let want = std::fs::read(dir.join(format!("{k}.{ext}"))).unwrap();
                // the document in a file on even cases, on standard input on odd ones
                let o = if k % 2 == 0 {
                    let mut args = args.clone();
                    let name = format!("{k}.json");
                    args.push(&name);
                    built.run(which, &dir, &args, b"")
                } else {
                    built.run(which, &dir, &args, &input)
                };
                assert!(o.status.success(), "{which} case {k} {ext}: {}", String::from_utf8_lossy(&o.stderr));
                assert!(
                    o.stdout == want,
                    "{which} case {k} {ext}\n  input: {}\n  want:  {}\n  got:   {}",
                    String::from_utf8_lossy(&input),
                    String::from_utf8_lossy(&want),
                    String::from_utf8_lossy(&o.stdout)
                );
            }
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn its_options_and_its_failures() {
    let dir = scratch("cli");
    std::fs::write(dir.join("doc.json"), r#"{"a":{"b":[10,20,{"c":true}]},"z":null}"#).unwrap();
    std::fs::write(dir.join("bad.json"), "[1,\n 2,\n]").unwrap();
    let built = Built::new(&dir);
    // (arguments, standard input, status, standard output, standard error)
    let cases: &[(&[&str], &str, i32, &str, &str)] = &[
        (&["-q", "a.b[1]", "doc.json"], "", 0, "20\n", ""),
        (&["--query", "a.b[2]", "-c", "doc.json"], "", 0, "{\"c\":true}\n", ""),
        (&["-q", "a", "-"], r#"{"a":[]}"#, 0, "[]\n", ""),
        (&["-i", "0", "-s"], r#"{"b":[1],"a":{}}"#, 0, "{\n\"a\": {},\n\"b\": [\n1\n]\n}\n", ""),
        (&["-c"], "  [ 1 ,\n 2 ]  ", 0, "[1,2]\n", ""),
        (&["bad.json"], "", 1, "", "jsonpp: bad.json:3:1: expected a value\n"),
        (&[], "{\"a\":1,}", 1, "", "jsonpp: standard input:1:8: expected a string key\n"),
        (&[], "", 1, "", "jsonpp: standard input:1:1: the text ends where a value should be\n"),
        (&[], "[1] x", 1, "", "jsonpp: standard input:1:5: unexpected text after the value\n"),
        (&[], "\"\\ud800\"", 1, "", "jsonpp: standard input:1:2: a lone surrogate in a \\u escape\n"),
        (&[], "\u{0}", 1, "", "jsonpp: standard input:1:1: expected a value\n"),
        (&["-q", "x", "doc.json"], "", 1, "", "jsonpp: doc.json: nothing at x\n"),
        (&["-q", "a..b"], "{}", 1, "", "jsonpp: bad path a..b: write a.b[2].c, or [0].name\n"),
        (&["-i", "99"], "1", 1, "", "jsonpp: --indent is 0 to 16\n"),
        (&["-i", "-1"], "1", 1, "", "jsonpp: --indent is 0 to 16\n"),
        (&["-i", "x"], "1", 1, "", "jsonpp: 'x' for indent is not an integer\n"),
        (&["missing.json"], "", 1, "", "jsonpp: missing.json: no such file or directory\n"),
        (&["doc.json", "doc.json"], "", 1, "", "jsonpp: one file at a time\n"),
    ];
    for which in ["wasm", "exe"] {
        for (args, stdin, status, stdout, stderr) in cases {
            let o = built.run(which, &dir, args, stdin.as_bytes());
            let got = (o.status.code(), String::from_utf8_lossy(&o.stdout).to_string(), String::from_utf8_lossy(&o.stderr).to_string());
            assert_eq!(got, (Some(*status), stdout.to_string(), stderr.to_string()), "{which} {args:?} with {stdin:?}");
        }
        // --help: the usage, status 0
        let o = built.run(which, &dir, &["--help"], b"");
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(o.status.success() && out.starts_with("usage: jsonpp [options] [file...]") && out.contains("--query"), "{which}: {out}");
        // a document that is not UTF-8
        let o = built.run(which, &dir, &[], b"\xff\xfe");
        assert_eq!((o.status.code(), String::from_utf8_lossy(&o.stderr).to_string()), (Some(1), "jsonpp: standard input:1:1: the text is not UTF-8\n".to_string()), "{which}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
