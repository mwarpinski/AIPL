//! Named constants and enum members (AIPL_SPEC.md 4.I), the reference for
//! the constant/enum half of `aipl_src/consts.aipl`. Runs on the flat
//! program after generic expansion, before parsing.
//!
//! `(const NAME:T literal)` is removed and every use of `NAME` becomes the
//! literal, so the checker and both code generators see an ordinary literal.
//! An enum member `E.m` becomes `(enum.cast E value)`: the `i32` value with
//! the enum's type, so the checker can keep enums apart. The `(enum ...)`
//! definitions stay for the parser and the checker.

use crate::parser::{Token, TokenKind};
use crate::resolver::{FlatProgram, Item};
use crate::sexpr::Sx;
use std::collections::HashMap;

pub fn expand(prog: FlatProgram) -> Result<FlatProgram, String> {
    let mut consts: HashMap<String, TokenKind> = HashMap::new();
    let mut enums: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    for item in prog.structs.iter().chain(prog.fns.iter()) {
        match item.sx.head() {
            Some("const") => {
                let (name, value) = read_const(item)?;
                if consts.insert(name.clone(), value).is_some() {
                    return Err(at(item, &item.sx, &format!("constant '{}' is defined twice", name)));
                }
            }
            Some("enum") => {
                if let Some((name, members)) = read_enum(&item.sx) {
                    enums.insert(name, members);
                }
            }
            _ => {}
        }
    }
    if consts.is_empty() && enums.is_empty() {
        return Ok(prog);
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
    Ok(FlatProgram { name: prog.name, structs: rewrite(prog.structs)?, fns: rewrite(prog.fns)? })
}

/// `(const NAME : TYPE VALUE)`: the name and the literal, checked against
/// the declared type.
fn read_const(item: &Item) -> Result<(String, TokenKind), String> {
    let parts = item.sx.items();
    let shape = "a constant is (const NAME:TYPE literal), e.g. (const PAGE_SIZE:i32 65536)";
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
    let fits = match (ty, &value) {
        ("i32", TokenKind::IntLit(v)) => *v >= i32::MIN as i64 && *v <= i32::MAX as i64,
        ("i64", TokenKind::Int64Lit(_)) | ("f64", TokenKind::FloatLit(_)) => true,
        ("bool", TokenKind::BoolLit(_)) | ("str", TokenKind::StringLit(_)) => true,
        ("i32" | "i64" | "f64" | "bool" | "str", _) => false,
        _ => {
            return Err(at(item, &parts[3], &format!("constant '{}' has type '{}'; a constant is i32, i64, f64, bool, or str", base, ty)))
        }
    };
    if !fits {
        return Err(at(item, &parts[4], &format!("constant '{}' is declared {} but its value is not an {} literal", base, ty, ty)));
    }
    Ok((name, value))
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
    consts: &HashMap<String, TokenKind>,
    enums: &HashMap<String, Vec<(String, i64)>>,
    err: &dyn Fn(&Sx, &str) -> String,
) -> Result<(), String> {
    let Sx::List { items, .. } = sx else { return Ok(()) };
    for i in 0..items.len() {
        let replacement = match &items[i] {
            Sx::Atom(Token { kind: TokenKind::Symbol(s), line, col }) => {
                if let Some(value) = consts.get(s) {
                    if matches!(items.get(i + 1), Some(Sx::Atom(Token { kind: TokenKind::Colon, .. }))) {
                        return Err(err(
                            &items[i],
                            &format!("'{}' is a constant; it cannot also name a variable, parameter, or field", s),
                        ));
                    }
                    Some(Sx::Atom(Token { kind: value.clone(), line: *line, col: *col }))
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
