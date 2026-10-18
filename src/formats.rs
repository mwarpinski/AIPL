//! The formatting forms (AIPL_SPEC.md 4.K): `(format "...")`, `(format_to b
//! "...")`, `(print "...")`, `(println "...")`, `(eprintln "...")`.
//!
//! Three steps, each mirrored in the self-hosted toolchain:
//!
//! 1. `split` (during constant expansion, consts.rs; `aipl_src/fmtsplit.aipl`)
//!    turns the string into pieces: `(println "x = {x:>4}!")` becomes
//!    `(format.println "x = " (format.piece x ">4") "!")`, so the checker sees
//!    the values as ordinary expressions.
//! 2. The checker types each piece and checks its format against the type.
//! 3. `lower` (the resolver, once the program checks; `aipl_src/fmtlower.aipl`)
//!    replaces each form with the `std/show` calls a programmer would write,
//!    chosen by each value's type. No backend sees a format op.
//!
//! A module that uses a form imports `std/show` itself (`uses_formats`).

use crate::ast::{Contract, Expr, Literal, MatchArm, Module, OpCode, Type};
use std::collections::HashMap;
use crate::parser::{Parser, Token, TokenKind};
use crate::sexpr::Sx;

/// The surface forms, and the op each becomes.
const FORMS: [(&str, &str); 5] = [
    ("format", "format.str"),
    ("format_to", "format.to"),
    ("print", "format.print"),
    ("println", "format.println"),
    ("eprintln", "format.eprintln"),
];

/// Whether any item uses a formatting form (so its module needs `std/show`).
/// A `[...]` list (parameters, fields, an enum's members, a union's
/// variants, binders) holds no expression, so `(enum E [(print 1)])` is no form.
pub fn uses_formats(items: &[Sx]) -> bool {
    fn walk(sx: &Sx) -> bool {
        if !sx.is_paren() {
            return false;
        }
        if let Some(h) = sx.head() {
            if FORMS.iter().any(|(f, _)| *f == h) || h == "dbg" {
                return true;
            }
        }
        sx.items().iter().any(walk)
    }
    items.iter().any(walk)
}

/// Step 1, in place over one item: every formatting form's string split into
/// pieces. `err` places a message at a form.
/// `src` is the module's text and `file` its file's name: `(dbg e)` becomes
/// `(format.dbg "file:L:C: e" e)`, e as written there (dbg_label).
pub fn split(sx: &mut Sx, src: &str, file: &str, err: &dyn Fn(&Sx, &str) -> String) -> Result<(), String> {
    // nothing in a [...] list is an expression (uses_formats)
    if !sx.is_paren() {
        return Ok(());
    }
    if let Some(h) = sx.head() {
        if let Some((form, op)) = FORMS.iter().find(|(f, _)| *f == h) {
            let replaced = split_form(sx, form, op, err)?;
            *sx = replaced;
        } else if h == "dbg" {
            let items = sx.items();
            if items.len() != 2 {
                return Err(err(sx, "dbg takes one expression, e.g. (dbg (call f x))"));
            }
            let (l, c) = sx.position();
            let label = format!("{file}:{l}:{c}: {}", form_text(src, items[1].position()));
            *sx = list_at(vec![Sx::symbol_at("format.dbg".into(), &items[0]), string_at(label, sx), items[1].clone()], sx);
        }
    }
    if let Some(items) = sx.items_mut() {
        for i in items {
            split(i, src, file, err)?;
        }
    }
    Ok(())
}

