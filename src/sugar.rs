//! Three forms written for the reader, rewritten as the resolver reads a
//! module (AIPL_SPEC.md 4.M), so nothing after it sees them:
//!
//! - `(when c body...)` is `(if c (block body...) (block))`;
//! - `(for i start end body...)` is `(loop i start (- end 1) 1 body...)`: i
//!   from start up to, but not including, end;
//! - `(and a b c ...)` is `(and a (and b c ...))`, and `or` the same;
//! - `(new S a)` is `(call (alloc.create S) a)` and `(arr.new T a n)` is
//!   `(call (alloc.array T) a n)`: every allocation names its allocator
//!   (docs/design/MEMORY_PLAN.md decision 5), and a module that uses them
//!   imports std/alloc.
//!
//! Every list made here is at the form's place, as aipl_src/resolver.aipl
//! writes it, so both toolchains give the same positions.

use crate::parser::{Token, TokenKind};
use crate::sexpr::Sx;

fn list_at(items: Vec<Sx>, at: (u32, u32)) -> Sx {
    let (line, col) = at;
    Sx::List { open: Token { kind: TokenKind::LParen, line, col }, items, close: Token { kind: TokenKind::RParen, line, col } }
}

fn symbol_at(s: &str, at: (u32, u32)) -> Sx {
    Sx::Atom(Token { kind: TokenKind::Symbol(s.to_string()), line: at.0, col: at.1 })
}

fn int_at(v: i64, at: (u32, u32)) -> Sx {
    Sx::Atom(Token { kind: TokenKind::IntLit(v), line: at.0, col: at.1 })
}

/// The message for `(new S)` and `(arr.new T n)`, which name no allocator.
pub const NEW_NEEDS_ALLOCATOR: &str = "new takes the allocator to make it from: (new S a), e.g. (new S (call alloc.default)), or (new S (call alloc.forever)) for memory kept until the program exits";
pub const ARR_NEW_NEEDS_ALLOCATOR: &str = "arr.new takes the allocator to make it from: (arr.new T a n), e.g. (arr.new T (call alloc.default) n), or (arr.new T (call alloc.forever) n) for memory kept until the program exits";

/// Rewrites the forms in `sx`, inner ones first; `err` places a message at
/// a form. A `[...]` list holds no expression and is left alone. `alloc`
/// is how this module names std/alloc's functions (`alloc.`, an alias's
/// prefix, or none in std/alloc itself), or why it cannot use new; `used`
/// is set when a new or arr.new is rewritten.
pub fn rewrite(sx: &mut Sx, alloc: &Result<String, String>, used: &mut bool, err: &dyn Fn(&Sx, &str) -> String) -> Result<(), String> {
    if !sx.is_paren() {
        return Ok(());
    }
    // a form's shape first, so the first mistake in the text is reported
    let n = sx.items().len();
    match sx.head() {
        Some("new" | "arr.new") if alloc.is_err() => return Err(err(sx, alloc.as_ref().unwrap_err())),
        Some("new") if n == 2 => return Err(err(sx, NEW_NEEDS_ALLOCATOR)),
        Some("new") if n != 3 => return Err(err(sx, "new is (new S a): the struct, then the allocator to make it from")),
        Some("arr.new") if n == 3 => return Err(err(sx, ARR_NEW_NEEDS_ALLOCATOR)),
        Some("arr.new") if n != 4 => return Err(err(sx, "arr.new is (arr.new T a n): the element type, the allocator to make the array from, then its length")),
        Some("when") if n < 2 => return Err(err(sx, "when takes a condition and a body: (when c body...)")),
        Some("for") if n < 4 || sx.items()[1].symbol().is_none() => {
            return Err(err(sx, "for is (for i start end body...): i from start up to end, end left out"))
        }
        _ => {}
    }
    if let Some(items) = sx.items_mut() {
        for i in items {
            rewrite(i, alloc, used, err)?;
        }
    }
    let at = sx.position();
    let items = sx.items().to_vec();
    match sx.head() {
        Some("when") => {
            let mut block = vec![symbol_at("block", at)];
            block.extend(items[2..].iter().cloned());
            *sx = list_at(vec![symbol_at("if", at), items[1].clone(), list_at(block, at), list_at(vec![symbol_at("block", at)], at)], at);
        }
        Some("for") => {
            let end = list_at(vec![symbol_at("-", at), items[3].clone(), int_at(1, at)], at);
            let mut out = vec![symbol_at("loop", at), items[1].clone(), items[2].clone(), end, int_at(1, at)];
            out.extend(items[4..].iter().cloned());
            *sx = list_at(out, at);
        }
        Some(form @ ("new" | "arr.new")) => {
            let prefix = alloc.as_ref().unwrap_or_else(|_| unreachable!());
            let f = if form == "new" { "create" } else { "array" };
            let mut out = vec![symbol_at("call", at), list_at(vec![symbol_at(&format!("{prefix}{f}"), at), items[1].clone()], at)];
            out.extend(items[2..].iter().cloned());
            *sx = list_at(out, at);
            *used = true;
        }
        Some(op @ ("and" | "or")) if items.len() > 3 => {
            // right-nested, as (and a (and b c))
            let mut tail = list_at(vec![symbol_at(op, at), items[items.len() - 2].clone(), items[items.len() - 1].clone()], at);
            for x in items[2..items.len() - 2].iter().rev() {
                tail = list_at(vec![symbol_at(op, at), x.clone(), tail], at);
            }
            *sx = list_at(vec![items[0].clone(), items[1].clone(), tail], at);
        }
        _ => {}
    }
    Ok(())
}
