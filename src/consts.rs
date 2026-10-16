//! Named constants and enum members (AIPL_SPEC.md 4.I), the reference for
//! the constant/enum half of `aipl_src/consts.aipl`. Runs on the flat
//! program after generic expansion, before parsing.
//!
//! `(const NAME:T value)` is removed and every use of `NAME` becomes the
//! value, so the checker and both code generators see an ordinary literal.
//! The value is a literal, or a constant expression folded here: other
//! constants (in any order), and `+ - * / % ^ shl shr shru bitand bitor`
//! on `i32`, `i64`, or (`+ - * /`) `f64`, and `and or not` on `bool`,
//! computed as the program would compute them. A folded `f64` is written
//! as its bits, `(f64.reinterpret_i64 Ni64)`, so no toolchain prints a
//! float (`aipl_src/cexpr.aipl` is the self-hosted toolchain's evaluator).
//! An enum member `E.m` becomes `(enum.cast E value)`: the `i32` value with
//! the enum's type, so the checker can keep enums apart. The `(enum ...)`
//! definitions stay for the parser and the checker.

use crate::parser::{Token, TokenKind};
use crate::resolver::{FlatProgram, Item};
use crate::sexpr::Sx;
use std::collections::HashMap;

pub fn expand(prog: FlatProgram) -> Result<FlatProgram, String> {
    let mut defs: HashMap<String, Def> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut enums: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    for item in prog.structs.iter().chain(prog.fns.iter()) {
        match item.sx.head() {
            Some("const") => {
                let (name, def) = read_const(item)?;
                if defs.insert(name.clone(), def).is_some() {
                    return Err(at(item, &item.sx, &format!("constant '{}' is defined twice", name)));
                }
                order.push(name);
            }
            Some("enum") => {
                if let Some((name, members)) = read_enum(&item.sx) {
                    enums.insert(name, members);
                }
            }
            _ => {}
        }
    }
    if defs.is_empty() && enums.is_empty() {
        return Ok(prog);
    }
    // each constant's replacement: its literal, or its folded value
    let mut consts: HashMap<String, Sx> = HashMap::new();
    let mut memo: HashMap<String, Val> = HashMap::new();
    for name in &order {
        let d = &defs[name];
        let sx = match &d.value {
            Sx::Atom(Token { kind: TokenKind::Symbol(_), .. }) | Sx::List { .. } => {
                let v = eval_const(name, &defs, &mut memo, &mut Vec::new())?;
                v.sx(&d.value)
            }
            literal => literal.clone(),
        };
        consts.insert(name.clone(), sx);
    }
    let rewrite = |items: Vec<Item>| -> Result<Vec<Item>, String> {
        let mut out = Vec::new();
        for mut item in items {
            match item.sx.head() {
                Some("const") => continue,
                Some("enum") => {}
                _ => {
                    let file = item.file.clone();
                    substitute(&mut item.sx, &consts, &enums, &|sx, msg| at_file(&file, sx, msg))?;
                }
            }
            out.push(item);
        }
        Ok(out)
    };
    Ok(FlatProgram { name: prog.name, structs: rewrite(prog.structs)?, fns: rewrite(prog.fns)?, files: prog.files })
}

/// A constant as written: its declared type, its value (a literal, a name,
/// or an expression), and where it is.
struct Def {
    ty: String,
    value: Sx,
    file: std::path::PathBuf,
}

/// A folded value.
#[derive(Clone, Copy, PartialEq)]
enum Val {
    I32(i32),
    I64(i64),
    F64(f64),
    Bool(bool),
}

impl Val {
    fn ty(self) -> &'static str {
        match self {
            Val::I32(_) => "i32",
            Val::I64(_) => "i64",
            Val::F64(_) => "f64",
            Val::Bool(_) => "bool",
        }
    }
    /// The value as source, at `at`'s position: a literal, or an f64's bits.
    fn sx(self, at: &Sx) -> Sx {
        let (line, col) = at.position();
        let tok = |kind| Sx::Atom(Token { kind, line, col });
        match self {
            Val::I32(v) => tok(TokenKind::IntLit(v as i64)),
            Val::I64(v) => tok(TokenKind::Int64Lit(v)),
            Val::Bool(b) => tok(TokenKind::BoolLit(b)),
            Val::F64(f) => Sx::List {
                open: Token { kind: TokenKind::LParen, line, col },
                items: vec![tok(TokenKind::Symbol("f64.reinterpret_i64".into())), tok(TokenKind::Int64Lit(f.to_bits() as i64))],
                close: Token { kind: TokenKind::RParen, line, col },
            },
        }
    }
}