/// The source text of the form at (line, col) (a column counts characters,
/// as the tokenizer does), on one line: comments dropped, each run of
/// whitespace one space, none just inside brackets, strings as written
/// (aipl_src/resolver.aipl dbg_text gives the same).
pub fn form_text(src: &str, (line, col): (u32, u32)) -> String {
    let b = src.as_bytes();
    let mut i = 0;
    let mut l = 1;
    while l < line && i < b.len() {
        if b[i] == b'\n' {
            l += 1;
        }
        i += 1;
    }
    for _ in 1..col {
        i += 1;
        while i < b.len() && b[i] & 0xC0 == 0x80 {
            i += 1;
        }
    }
    let delimiter = |c: u8| c.is_ascii_whitespace() || matches!(c, b'(' | b')' | b'[' | b']' | b'"' | b';');
    let mut out: Vec<u8> = Vec::new();
    if i < b.len() && !matches!(b[i], b'(' | b'[' | b'"') {
        // an atom
        while i < b.len() && !delimiter(b[i]) {
            out.push(b[i]);
            i += 1;
        }
        return String::from_utf8_lossy(&out).into_owned();
    }
    let mut depth = 0;
    let mut space = false;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            space = true;
            i += 1;
            continue;
        }
        if c == b';' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            space = true;
            continue;
        }
        if space && !matches!(out.last(), None | Some(b'(' | b'[')) && !matches!(c, b')' | b']') {
            out.push(b' ');
        }
        space = false;
        match c {
            b'"' => {
                out.push(c);
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' && i + 1 < b.len() {
                        out.push(b[i]);
                        i += 1;
                    }
                    out.push(b[i]);
                    i += 1;
                }
                out.push(b'"');
                i += 1;
            }
            b'(' | b'[' => {
                out.push(c);
                depth += 1;
                i += 1;
            }
            b')' | b']' => {
                out.push(c);
                depth -= 1;
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
        if depth == 0 {
            break;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn split_form(sx: &Sx, form: &str, op: &str, err: &dyn Fn(&Sx, &str) -> String) -> Result<Sx, String> {
    let items = sx.items();
    let to = form == "format_to";
    let shape = || {
        if to {
            "format_to takes a buffer and a string literal, e.g. (format_to b \"n = {n}\")".to_string()
        } else {
            format!("{form} takes one string literal, e.g. ({form} \"n = {{n}}\")")
        }
    };
    let want = if to { 3 } else { 2 };
    let text = match items.get(want - 1) {
        Some(Sx::Atom(Token { kind: TokenKind::StringLit(s), .. })) if items.len() == want => s.clone(),
        _ => return Err(err(sx, &shape())),
    };
    let at = &items[want - 1];
    let mut out = vec![Sx::symbol_at(op.to_string(), &items[0])];
    if to {
        out.push(items[1].clone());
    }
    for piece in pieces(&text).map_err(|m| err(at, &m))? {
        out.push(match piece {
            Piece::Text(t) => string_at(t, at),
            Piece::Value { code, spec } => {
                // a name or literal is one token, an expression one form
                let toks = Parser::tokenize(&code).map_err(|m| err(at, &m))?;
                let mut value = Sx::read_one(&toks).map_err(|m| err(at, &m))?;
                // a dbg is labelled with where it is written, which a placeholder's code is not
                if holds_dbg(&value) {
                    return Err(err(at, "a placeholder cannot hold a dbg; bind the value with let and dbg it there"));
                }
                move_to(&mut value, at.position());
                list_at(vec![Sx::symbol_at("format.piece".into(), at), value, string_at(spec, at)], at)
            }
        });
    }
    Ok(list_at(out, sx))
}

/// Whether sx is or holds a `(dbg ...)`.
fn holds_dbg(sx: &Sx) -> bool {
    sx.is_paren() && (sx.head() == Some("dbg") || sx.items().iter().any(holds_dbg))
}

/// One piece of a format string: text, or a value (a name, or an
/// expression in parentheses, as written) and its format.
#[derive(Debug, PartialEq)]
pub enum Piece {
    Text(String),
    Value { code: String, spec: String },
}

/// A format string's pieces: `{{` and `}}` are braces; `{name}`,
/// `{(expression)}`, and either followed by `:format`, are values.
pub fn pieces(text: &str) -> Result<Vec<Piece>, String> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    let unclosed = "unclosed { in a format string (write {{ for a brace)".to_string();
    let shape = "a placeholder is {name} or {(expression)}, then optionally :format, e.g. {n:>5} or {x:.2}".to_string();
    // what the self-hosted toolchain sees as a quote or a backslash in the
    // raw string (it reads placeholders unescaped)
    let quoted = |c: u8| matches!(c, b'"' | b'\\' | b'\n' | b'\t' | b'\r' | 0);
    let no_string = "a placeholder cannot hold a string or an escape; bind it with let first".to_string();
    while i < b.len() {
        match b[i] {
            b'{' if b.get(i + 1) == Some(&b'{') => {
                lit.push('{');
                i += 2;
            }
            b'}' if b.get(i + 1) == Some(&b'}') => {
                lit.push('}');
                i += 2;
            }
            b'}' => return Err("a } in a format string closes nothing (write }} for a brace)".into()),
            b'{' => {
                let start = i + 1;
                let mut j = start;
                if b.get(j) == Some(&b'(') {
                    let mut depth = 0;
                    while j < b.len() {
                        match b[j] {
                            b'(' => depth += 1,
                            b')' => depth -= 1,
                            c if quoted(c) => return Err(no_string),
                            b'{' | b'}' => return Err(unclosed),
                            _ => {}
                        }
                        j += 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    if depth != 0 {
                        return Err(unclosed);
                    }
                } else {
                    while j < b.len() && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'_' | b'.' | b'+' | b'-')) {
                        j += 1;
                    }
                    if j == start {
                        return Err(match b.get(j) {
                            Some(b'}') => "{} needs a value: {name} or {(expression)}".into(),
                            None => unclosed,
                            _ => shape,
                        });
                    }
                }
                let code = text[start..j].to_string();
                let mut spec = String::new();
                match b.get(j) {
                    Some(b'}') => j += 1,
                    Some(b':') => {
                        j += 1;
                        let s = j;
                        while j < b.len() && b[j] != b'}' {
                            if quoted(b[j]) {
                                return Err(no_string);
                            }
                            if b[j] == b'{' {
                                return Err(shape);
                            }
                            j += 1;
                        }
                        if j == b.len() {
                            return Err(unclosed);
                        }
                        spec = text[s..j].to_string();
                        j += 1;
                    }
                    None => return Err(unclosed),
                    _ => return Err(shape),
                }
                if !lit.is_empty() {
                    out.push(Piece::Text(std::mem::take(&mut lit)));
                }
                out.push(Piece::Value { code, spec });
                i = j;
            }
            _ => {
                // a whole UTF-8 character
                let n = text[i..].chars().next().map_or(1, |c| c.len_utf8());
                lit.push_str(&text[i..i + n]);
                i += n;
            }
        }
    }
    if !lit.is_empty() {
        out.push(Piece::Text(lit));
    }
    Ok(out)
}

