//! `aipl eval -e EXPRS [--with FILE] [--wasm | --native]`
//! (docs/design/FEEDBACK_TOOLS_PLAN.md, F2): one or more expressions,
//! evaluated, and the last one's value printed with its type, `49 : i32`.
//!
//! The expressions become the body of a function, `__eval`, added at the
//! end of FILE's own module (so they may call its private functions; its
//! `main` is not run), or of an empty module. Standard modules they name
//! (`str.find`, `text.split`) are imported for them. The module is resolved
//! and checked with `__eval` returning `void`, which gives the type of its
//! last expression (TypeChecker::body_type); then, in the flat module,
//! `__eval` returns that type and a function `__eval_print` prints it with
//! the formatting forms (lowered as any other, formats::lower). The VM runs
//! `__eval_print`; `--wasm` and `--native` compile the module with a
//! `_start` that calls it and run it as a program. Errors on the
//! expressions' lines are reported at `-e:LINE:COL`. The decisions live in
//! this file, not in the language: nothing here is new syntax.

use crate::ast::{Expr, FnDef, Literal, Module, OpCode, Type};
use crate::checker::TypeChecker;
use crate::resolver::Resolver;
use std::path::{Path, PathBuf};

/// Where the expressions run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    Vm,
    Wasm,
    Native,
}

/// Where the expressions sit in the generated source: their first line,
/// how many lines they take, and the columns before them on the first.
struct Place {
    path: PathBuf,
    line: u32,
    lines: u32,
    col: u32,
}

/// Evaluates `exprs` and prints the last value; the exit status. `runner`
/// is the aipl-run launcher (for `--wasm`), `args` the program's arguments.
pub fn run(exprs: &str, with: Option<&Path>, backend: Backend, runner: &Path, args: &[String]) -> Result<i32, String> {
    let (module, place) = prepare(exprs, with, backend)?;
    let mapped = |e: String| relocate(&e, &place);
    match backend {
        Backend::Vm => {
            let mut vm = crate::vm::VM::new();
            vm.set_args(std::iter::once("-e".to_string()).chain(args.iter().cloned()).collect());
            vm.load_module(module);
            match vm.invoke("__eval_print", vec![]) {
                Ok(_) => Ok(0),
                Err(e) => Err(mapped(format!("{e}\n{}", vm.call_chain()).trim_end().to_string())),
            }
        }
        Backend::Wasm | Backend::Native => {
            let wasm = crate::compiler::wasm::WasmCompiler::compile(&module)?;
            let (bytes, ext) = if backend == Backend::Native { (crate::native::executable(&wasm, false)?, "") } else { (wasm, ".wasm") };
            let file = std::env::temp_dir().join(format!("aipl_eval_{}{ext}", std::process::id()));
            std::fs::write(&file, &bytes).map_err(|e| format!("cannot write {}: {e}", file.display()))?;
            let mut cmd = if backend == Backend::Native {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
                std::process::Command::new(&file)
            } else {
                let mut c = std::process::Command::new(runner);
                c.arg(&file);
                c
            };
            let out = cmd.args(args).stderr(std::process::Stdio::piped()).output();
            let _ = std::fs::remove_file(&file);
            let out = out.map_err(|e| e.to_string())?;
            use std::io::Write;
            std::io::stdout().write_all(&out.stdout).map_err(|e| e.to_string())?;
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.is_empty() {
                // the program is named as -e, not as the temporary file
                let err = err.replace(&format!("{}: ", file.display()), "-e: ");
                eprintln!("{}", mapped(err));
            }
            Ok(out.status.code().unwrap_or(1))
        }
    }
}