/// Constant `name`'s value, folded (memoised); an error names the constant
/// whose own value is wrong, at that value.
fn eval_const(name: &str, defs: &HashMap<String, Def>, memo: &mut HashMap<String, Val>, active: &mut Vec<String>) -> Result<Val, String> {
    if let Some(v) = memo.get(name) {
        return Ok(*v);
    }
    let d = &defs[name];
    let base = name.rsplit('.').next().unwrap_or(name);
    let fail = |msg: String| at_file(&d.file, &d.value, &msg);
    if active.iter().any(|a| a == name) {
        return Err(fail(format!("constant '{}': its value depends on itself", base)));
    }
    active.push(name.to_string());
    let v = eval_expr(&d.value, base, defs, memo, active).map_err(|e| e.unwrap_or_else(&fail))?;
    active.pop();
    if v.ty() != d.ty {
        return Err(fail(format!("constant '{}' is declared {} but its value is {}", base, d.ty, v.ty())));
    }
    memo.insert(name.to_string(), v);
    Ok(v)
}

/// An expression's value. Err(Ok(full message)) is another constant's
/// error, passed on; Err(Err(text)) is this one's, to be placed.
fn eval_expr(e: &Sx, base: &str, defs: &HashMap<String, Def>, memo: &mut HashMap<String, Val>, active: &mut Vec<String>) -> Result<Val, Result<String, String>> {
    let here = |m: String| Err(Err(m));
    match e {
        Sx::Atom(Token { kind, .. }) => match kind {
            TokenKind::IntLit(v) if *v >= i32::MIN as i64 && *v <= i32::MAX as i64 => Ok(Val::I32(*v as i32)),
            TokenKind::IntLit(v) => here(format!("constant '{}': {} does not fit in an i32", base, v)),
            TokenKind::Int64Lit(v) => Ok(Val::I64(*v)),
            TokenKind::FloatLit(f, _) => Ok(Val::F64(*f)),
            TokenKind::BoolLit(b) => Ok(Val::Bool(*b)),
            TokenKind::Symbol(s) if defs.contains_key(s) => eval_const(s, defs, memo, active).map_err(Ok),
            TokenKind::Symbol(s) => here(format!("constant '{}': '{}' is not a constant", base, s)),
            _ => {
                let what = match kind {
                    TokenKind::StringLit(_) => "a str".to_string(),
                    TokenKind::Float32Lit(..) => "an f32".to_string(),
                    other => other.to_string(),
                };
                here(format!("constant '{}': a constant expression holds numbers, bools, and other constants, not {}", base, what))
            }
        },
        Sx::List { items, .. } => {
            let op = items.first().and_then(|h| h.symbol()).unwrap_or("");
            let args: Vec<Val> = items.iter().skip(1).map(|a| eval_expr(a, base, defs, memo, active)).collect::<Result<_, _>>()?;
            fold(op, &args).map_err(|m| Err(format!("constant '{}': {}", base, m)))
        }
    }
}