fn string_at(s: String, at: &Sx) -> Sx {
    let (line, col) = at.position();
    Sx::Atom(Token { kind: TokenKind::StringLit(s), line, col })
}

fn list_at(items: Vec<Sx>, at: &Sx) -> Sx {
    let (line, col) = at.position();
    Sx::List { open: Token { kind: TokenKind::LParen, line, col }, items, close: Token { kind: TokenKind::RParen, line, col } }
}

/// Every token of `sx` at `pos` (a placeholder's code is reported at its string).
fn move_to(sx: &mut Sx, pos: (u32, u32)) {
    match sx {
        Sx::Atom(t) => (t.line, t.col) = pos,
        Sx::List { open, items, close } => {
            (open.line, open.col) = pos;
            (close.line, close.col) = pos;
            for i in items {
                move_to(i, pos);
            }
        }
    }
}

/// A format, as written after the colon: `[[fill]align][+][0][width][.decimals][x|X|b]`.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Spec {
    pub fill: u8,
    /// 0 the type's default, 1 '<', 2 '>', 3 '^'
    pub align: u8,
    pub plus: bool,
    pub zero: bool,
    pub width: u32,
    pub decimals: Option<u32>,
    /// 0 decimal, 1 'x', 2 'X', 3 'b'
    pub base: u8,
}

