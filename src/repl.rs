//! `aipl repl`: expressions typed at a prompt, each evaluated and its value
//! printed with its type (`49 : i32`), as `aipl eval -e` does, and
//! definitions (`fn`, `const`, `struct`, `enum`, `union`, `import`, `impl`) kept
//! for the lines that follow. It is for people at a keyboard; the tools an AI
//! uses are `aipl eval -e`, `aipl test` and `aipl verify`.
//!
//! The session is a module, `repl`, in a temporary file that grows by one
//! definition at a time; every expression is evaluated inside it with
//! `evalexpr::run` (the VM), so a definition may call any other, private or
//! not. A definition is kept only if the module still checks with it. A new
//! `fn`, `const` or type replaces an earlier one of the same name. Nothing
//! else is remembered between lines: a `let` lasts for its own line, because
//! every line is a new program. (`const` is how a value is kept.)
//!
//! A line is read until its brackets close, so a form may take several. With
//! the input not a terminal there are no prompts, which makes the output a
//! transcript: the values, the errors, nothing else.

use crate::evalexpr::{run, Backend};
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;

const HELP: &str = "\
Type an expression to see its value and type: (+ 1 2), (call str.find \"hello\" \"l\").
Type a definition to keep it: (fn double [x:i32] -> i32 (* x 2)), (const LIMIT:i32 10),
(struct, enum, union, import, impl). A new one replaces an old one of the same name.
A line is read until its brackets close. Nothing else is kept: a (let ...) lasts for its line.
  :show    the definitions so far
  :reset   forget them
  :help    this text
  :quit    leave (so does the end of input, Ctrl-D)
";

/// One definition of the session: what it defines (a `fn double`), and its text.
struct Def {
    key: String,
    text: String,
}

/// How far into `input` the first form reaches: its end, or None if it has not closed.
/// Text that does not begin a form (white space, a comment) is skipped; a `)` with nothing
/// open is an error.
enum Scan {
    /// The index just past the form, or past the text if only white space and comments remain.
    Done(usize),
    /// A form is open still.
    Open,
    /// A bracket closes what was never opened.
    Stray,
}

/// Scans `input` from `from` for one top-level form.
fn scan_form(input: &str, from: usize) -> Scan {
    let b = input.as_bytes();
    let (mut i, mut depth, mut started) = (from, 0i32, false);
    while i < b.len() {
        match b[i] {
            b';' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'"' => {
                started = true;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                if i >= b.len() {
                    return Scan::Open;
                }
            }
            b'(' | b'[' => {
                started = true;
                depth += 1;
            }
            b')' | b']' => {
                if depth == 0 {
                    return Scan::Stray;
                }
                depth -= 1;
            }
            c if !c.is_ascii_whitespace() => started = true,
            _ => {}
        }
        i += 1;
        if started && depth == 0 {
            // a word or a number ends at white space or a bracket; a form at its closing bracket
            if !matches!(b[i - 1], b')' | b']' | b'"') {
                while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'(' | b')' | b'[' | b']' | b'"' | b';') {
                    i += 1;
                }
            }
            return Scan::Done(i);
        }
    }
    if started {
        Scan::Open
    } else {
        Scan::Done(b.len())
    }
}

/// The top-level forms of `input`, each as its text; Err(Open) when the last has not closed.
fn forms(input: &str) -> Result<Vec<String>, Scan> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < input.len() {
        match scan_form(input, at) {
            Scan::Done(end) => {
                let piece = strip_comments(&input[at..end]);
                if !piece.trim().is_empty() {
                    out.push(piece.trim().to_string());
                }
                at = end;
            }
            other => return Err(other),
        }
    }
    Ok(out)
}

/// `text` without a leading comment line.
fn strip_comments(text: &str) -> String {
    let mut s = text.trim_start();
    while s.starts_with(';') {
        s = s.find('\n').map_or("", |n| s[n + 1..].trim_start());
    }
    s.to_string()
}

/// What a definition defines: its head and, for most, its name (`fn double`); the whole form otherwise.
fn definition_key(form: &str) -> Option<String> {
    let inner = form.strip_prefix('(')?;
    let mut words = inner.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | ':'));
    let head = words.find(|w| !w.is_empty())?;
    match head {
        "fn" | "const" | "struct" | "enum" | "union" => {
            let name = words.find(|w| !w.is_empty())?;
            Some(format!("{head} {name}"))
        }
        "import" | "impl" => Some(form.to_string()),
        _ => None,
    }
}