/// `op` on `args`, as compiled code computes it: i32 and i64 wrap, shifts
/// take their count modulo the width, and `/` and `%` by zero (or `/` of
/// the smallest value by -1) are errors.
fn fold(op: &str, args: &[Val]) -> Result<Val, String> {
    let ops = "+ - * / % ^ shl shr shru bitand bitor on numbers, and and or not on bools";
    if !matches!(op, "+" | "-" | "*" | "/" | "%" | "^" | "shl" | "shr" | "shru" | "bitand" | "bitor" | "and" | "or" | "not") {
        return Err(format!("a constant expression takes {}; not ({} ...)", ops, op));
    }
    let want = if op == "not" { 1 } else { 2 };
    if args.len() != want {
        return Err(format!("{} takes {} operand{}, got {}", op, want, if want == 1 { "" } else { "s" }, args.len()));
    }
    if op == "not" {
        return match args[0] {
            Val::Bool(b) => Ok(Val::Bool(!b)),
            v => Err(format!("not does not apply to {}", v.ty())),
        };
    }
    let (a, b) = (args[0], args[1]);
    if a.ty() != b.ty() {
        return Err(format!("{} needs two operands of one type, got {} and {}", op, a.ty(), b.ty()));
    }
    let bad = || Err(format!("{} does not apply to {}", op, a.ty()));
    let zero = || Err(format!("{} by zero", op));
    Ok(match (a, b) {
        (Val::I32(x), Val::I32(y)) => Val::I32(match op {
            "+" => x.wrapping_add(y),
            "-" => x.wrapping_sub(y),
            "*" => x.wrapping_mul(y),
            "/" | "%" if y == 0 => return zero(),
            "/" if x == i32::MIN && y == -1 => return Err("/ overflows".to_string()),
            "/" => x / y,
            "%" => x.wrapping_rem(y),
            "^" => x ^ y,
            "shl" => x.wrapping_shl(y as u32),
            "shr" => x.wrapping_shr(y as u32),
            "shru" => ((x as u32).wrapping_shr(y as u32)) as i32,
            "bitand" => x & y,
            "bitor" => x | y,
            _ => return bad(),
        }),
        (Val::I64(x), Val::I64(y)) => Val::I64(match op {
            "+" => x.wrapping_add(y),
            "-" => x.wrapping_sub(y),
            "*" => x.wrapping_mul(y),
            "/" | "%" if y == 0 => return zero(),
            "/" if x == i64::MIN && y == -1 => return Err("/ overflows".to_string()),
            "/" => x / y,
            "%" => x.wrapping_rem(y),
            "^" => x ^ y,
            "shl" => x.wrapping_shl(y as u32),
            "shr" => x.wrapping_shr(y as u32),
            "shru" => ((x as u64).wrapping_shr(y as u32)) as i64,
            "bitand" => x & y,
            "bitor" => x | y,
            _ => return bad(),
        }),
        (Val::F64(x), Val::F64(y)) => Val::F64(match op {
            "+" => x + y,
            "-" => x - y,
            "*" => x * y,
            "/" => x / y,
            _ => return bad(),
        }),
        (Val::Bool(x), Val::Bool(y)) => Val::Bool(match op {
            "and" => x && y,
            "or" => x || y,
            _ => return bad(),
        }),
        _ => return bad(),
    })
}

/// `(const NAME : TYPE VALUE)`: the name, and the literal checked against
/// the declared type, or the name or expression to fold.
fn read_const(item: &Item) -> Result<(String, Def), String> {
    let parts = item.sx.items();
    let shape = "a constant is (const NAME:TYPE value), e.g. (const PAGE_SIZE:i32 65536) or (const PAGES:i32 (* 4 PAGE_SIZE))";
    if parts.len() != 5 || !matches!(&parts[2], Sx::Atom(Token { kind: TokenKind::Colon, .. })) {
        return Err(at(item, &item.sx, shape));
    }
    let name = parts[1].symbol().ok_or_else(|| at(item, &item.sx, shape))?.to_string();
    let base = name.rsplit('.').next().unwrap_or(&name);
    let well_formed = base.len() >= 2
        && base.starts_with(|c: char| c.is_ascii_uppercase())
        && base.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    if !well_formed {
        return Err(at(
            item,
            &parts[1],
            &format!("constant '{}' must be named in capitals, at least two characters (A-Z, 0-9, _), e.g. MAX_DEPTH", base),
        ));
    }
    let ty = parts[3].symbol().unwrap_or("");
    let value = match &parts[4] {
        Sx::Atom(t) => t.kind.clone(),
        _ => TokenKind::LParen,
    };
    let def = Def { ty: ty.to_string(), value: parts[4].clone(), file: item.file.clone() };
    // a name or an expression: folded once every constant is known
    if matches!(value, TokenKind::Symbol(_) | TokenKind::LParen) && matches!(ty, "i32" | "i64" | "f64" | "bool") {
        return Ok((name, def));
    }
    let fits = match (ty, &value) {
        ("i32", TokenKind::IntLit(v)) => *v >= i32::MIN as i64 && *v <= i32::MAX as i64,
        ("i64", TokenKind::Int64Lit(_)) | ("f64", TokenKind::FloatLit(..)) => true,
        ("bool", TokenKind::BoolLit(_)) | ("str", TokenKind::StringLit(_)) => true,
        ("i32" | "i64" | "f64" | "bool" | "str", _) => false,
        _ => {
            return Err(at(item, &parts[3], &format!("constant '{}' has type '{}'; a constant is i32, i64, f64, bool, or str", base, ty)))
        }
    };
    if !fits {
        return Err(at(item, &parts[4], &format!("constant '{}' is declared {} but its value is not an {} literal", base, ty, ty)));
    }
    Ok((name, def))
}