impl Spec {
    pub fn parse(text: &str) -> Result<Spec, String> {
        let bad = || format!("bad format '{text}': a format is [[fill]align][+][0][width][.decimals][x|X|b], e.g. {{n:>8}} or {{x:.2}}");
        let b = text.as_bytes();
        let mut s = Spec::default();
        let mut i = 0;
        let align_of = |c: u8| match c {
            b'<' => Some(1),
            b'>' => Some(2),
            b'^' => Some(3),
            _ => None,
        };
        if b.len() >= 2 && align_of(b[1]).is_some() {
            if !(32..127).contains(&b[0]) {
                return Err(bad());
            }
            s.fill = b[0];
            s.align = align_of(b[1]).unwrap();
            i = 2;
        } else if let Some(a) = b.first().and_then(|c| align_of(*c)) {
            s.align = a;
            i = 1;
        }
        if b.get(i) == Some(&b'+') {
            s.plus = true;
            i += 1;
        }
        if b.get(i) == Some(&b'0') {
            s.zero = true;
            i += 1;
        }
        let digits = |i: &mut usize| -> Option<u32> {
            let st = *i;
            while *i < b.len() && b[*i].is_ascii_digit() {
                *i += 1;
            }
            if *i == st {
                None
            } else {
                Some(text[st..*i].parse::<u32>().unwrap_or(u32::MAX))
            }
        };
        if let Some(w) = digits(&mut i) {
            if w > 4095 {
                return Err(format!("bad format '{text}': a width is at most 4095"));
            }
            s.width = w;
        }
        if b.get(i) == Some(&b'.') {
            i += 1;
            let d = digits(&mut i).ok_or_else(bad)?;
            if d > 40 {
                return Err(format!("bad format '{text}': at most 40 decimals"));
            }
            s.decimals = Some(d);
        }
        match b.get(i) {
            Some(b'x') => s.base = 1,
            Some(b'X') => s.base = 2,
            Some(b'b') => s.base = 3,
            _ => {}
        }
        if s.base != 0 {
            i += 1;
        }
        if i != b.len() {
            return Err(bad());
        }
        Ok(s)
    }

    /// std/show's spec word.
    pub fn word(&self) -> i32 {
        (self.width
            | ((self.decimals.map_or(0, |d| d + 1)) << 12)
            | ((self.align as u32) << 18)
            | if self.zero { 1 << 20 } else { 0 }
            | ((self.base as u32) << 21)
            | if self.plus { 1 << 23 } else { 0 }
            | ((self.fill as u32) << 24)) as i32
    }

    /// Why this format does not fit a value of type `t`, if it does not.
    pub fn misfit(&self, t: &Type) -> Option<String> {
        let float = matches!(t, Type::F32 | Type::F64);
        let int = matches!(t, Type::I32 | Type::I64);
        if self.decimals.is_some() && !float {
            return Some(format!("decimals (.N) are for f32 and f64, not {t}"));
        }
        if self.base != 0 && !int {
            return Some(format!("x, X, and b are for i32 and i64, not {t}"));
        }
        if (self.plus || self.zero) && !(float || int) {
            return Some(format!("+ and 0 are for numbers, not {t}"));
        }
        None
    }
}

/// Whether the formatting forms print a value of type `t`.
pub fn printable(t: &Type) -> bool {
    matches!(t, Type::I32 | Type::I64 | Type::F32 | Type::F64 | Type::Bool | Type::Str | Type::Enum(_))
        || matches!(t, Type::Ptr(s) if **s == Type::Struct("str.Bytes".into()))
}

pub fn is_format(op: &OpCode) -> bool {
    matches!(op, OpCode::FormatStr | OpCode::FormatTo | OpCode::FormatPrint | OpCode::FormatPrintln | OpCode::FormatEprintln)
}

/// The ops this module lowers: the formatting forms, dbg, and the early
/// returns.
fn lowered(op: &OpCode) -> bool {
    is_format(op) || matches!(op, OpCode::Try | OpCode::TryOpt | OpCode::TryOr | OpCode::Defer | OpCode::FormatDbg)
}

/// `(call show.put_T chain value spec)`: the call that writes `value`, of
/// type `t`, onto the buffer `chain` gives.
fn put(chain: Expr, value: Expr, t: &Type, spec: i64, enums: &HashMap<String, Vec<String>>, span: (u32, u32)) -> Expr {
    let (func, value) = match t {
        Type::I32 => ("show.put_i32", value),
        Type::I64 => ("show.put_i64", value),
        Type::F32 => ("show.put_f32", value),
        Type::F64 => ("show.put_f64", value),
        Type::Bool => ("show.put_bool", value),
        Type::Str => ("show.put_str", value),
        Type::Enum(name) => {
            // the member's name: (match v (E.a "a") (E.b "b") ...)
            let arms = enums.get(name).into_iter().flatten().map(|m| MatchArm {
                member: format!("{name}.{m}"),
                binders: None,
                body: vec![Expr::Lit(Literal::Str(m.clone()), span)],
                span,
            });
            ("show.put_str", Expr::Match { value: Box::new(value), arms: arms.collect(), else_body: None, span })
        }
        _ => ("show.put_bytes", value),
    };
    Expr::Call { func: func.into(), args: vec![chain, value, Expr::Lit(Literal::Int(spec), span)], span }
}

