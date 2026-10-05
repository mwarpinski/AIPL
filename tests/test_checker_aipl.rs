//! The type checker in AIPL (docs/CHECKER_PLAN.md): each piece checked
//! against the Rust front end it mirrors, on the same text.

use aipl_core::checker::TypeChecker;
use aipl_core::parser::{Parser, TokenKind};
use aipl_core::resolver::Resolver;
use aipl_core::vm::{Value, VM};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// A VM with an AIPL module (and its imports) loaded.
fn vm_with(rel: &str) -> VM {
    let m = Resolver::resolve(&root().join(rel)).unwrap();
    TypeChecker::new().check_module(&m).unwrap();
    let mut vm = VM::new();
    vm.load_module(m);
    vm
}

fn int(v: Value) -> i64 {
    match v {
        Value::Int(i) => i,
        other => panic!("expected Int, got {other:?}"),
    }
}

/// One token as both tokenizers can describe it: kind, line, column, and
/// its value spelled out.
#[derive(Debug, PartialEq)]
struct Tok {
    kind: String,
    line: u32,
    col: u32,
    value: String,
}

/// Line and column of byte offset `pos`, counting characters (as the Rust
/// tokenizer does).
fn line_col(src: &str, pos: usize) -> (u32, u32) {
    let before = &src[..pos];
    let line = before.matches('\n').count() as u32 + 1;
    let col = before.rsplit('\n').next().unwrap().chars().count() as u32 + 1;
    (line, col)
}

fn unescape(raw: &str) -> String {
    let mut out = String::new();
    let mut it = raw.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            out.push(match it.next().unwrap() {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '0' => '\0',
                other => other,
            });
        } else {
            out.push(c);
        }
    }
    out
}

/// The Rust tokenizer's tokens, or its error message.
fn rust_tokens(src: &str) -> Result<Vec<Tok>, String> {
    let toks = Parser::tokenize(src)?;
    Ok(toks
        .into_iter()
        .map(|t| {
            let (kind, value) = match t.kind {
                TokenKind::LParen => ("lparen", String::new()),
                TokenKind::RParen => ("rparen", String::new()),
                TokenKind::LBracket => ("lbracket", String::new()),
                TokenKind::RBracket => ("rbracket", String::new()),
                TokenKind::Colon => ("colon", String::new()),
                TokenKind::Arrow => ("arrow", String::new()),
                TokenKind::Symbol(s) => ("symbol", s),
                // the AIPL token holds the low 32 bits
                TokenKind::IntLit(i) => ("int", (i as i32).to_string()),
                TokenKind::BoolLit(b) => ("bool", b.to_string()),
                TokenKind::StringLit(s) => ("string", s),
                TokenKind::Int64Lit(i) => ("int64", i.to_string()),
                TokenKind::FloatLit(f) => ("float", f.to_bits().to_string()),
            };
            Tok { kind: kind.into(), line: t.line, col: t.col, value }
        })
        .collect())
}

const KINDS: [&str; 13] = ["lparen", "rparen", "lbracket", "rbracket", "colon", "arrow", "symbol", "int", "bool", "string", "int64", "float", "bad"];

/// compiler.tokenize's tokens; a bad token (always last) becomes Err with
/// its position and TokErr.
fn aipl_tokens(vm: &mut VM, src: &str) -> Result<Vec<Tok>, String> {
    let n = src.len();
    let input = int(vm.invoke("host_alloc", vec![Value::Int(n as i64 + 1)]).unwrap());
    vm.write_bytes(input as usize, src.as_bytes());
    let size = int(vm.invoke("token_size", vec![]).unwrap()) as usize;
    let out = int(vm.invoke("host_alloc", vec![Value::Int(((n + 1) * size) as i64)]).unwrap());
    let count = int(vm.invoke("tokenize", vec![Value::Int(input), Value::Int(n as i64), Value::Int(out)]).unwrap());
    let word = |vm: &VM, a: usize| i32::from_le_bytes(vm.read_bytes(a, 4).try_into().unwrap());
    let mut toks = Vec::new();
    for i in 0..count as usize {
        let rec = out as usize + i * size;
        let (kind, a, b, pos) = (word(vm, rec), word(vm, rec + 4), word(vm, rec + 8), word(vm, rec + 12) as usize);
        let kind = KINDS[kind as usize];
        let (line, col) = line_col(src, pos);
        let span = |a: i32, b: i32| src[a as usize..(a + b) as usize].to_string();
        let value = match kind {
            "symbol" => span(a, b),
            // the low 32 bits; b is the literal's length
            "int" => {
                assert_eq!(src[pos..pos + b as usize].parse::<i64>().unwrap() as i32, a, "int literal at {pos}");
                a.to_string()
            }
            "bool" => (a != 0).to_string(),
            "string" => unescape(&span(a, b)),
            "int64" => span(a, b - 3).parse::<i64>().unwrap().to_string(),
            "float" => span(a, b).parse::<f64>().unwrap().to_bits().to_string(),
            "bad" => return Err(format!("{line}:{col} {}", ["unterminated_string", "unknown_escape", "stray_whitespace"][a as usize])),
            _ => String::new(),
        };
        toks.push(Tok { kind: kind.into(), line, col, value });
    }
    Ok(toks)
}