/// `(enum NAME [a b (c 10) ...])`: the members and their values, as the
/// parser computes them. None if malformed (the parser reports it).
fn read_enum(sx: &Sx) -> Option<(String, Vec<(String, i64)>)> {
    let parts = sx.items();
    let name = parts.get(1)?.symbol()?.to_string();
    let mut members = Vec::new();
    let mut next = 0i64;
    for m in parts.get(2)?.items() {
        let (n, v) = match m.symbol() {
            Some(n) => (n.to_string(), next),
            None => {
                let pair = m.items();
                match (pair.first().and_then(|p| p.symbol()), pair.get(1)) {
                    (Some(n), Some(Sx::Atom(Token { kind: TokenKind::IntLit(v), .. }))) => (n.to_string(), *v),
                    _ => return None,
                }
            }
        };
        next = v + 1;
        members.push((n, v));
    }
    Some((name, members))
}

fn substitute(
    sx: &mut Sx,
    consts: &HashMap<String, Sx>,
    enums: &HashMap<String, Vec<(String, i64)>>,
    err: &dyn Fn(&Sx, &str) -> String,
) -> Result<(), String> {
    let is_match = sx.head() == Some("match");
    let Sx::List { items, .. } = sx else { return Ok(()) };
    for i in 0..items.len() {
        // a match arm's head names an enum member or union variant for the
        // checker; it stays a name: (Color.red body...)
        if is_match && i >= 2 {
            if let Sx::List { items: arm, .. } = &mut items[i] {
                if !arm.is_empty() {
                    let placeholder = Sx::symbol_at("match-arm".into(), &arm[0]);
                    let head = std::mem::replace(&mut arm[0], placeholder);
                    substitute(&mut items[i], consts, enums, err)?;
                    if let Sx::List { items: arm, .. } = &mut items[i] {
                        arm[0] = head;
                    }
                    continue;
                }
            }
        }
        let replacement = match &items[i] {
            Sx::Atom(Token { kind: TokenKind::Symbol(s), line, col }) => {
                if let Some(value) = consts.get(s) {
                    if matches!(items.get(i + 1), Some(Sx::Atom(Token { kind: TokenKind::Colon, .. }))) {
                        return Err(err(
                            &items[i],
                            &format!("'{}' is a constant; it cannot also name a variable, parameter, or field", s),
                        ));
                    }
                    Some(match value {
                        Sx::Atom(t) => Sx::Atom(Token { kind: t.kind.clone(), line: *line, col: *col }),
                        list => list.clone(),
                    })
                } else if let Some(dot) = s.rfind('.') {
                    match enums.get(&s[..dot]) {
                        Some(members) => {
                            let member = &s[dot + 1..];
                            let value = members.iter().find(|(n, _)| n == member).map(|(_, v)| *v).ok_or_else(|| {
                                err(&items[i], &format!("enum '{}' has no member '{}'", &s[..dot], member))
                            })?;
                            let tok = |kind| Sx::Atom(Token { kind, line: *line, col: *col });
                            Some(Sx::List {
                                open: Token { kind: TokenKind::LParen, line: *line, col: *col },
                                items: vec![
                                    tok(TokenKind::Symbol("enum.cast".into())),
                                    tok(TokenKind::Symbol(s[..dot].to_string())),
                                    tok(TokenKind::IntLit(value)),
                                ],
                                close: Token { kind: TokenKind::RParen, line: *line, col: *col },
                            })
                        }
                        None => None,
                    }
                } else {
                    None
                }
            }
            Sx::List { .. } => {
                substitute(&mut items[i], consts, enums, err)?;
                None
            }
            Sx::Atom(_) => None,
        };
        if let Some(r) = replacement {
            items[i] = r;
        }
    }
    Ok(())
}

fn at(item: &Item, sx: &Sx, msg: &str) -> String {
    at_file(&item.file, sx, msg)
}

fn at_file(file: &std::path::Path, sx: &Sx, msg: &str) -> String {
    let (l, c) = sx.position();
    format!("{}: {}:{}: {}", file.display(), l, c, msg)
}