/// `(format.dbg "label" e)` as `(block (let _dbg_N:T e) (call show.eprintln
/// ...) _dbg_N)`: the label and the value written to standard error, then
/// the value. A pointer or array shows its type and address; a value
/// formatting cannot print, its type. N counts the function's dbgs, inner
/// ones first (aipl_src/fmtlower.aipl lower_dbg writes the same).
fn lower_dbg(mut args: Vec<Expr>, span: (u32, u32), lw: &mut Lower) -> Expr {
    lw.dbgs += 1;
    let v = format!("_dbg_{}", lw.dbgs);
    let t = lw.types.get(&(&args[1] as *const Expr as usize)).cloned().unwrap_or(Type::Void);
    let value = args.remove(1);
    let Expr::Lit(Literal::Str(label), _) = args.remove(0) else { return value };
    let call = |func: &str, args: Vec<Expr>| Expr::Call { func: func.into(), args, span };
    let var = || Expr::Var(v.clone(), span);
    let start = |text: String| call("show.lit", vec![call("show.start", vec![]), Expr::Lit(Literal::Str(text), span)]);
    let chain = match &t {
        p if printable(p) => put(start(format!("{label} = ")), var(), p, 0, lw.enums, span),
        Type::Ptr(_) | Type::Array(_) => {
            let kind = if matches!(t, Type::Ptr(_)) { crate::ast::AddrKind::Ptr } else { crate::ast::AddrKind::Arr };
            let addr = Expr::Unsafe(vec![Expr::Addr { val: Box::new(var()), kind, span }], span);
            put(start(format!("{label} = {} @ ", source_type(&t))), addr, &Type::I32, 0, lw.enums, span)
        }
        _ => start(format!("{label} = <value> : {}", source_type(&t))),
    };
    Expr::Block(vec![Expr::Let { name: v.clone(), ty: t, val: Box::new(value), span }, call("show.eprintln", vec![chain]), var()], span)
}

/// A type as source writes it: instances of generic types, which the flat
/// program names `option.Option<i32>`, as `(option.Option i32)`
/// (aipl_src/fmtlower.aipl source_type writes the same).
pub fn source_type(t: &Type) -> String {
    fn convert(s: &str) -> String {
        let Some(open) = s.find('<') else { return s.to_string() };
        // the matching '>', and the arguments between, split at depth 0
        let (mut depth, mut args, mut from) = (0, Vec::new(), open + 1);
        for (k, c) in s.char_indices().skip(open) {
            match c {
                '<' | '(' | '[' => depth += 1,
                '>' | ')' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        args.push(convert(&s[from..k]));
                        let rest = convert(&s[k + 1..]);
                        let base = &s[..open];
                        // a base after other text (inside "(ptr ...") keeps that text
                        let split = base.rfind([' ', '(', '[']).map_or(0, |p| p + 1);
                        return format!("{}({} {}){rest}", &base[..split], &base[split..], args.join(" "));
                    }
                }
                ',' if depth == 1 => {
                    args.push(convert(&s[from..k]));
                    from = k + 1;
                }
                _ => {}
            }
        }
        s.to_string()
    }
    convert(&t.to_string())
}

/// What lowering a function's body needs: the noted types, the enums'
/// members, the function's return type, and a count for fresh names.
struct Lower<'a> {
    types: &'a HashMap<usize, Type>,
    enums: &'a HashMap<String, Vec<String>>,
    ret: Type,
    tries: u32,
    dbgs: u32,
}