/// The module to run, with `__eval_print` (and for a compiled backend a
/// `_start` calling it), and where the expressions are in its source.
fn prepare(exprs: &str, with: Option<&Path>, backend: Backend) -> Result<(Module, Place), String> {
    let (path, src) = match with {
        Some(p) => (p.to_path_buf(), std::fs::read_to_string(p).map_err(|e| format!("cannot read {}: {e}", p.display()))?),
        None => (std::env::current_dir().map_err(|e| e.to_string())?.join("-e.aipl"), "(module eval)\n".to_string()),
    };
    let (name, close) = module_form(&src).ok_or_else(|| format!("{}: no (module NAME ...) to evaluate in", path.display()))?;
    // std/show prints the value (the resolver adds it only for source that formats)
    let mut imports = String::new();
    if !src.contains("(import show)") {
        imports.push_str("  (import show)\n");
    }
    for m in named_modules(exprs) {
        if m != name && m != "show" && !src.contains(&format!("(import {m})")) {
            imports.push_str(&format!("  (import {m})\n"));
        }
    }
    let head = format!("{}\n{imports}  (fn __eval [] -> void ", &src[..close]);
    let place = Place {
        path: path.clone(),
        line: head.matches('\n').count() as u32 + 1,
        lines: exprs.matches('\n').count() as u32 + 1,
        col: (head.len() - head.rfind('\n').map_or(0, |i| i + 1)) as u32,
    };
    let text = format!("{head}{exprs})\n{}", &src[close..]);
    let mapped = |e: String| relocate(&e, &place);

    let mut module = Resolver::resolve_source(&text, &path).map_err(mapped)?;
    let mut checker = TypeChecker::new();
    checker.check_module(&module).map_err(mapped)?;
    let ty = checker.body_type("__eval").ok_or("aipl eval: the expressions were not checked")?;

    let span = (place.line, place.col + 1);
    let lit = |s: String| Expr::Lit(Literal::Str(s), span);
    let var = || Expr::Var("__v".into(), span);
    let piece = |v: Expr| Expr::Op { op: OpCode::FormatPiece, args: vec![v, lit(String::new())], span };
    let shown = crate::formats::source_type(&ty);
    let mut body = Vec::new();
    if ty == Type::Void {
        body.push(Expr::Call { func: "__eval".into(), args: vec![], span });
    } else {
        let f = module.functions.iter_mut().find(|f| f.name == "__eval").ok_or("aipl eval: no __eval")?;
        f.return_type = ty.clone();
        body.push(Expr::Let { name: "__v".into(), ty: ty.clone(), val: Box::new(Expr::Call { func: "__eval".into(), args: vec![], span }), span });
        let pieces = if crate::formats::prints(&ty) {
            vec![piece(var()), lit(format!(" : {shown}"))]
        } else {
            vec![lit(format!("<value> : {shown}"))]
        };
        body.push(Expr::Op { op: OpCode::FormatPrintln, args: pieces, span });
    }
    let def = |name: &str, body: Vec<Expr>| FnDef {
        name: name.into(),
        params: vec![],
        return_type: Type::Void,
        contracts: vec![],
        body,
        span,
        is_unsafe: true,
    };
    module.functions.push(def("__eval_print", body));
    if backend != Backend::Vm {
        if module.functions.iter().any(|f| f.name == "_start") {
            return Err(format!(
                "{}: defines _start, and --wasm and --native run the expressions as the program's _start: leave them out to use the VM",
                path.display()
            ));
        }
        module.functions.push(def("_start", vec![Expr::Call { func: "__eval_print".into(), args: vec![], span }]));
    }
    // the printing is a (println ...), lowered to std/show calls
    crate::formats::lower(&mut module);
    TypeChecker::new().check_module(&module).map_err(mapped)?;
    Ok((module, place))
}

/// The module's name and the index of the ')' that closes `(module ...)`.
fn module_form(src: &str) -> Option<(String, usize)> {
    let b = src.as_bytes();
    let (mut i, mut depth, mut start) = (0, 0usize, None);
    while i < b.len() {
        match b[i] {
            b';' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'(' | b'[' => {
                if depth == 0 && start.is_none() && src[i + 1..].trim_start().starts_with("module") {
                    start = Some(i);
                }
                depth += 1;
            }
            b')' | b']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 && start.is_some() {
                    let after = src[start? + 1..].trim_start().strip_prefix("module")?;
                    let name = after.split(|c: char| c.is_whitespace() || c == '(' || c == ')').find(|s| !s.is_empty())?;
                    return Some((name.to_string(), i));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The standard modules the expressions name, as `str` in `(call str.find
/// ...)`, in the order named.
fn named_modules(exprs: &str) -> Vec<String> {
    let std = Path::new(env!("CARGO_MANIFEST_DIR")).join("aipl_src/std");
    let b = exprs.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let starts = b[i].is_ascii_lowercase() && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_' || b[i - 1] == b'.'));
        if !starts {
            i += 1;
            continue;
        }
        let s = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
            i += 1;
        }
        let word = &exprs[s..i];
        if i < b.len() && b[i] == b'.' && std.join(format!("{word}.aipl")).exists() && !out.iter().any(|m| m == word) {
            out.push(word.to_string());
        }
    }
    out
}

/// `msg` with each position on the expressions' lines given as `-e:L:C`,
/// and without the generated functions' frames in a call chain.
fn relocate(msg: &str, place: &Place) -> String {
    let msg: String = msg
        .split_inclusive('\n')
        .filter(|l| !(l.trim_start().starts_with("at __eval_print ") || l.trim_start().starts_with("at _start ")))
        .collect();
    let msg = msg.trim_end_matches('\n');
    let full = place.path.display().to_string();
    let short = place.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = msg.to_string();
    // "path: L:C:" (errors) and "path:L:C" (call chains)
    for prefix in [format!("{full}: "), format!("{full}:"), format!("{short}:")] {
        let mut result = String::new();
        let mut rest = out.as_str();
        while let Some(at) = rest.find(&prefix) {
            result.push_str(&rest[..at]);
            let after = &rest[at + prefix.len()..];
            let digits = |s: &str| s.bytes().take_while(|b| b.is_ascii_digit()).count();
            let nl = digits(after);
            let nc = if nl > 0 && after[nl..].starts_with(':') { digits(&after[nl + 1..]) } else { 0 };
            if nc > 0 {
                let l: u32 = after[..nl].parse().unwrap_or(0);
                let c: u32 = after[nl + 1..nl + 1 + nc].parse().unwrap_or(0);
                if l >= place.line && l < place.line + place.lines {
                    let col = if l == place.line { c.saturating_sub(place.col) } else { c };
                    result.push_str(&format!("-e:{}:{col}", l - place.line + 1));
                    rest = &after[nl + 1 + nc..];
                    continue;
                }
            }
            result.push_str(&prefix);
            rest = after;
        }
        result.push_str(rest);
        out = result;
    }
    out
}