/// The first `L:C` of a Rust error, and which tokenizer error it is.
fn rust_error(e: &str) -> String {
    let pos = e.split(": ").next().unwrap();
    let what = if e.contains("Unterminated string") {
        "unterminated_string"
    } else if e.contains("Unknown escape") {
        "unknown_escape"
    } else if e.contains("unexpected whitespace") {
        "stray_whitespace"
    } else {
        panic!("unexpected tokenizer error {e}")
    };
    format!("{pos} {what}")
}

const TRICKY: &[&str] = &[
    "(+ 1 -2 +3 007 -0 2147483647 2147483648 4294967295 4294967296 -2147483649)",
    "9223372036854775807 9223372036854775808 -9223372036854775808 -9223372036854775809 00000000000000000000001",
    "5i64 -5i64 +5i64 i64 -i64 9223372036854775807i64 9223372036854775808i64 1.5i64",
    "1.5 -0.25 +2.0 .5 5. 1.5e3 1.5E-3 2.e+10 1e5 1.5e 1.5.3 - + -. . e.5",
    "-> true false truex ->x a.b.c x:i32 [a b] ;; comment\n(x)",
    "\"a\\nb\\t\\\"q\\\\\" \"\" \"é ü 日本\" (é) x\r\ny",
    "\"unterminated",
    "\"line\nbreak\"",
    "\"bad \\q escape\"",
    "\"ends in backslash\\",
    "(a\u{000C}b)",
    "(a \u{00A0}b)",
    "(a\u{2003})",
    "(\u{3000})",
    "(a\u{000B})",
    "\"\u{00A0} inside a string is fine\" ;; and \u{2003} in a comment\n1",
    "(é\u{00A0})",
];

#[test]
fn tokenizers_agree_on_every_repository_file_and_tricky_input() {
    let mut vm = vm_with("aipl_src/frontend_host.aipl");
    let mut inputs: Vec<(String, String)> = TRICKY.iter().map(|s| (format!("{s:?}"), s.to_string())).collect();
    for dir in ["aipl_src", "aipl_src/std", "aipl_src/native", "examples", "tests/aipl", "benchmarks"] {
        let mut files: Vec<_> = std::fs::read_dir(root().join(dir)).unwrap().map(|e| e.unwrap().path()).collect();
        files.sort();
        for f in files.into_iter().filter(|f| f.extension().is_some_and(|e| e == "aipl")) {
            inputs.push((f.display().to_string(), std::fs::read_to_string(&f).unwrap()));
        }
    }
    assert!(inputs.len() > 40, "{} inputs", inputs.len());
    for (name, src) in &inputs {
        let ours = aipl_tokens(&mut vm, src);
        match rust_tokens(src) {
            Ok(theirs) => {
                let ours = ours.unwrap_or_else(|e| panic!("{name}: AIPL tokenizer error {e}, Rust accepts"));
                for (i, (a, b)) in theirs.iter().zip(ours.iter()).enumerate() {
                    assert_eq!(a, b, "{name}: token {i}");
                }
                assert_eq!(theirs.len(), ours.len(), "{name}: token count");
            }
            Err(e) => assert_eq!(ours.err(), Some(rust_error(&e)), "{name}: Rust says {e}"),
        }
    }
}

/// line_col counts lines from 1 and columns in characters.
#[test]
fn line_col_counts_characters() {
    let mut vm = vm_with("aipl_src/frontend_host.aipl");
    let src = "ab\né日x\n\nz";
    let p = int(vm.invoke("host_alloc", vec![Value::Int(src.len() as i64)]).unwrap());
    vm.write_bytes(p as usize, src.as_bytes());
    for (pos, want) in [(0, (1, 1)), (1, (1, 2)), (3, (2, 1)), (5, (2, 2)), (8, (2, 3)), (10, (3, 1)), (11, (4, 1))] {
        let line = int(vm.invoke("line_of", vec![Value::Int(p), Value::Int(pos)]).unwrap());
        let col = int(vm.invoke("col_of", vec![Value::Int(p), Value::Int(pos)]).unwrap());
        assert_eq!((line as u32, col as u32), want, "offset {pos}");
        assert_eq!(line_col(src, pos as usize), want, "offset {pos} (test helper)");
    }
}