/// Step 3: every formatting form in `module` replaced by `std/show` calls,
/// if the module checks (else it is left for the checker to report).
pub fn lower(module: &mut Module) {
    let mut any = false;
    for f in &module.functions {
        for e in f.body.iter().chain(f.contracts.iter().map(contract_expr)) {
            crate::compiler::wasm::walk_expr(e, &mut |x| {
                if matches!(x, Expr::Op { op, .. } if lowered(op)) {
                    any = true;
                }
            });
        }
    }
    if !any {
        return;
    }
    let Some(types) = crate::checker::TypeChecker::format_types(module) else { return };
    let enums: HashMap<String, Vec<String>> =
        module.enums.iter().map(|e| (e.name.clone(), e.members.iter().map(|(m, _)| m.clone()).collect())).collect();
    for f in &mut module.functions {
        let mut lw = Lower { types: &types, enums: &enums, ret: f.return_type.clone(), tries: 0, dbgs: 0 };
        for e in &mut f.body {
            match e {
                // a defer's statements, lowered where they are
                Expr::Op { op: OpCode::Defer, args, .. } => args.iter_mut().for_each(|a| lower_expr(a, &mut lw)),
                _ => lower_expr(e, &mut lw),
            }
        }
        lower_defers(&mut f.body, &f.return_type);
        for c in &mut f.contracts {
            match c {
                Contract::Requires(e) | Contract::Ensures(e) | Contract::Invariant(e) => lower_expr(e, &mut lw),
            }
        }
    }
}

/// A body's defers, once its tries are lowered: each return after a defer
/// becomes `(block (let _defer_ret_N:R v) D_k ... D_1 (return
/// _defer_ret_N))` (or, returning nothing, the defers and the return), and
/// the body's value `(let _defer_val:R v) D_k ... D_1 _defer_val` (a void
/// body's: the defers after it; one that always leaves: as it is).
fn lower_defers(body: &mut Vec<Expr>, ret: &Type) {
    if !body.iter().any(|e| matches!(e, Expr::Op { op: OpCode::Defer, .. })) {
        return;
    }
    let mut out = Vec::new();
    let mut active: Vec<Expr> = Vec::new();
    let mut rets = 0;
    for mut e in std::mem::take(body) {
        if let Expr::Op { op: OpCode::Defer, args, .. } = e {
            active.splice(0..0, args.into_iter().rev().collect::<Vec<_>>().into_iter().rev());
            continue;
        }
        if !active.is_empty() {
            wrap_returns(&mut e, &active, ret, &mut rets);
        }
        out.push(e);
    }
    if !active.is_empty() {
        match out.pop() {
            Some(last) if crate::checker::diverges(&last) => out.push(last),
            Some(last) if *ret != Type::Void => {
                let span = last.span();
                out.push(Expr::Let { name: "_defer_val".into(), ty: ret.clone(), val: Box::new(last), span });
                out.extend(active.iter().cloned());
                out.push(Expr::Var("_defer_val".into(), span));
            }
            last => {
                out.extend(last);
                out.extend(active.iter().cloned());
            }
        }
    }
    *body = out;
}

/// Every return in e, the defers `active` (the last defer's first) run first.
fn wrap_returns(e: &mut Expr, active: &[Expr], ret: &Type, rets: &mut u32) {
    if let Expr::Return { val, span } = e {
        let span = *span;
        // numbered as met in the text: this return before any in its value
        let mut block = Vec::new();
        let back = match val.take() {
            Some(mut v) => {
                *rets += 1;
                let name = format!("_defer_ret_{}", rets);
                wrap_returns(&mut v, active, ret, rets);
                block.push(Expr::Let { name: name.clone(), ty: ret.clone(), val: v, span });
                Some(Box::new(Expr::Var(name, span)))
            }
            None => None,
        };
        block.extend(active.iter().cloned());
        block.push(Expr::Return { val: back, span });
        *e = Expr::Block(block, span);
        return;
    }
    for c in children_mut(e) {
        wrap_returns(c, active, ret, rets);
    }
}