struct Session {
    defs: Vec<Def>,
    file: std::path::PathBuf,
}

impl Session {
    fn text(&self, defs: &[Def]) -> String {
        let mut s = String::from("(module repl\n");
        for d in defs {
            s.push_str("  ");
            s.push_str(&d.text.replace('\n', "\n  "));
            s.push('\n');
        }
        s.push_str(")\n");
        s
    }

    fn write(&self, defs: &[Def]) -> Result<(), String> {
        std::fs::write(&self.file, self.text(defs)).map_err(|e| format!("cannot write {}: {e}", self.file.display()))
    }

    /// Keeps the definitions if the session still checks with them.
    fn define(&mut self, new: Vec<(String, String)>) -> Result<(), String> {
        let mut defs: Vec<Def> = Vec::new();
        for d in &self.defs {
            defs.push(Def { key: d.key.clone(), text: d.text.clone() });
        }
        for (key, text) in new {
            defs.retain(|d| d.key != key);
            defs.push(Def { key, text });
        }
        self.write(&defs)?;
        // an empty block is void: nothing is printed, and only the checking can fail
        match run("(block)", Some(&self.file), Backend::Vm, Path::new(""), &[]) {
            Ok(0) => {
                self.defs = defs;
                Ok(())
            }
            Ok(_) => Err("the definition was not accepted".to_string()),
            Err(e) => Err(e),
        }
    }

    fn eval(&self, exprs: &str) -> Result<(), String> {
        self.write(&self.defs)?;
        match run(exprs, Some(&self.file), Backend::Vm, Path::new(""), &[]) {
            Ok(0) => Ok(()),
            Ok(n) => Err(format!("the expressions ended with status {n}")),
            Err(e) => Err(e),
        }
    }
}

/// An error, with its source line under it; the session's file is called `repl`.
fn report(e: &str, file: &Path) {
    eprintln!("Error: {}", crate::excerpt::add(e).replace(&file.display().to_string(), "repl"));
}

/// Runs the REPL on standard input and output; the exit status.
pub fn command() -> Result<i32, String> {
    let tty = std::io::stdin().is_terminal();
    let mut session = Session { defs: Vec::new(), file: std::env::temp_dir().join(format!("aipl_repl_{}.aipl", std::process::id())) };
    if tty {
        println!("aipl repl: an expression shows its value, a definition is kept. :help, :quit");
    }
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    let mut pending = String::new();
    loop {
        if tty {
            print!("{}", if pending.is_empty() { "aipl> " } else { "  ... " });
            let _ = std::io::stdout().flush();
        }
        let Some(line) = lines.next() else { break };
        let line = line.map_err(|e| e.to_string())?;
        if pending.is_empty() && line.trim_start().starts_with(':') {
            match line.trim() {
                ":quit" | ":q" | ":exit" => break,
                ":help" | ":h" | ":?" => print!("{HELP}"),
                ":reset" => session.defs.clear(),
                ":show" => {
                    for d in &session.defs {
                        println!("{}", d.text);
                    }
                }
                other => report(&format!("{other}: not a command (:help lists them)"), &session.file),
            }
            continue;
        }
        pending.push_str(&line);
        pending.push('\n');
        let parts = match forms(&pending) {
            Ok(p) => p,
            Err(Scan::Open) => continue,
            Err(Scan::Stray) => {
                report("a bracket closes what was never opened", &session.file);
                pending.clear();
                continue;
            }
            Err(Scan::Done(_)) => unreachable!(),
        };
        let input = std::mem::take(&mut pending);
        if parts.is_empty() {
            continue;
        }
        let keys: Vec<Option<String>> = parts.iter().map(|p| definition_key(p)).collect();
        let result = if keys.iter().all(|k| k.is_some()) {
            session.define(keys.into_iter().flatten().zip(parts).collect())
        } else if keys.iter().any(|k| k.is_some()) {
            Err("a line is either definitions or expressions, not both".to_string())
        } else {
            session.eval(input.trim())
        };
        if let Err(e) = result {
            report(&e, &session.file);
        }
        let _ = std::io::stdout().flush();
    }
    let _ = std::fs::remove_file(&session.file);
    Ok(0)
}