/// `(try r)` as `(match_result r (ok _try_N _try_N) (err _try_err_N (return
/// (err:R _try_err_N))))`; `(try_opt o)` as `(match o (U.some [_try_N]
/// _try_N) (U.none (return (make RU.none))))`; `(try_or o e)` the same with
/// `(return (err:R e))`. R is the function's ok type, RU its Option.
fn lower_try(op: &OpCode, mut args: Vec<Expr>, span: (u32, u32), lw: &mut Lower) -> Expr {
    lw.tries += 1;
    let (v, x) = (format!("_try_{}", lw.tries), format!("_try_err_{}", lw.tries));
    let operand_type = lw.types.get(&(&args[0] as *const Expr as usize)).cloned().unwrap_or(Type::Void);
    let ret_ok = match &lw.ret {
        Type::ResultType(ok, _) => Some((**ok).clone()),
        _ => None,
    };
    let ret_union = match &lw.ret {
        Type::Union(u) => u.clone(),
        _ => String::new(),
    };
    let fail = |e: Expr| Expr::Return { val: Some(Box::new(Expr::Err(Box::new(e), ret_ok.clone(), span))), span };
    let operand = args.remove(0);
    if *op == OpCode::Try {
        return Expr::MatchResult {
            expr: Box::new(operand),
            ok_var: v.clone(),
            ok_body: vec![Expr::Var(v, span)],
            err_var: x.clone(),
            err_body: vec![fail(Expr::Var(x, span))],
            span,
        };
    }
    let Type::Union(u) = operand_type else { return operand };
    let none_body = if *op == OpCode::TryOpt {
        Expr::Return { val: Some(Box::new(Expr::Make { union_name: ret_union, variant: "none".into(), args: vec![], span })), span }
    } else {
        fail(args.remove(0))
    };
    Expr::Match {
        value: Box::new(operand),
        arms: vec![
            MatchArm { member: format!("{u}.some"), binders: Some(vec![v.clone()]), body: vec![Expr::Var(v, span)], span },
            MatchArm { member: format!("{u}.none"), binders: None, body: vec![none_body], span },
        ],
        else_body: None,
        span,
    }
}

fn contract_expr(c: &Contract) -> &Expr {
    match c {
        Contract::Requires(e) | Contract::Ensures(e) | Contract::Invariant(e) => e,
    }
}

fn lower_expr(e: &mut Expr, lw: &mut Lower) {
    let (op, args, span) = match e {
        Expr::Op { op, args, span } if lowered(op) => (op.clone(), args, *span),
        _ => {
            for c in children_mut(e) {
                lower_expr(c, lw);
            }
            return;
        }
    };
    if op == OpCode::FormatDbg {
        // the operand first (a dbg inside it is numbered before this one)
        for a in args.iter_mut() {
            lower_expr(a, lw);
        }
        let args = std::mem::take(args);
        *e = lower_dbg(args, span, lw);
        return;
    }
    if !is_format(&op) {
        // the operands first (a try inside a try's operand), keeping the
        // operand's address until its type is read
        for a in args.iter_mut() {
            lower_expr(a, lw);
        }
        let args = std::mem::take(args);
        *e = lower_try(&op, args, span, lw);
        return;
    }
    let (types, enums) = (lw.types, lw.enums);
    // each piece's type, by its value's address, before anything moves
    let piece_types: Vec<Option<Type>> = args
        .iter()
        .map(|p| match p {
            Expr::Op { op: OpCode::FormatPiece, args: pa, .. } => Some(types.get(&(&pa[0] as *const Expr as usize)).cloned().unwrap_or(Type::Void)),
            _ => None,
        })
        .collect();
    let target_type = if op == OpCode::FormatTo { args.first().and_then(|b| types.get(&(b as *const Expr as usize))).cloned() } else { None };
    let call = |func: &str, args: Vec<Expr>| Expr::Call { func: func.into(), args, span };
    let mut parts = std::mem::take(args);
    for p in parts.iter_mut() {
        match p {
            Expr::Op { op: OpCode::FormatPiece, args: pa, .. } => lower_expr(&mut pa[0], lw),
            other => lower_expr(other, lw),
        }
    }
    let mut iter = parts.into_iter().zip(piece_types);
    // a writer's text goes to its buffer, then (call stream.format_end w b)
    let mut writer = None;
    let mut chain = if op == OpCode::FormatTo {
        let b = iter.next().map(|(b, _)| b).unwrap();
        if target_type == Some(Type::Ptr(Box::new(Type::Struct("stream.Writer".into())))) {
            writer = Some(b.clone());
            call("stream.buffer", vec![b])
        } else {
            b
        }
    } else {
        call("show.start", vec![])
    };
    for (p, t) in iter {
        chain = match (p, t) {
            (Expr::Op { args: mut pa, .. }, Some(t)) => {
                let spec = match pa.pop() {
                    Some(Expr::Lit(Literal::Str(s), _)) => Spec::parse(&s).map(|s| s.word()).unwrap_or(0),
                    _ => 0,
                };
                let value = pa.pop().unwrap();
                put(chain, value, &t, spec as i64, enums, span)
            }
            (lit, _) => call("show.lit", vec![chain, lit]),
        };
    }
    *e = match op {
        OpCode::FormatStr => call("show.done", vec![chain]),
        OpCode::FormatTo => match writer {
            Some(w) => call("stream.format_end", vec![w, chain]),
            None => call("show.end_to", vec![chain]),
        },
        OpCode::FormatPrint => call("show.print", vec![chain]),
        OpCode::FormatPrintln => call("show.println", vec![chain]),
        _ => call("show.eprintln", vec![chain]),
    };
}

/// An expression's direct subexpressions, to change in place.
fn children_mut(e: &mut Expr) -> Vec<&mut Expr> {
    match e {
        Expr::Lit(..) | Expr::Var(..) | Expr::NewStruct { .. } | Expr::Sizeof { .. } | Expr::Null { .. } | Expr::Ref { .. } => vec![],
        Expr::Break(_) | Expr::Continue(_) => vec![],
        Expr::Let { val, .. } | Expr::Set { val, .. } => vec![&mut **val],
        Expr::If { cond, then_branch, else_branch, .. } => vec![&mut **cond, &mut **then_branch, &mut **else_branch],
        Expr::Loop { start, end, step, body, .. } => {
            let mut v: Vec<&mut Expr> = vec![&mut **start, &mut **end, &mut **step];
            v.extend(body.iter_mut());
            v
        }
        Expr::While { cond, body, .. } => {
            let mut v: Vec<&mut Expr> = vec![&mut **cond];
            v.extend(body.iter_mut());
            v
        }
        Expr::Call { args, .. } | Expr::Op { args, .. } | Expr::Make { args, .. } | Expr::Record { args, .. } => args.iter_mut().collect(),
        Expr::Block(exprs, _) | Expr::Unsafe(exprs, _) => exprs.iter_mut().collect(),
        Expr::Ok(inner, _, _) | Expr::Err(inner, _, _) => vec![&mut **inner],
        Expr::MatchResult { expr, ok_body, err_body, .. } => {
            let mut v: Vec<&mut Expr> = vec![&mut **expr];
            v.extend(ok_body.iter_mut());
            v.extend(err_body.iter_mut());
            v
        }
        Expr::With { value, new_val, .. } => vec![&mut **value, &mut **new_val],
        Expr::Match { value, arms, else_body, .. } => {
            let mut v: Vec<&mut Expr> = vec![&mut **value];
            for a in arms.iter_mut() {
                v.extend(a.body.iter_mut());
            }
            if let Some(eb) = else_body {
                v.extend(eb.iter_mut());
            }
            v
        }
        Expr::Return { val, .. } => val.iter_mut().map(|v| &mut **v).collect(),
        Expr::CallRef { func, args, .. } => {
            let mut v: Vec<&mut Expr> = vec![&mut **func];
            v.extend(args.iter_mut());
            v
        }
        Expr::GetField { ptr, .. } | Expr::ArrNew { size: ptr, .. } | Expr::ArrLen { arr: ptr, .. } | Expr::Addr { val: ptr, .. } => vec![&mut **ptr],
        Expr::Cast { addr, gen, .. } => {
            let mut v: Vec<&mut Expr> = vec![&mut **addr];
            if let Some(g) = gen {
                v.push(&mut **g);
            }
            v
        }
        Expr::PutField { ptr, val, .. } => vec![&mut **ptr, &mut **val],
        Expr::ArrGet { ptr, index, .. } => vec![&mut **ptr, &mut **index],
        Expr::ArrSet { ptr, index, val, .. } => vec![&mut **ptr, &mut **index, &mut **val],
    }
}
