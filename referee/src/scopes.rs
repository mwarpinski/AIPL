//! A name's scope is its block or match arm (docs/design/LANGUAGE_DECISIONS.md
//! item 5), the reference for the scope half of `aipl_src/generics.aipl`
//! (scope_fn). Runs on each function of the flat program, after generic
//! expansion and before parsing.
//!
//! The checker lets a sibling block or arm declare a name again, with any
//! type. The code generators give each name one local per function, so a
//! name declared again where its first declaration is no longer visible is
//! renamed `'K.name` (its K-th declaration in the function), with every use
//! inside its scope; `shown` writes it back as `name` in a message. A
//! declaration of a name that is still visible keeps that name, so the
//! checker reports the shadowing as before.
//!
//! The scopes are the checker's: a `block`, an `unsafe` block, a `defer`,
//! each branch of an `if`, the body of a `when`, each clause of a `cond`, a
//! `while` body, a `counted_loop`'s variable and body, each `match` arm with
//! its binders, each `match_result` arm with its variable, and each
//! contract. A form's head, a `call`'s or `ref`'s callee, and a `let`'s type
//! are not uses.

use crate::parser::{Token, TokenKind};
use crate::sexpr::Sx;
use std::collections::HashMap;

#[derive(Default)]
struct Scopes {
    /// How many times each name has been declared in the function.
    seen: HashMap<String, u32>,
    /// The names visible, innermost scope last: as written -> as renamed.
    stack: Vec<HashMap<String, String>>,
}

impl Scopes {
    fn visible(&self, n: &str) -> Option<&String> {
        self.stack.iter().rev().find_map(|s| s.get(n))
    }

    fn declare(&mut self, atom: &mut Sx) {
        let Sx::Atom(Token { kind: TokenKind::Symbol(n), .. }) = atom else { return };
        if let Some(v) = self.visible(n) {
            *n = v.clone();
            return;
        }
        let k = self.seen.entry(n.clone()).or_insert(0);
        *k += 1;
        let renamed = if *k == 1 { n.clone() } else { format!("'{}.{}", k, n) };
        self.stack.last_mut().unwrap().insert(n.clone(), renamed.clone());
        *n = renamed;
    }

    fn scoped(&mut self, items: &mut [Sx]) {
        self.stack.push(HashMap::new());
        for i in items {
            self.walk(i);
        }
        self.stack.pop();
    }

    fn walk(&mut self, sx: &mut Sx) {
        if let Sx::Atom(Token { kind: TokenKind::Symbol(s), .. }) = sx {
            if let Some(v) = self.visible(s) {
                *s = v.clone();
            }
            return;
        }
        if !sx.is_paren() {
            for i in sx.items_mut().into_iter().flatten() {
                self.walk(i);
            }
            return;
        }
        let head = sx.head().map(str::to_string);
        let items = sx.items_mut().unwrap();
        match head.as_deref() {
            Some("let") if items.len() >= 5 && matches!(items[2], Sx::Atom(Token { kind: TokenKind::Colon, .. })) => {
                for i in &mut items[4..] {
                    self.walk(i);
                }
                self.declare(&mut items[1]);
            }
            Some("block" | "unsafe" | "defer" | "req" | "ens" | "inv") => self.scoped(&mut items[1..]),
            Some("if") if items.len() > 1 => {
                self.walk(&mut items[1]);
                for k in 2..items.len() {
                    self.scoped(&mut items[k..k + 1]);
                }
            }
            Some("when" | "while") if items.len() > 1 => {
                self.walk(&mut items[1]);
                self.scoped(&mut items[2..]);
            }
            Some("counted_loop") if items.len() >= 5 => {
                for i in &mut items[2..5] {
                    self.walk(i);
                }
                self.stack.push(HashMap::new());
                self.declare(&mut items[1]);
                for i in &mut items[5..] {
                    self.walk(i);
                }
                self.stack.pop();
            }
            Some("cond") => {
                for clause in &mut items[1..] {
                    let Some(parts) = clause.items_mut() else { continue };
                    if parts.is_empty() {
                        continue;
                    }
                    if parts[0].symbol() != Some("else") {
                        self.walk(&mut parts[0]);
                    }
                    self.scoped(&mut parts[1..]);
                }
            }
            Some("match") if items.len() > 1 => {
                self.walk(&mut items[1]);
                for arm in &mut items[2..] {
                    let Some(parts) = arm.items_mut().filter(|p| !p.is_empty()) else { continue };
                    // (V.x [binders] body...), (V.x body...), (else body...)
                    let binders = parts.len() > 1 && matches!(&parts[1], Sx::List { open: Token { kind: TokenKind::LBracket, .. }, .. });
                    self.stack.push(HashMap::new());
                    if binders {
                        for b in parts[1].items_mut().unwrap() {
                            if b.symbol() != Some("_") {
                                self.declare(b);
                            }
                        }
                    }
                    for i in &mut parts[if binders { 2 } else { 1 }..] {
                        self.walk(i);
                    }
                    self.stack.pop();
                }
            }
            Some("match_result") if items.len() > 1 => {
                self.walk(&mut items[1]);
                for arm in &mut items[2..] {
                    let Some(parts) = arm.items_mut() else { continue };
                    if parts.len() < 2 {
                        continue;
                    }
                    self.stack.push(HashMap::new());
                    self.declare(&mut parts[1]);
                    for i in &mut parts[2..] {
                        self.walk(i);
                    }
                    self.stack.pop();
                }
            }
            // the callee names a function, not a local
            Some("call" | "ref") => {
                for i in items.iter_mut().skip(2) {
                    self.walk(i);
                }
            }
            Some(_) => {
                for i in items.iter_mut().skip(1) {
                    self.walk(i);
                }
            }
            None => {
                for i in items.iter_mut() {
                    self.walk(i);
                }
            }
        }
    }
}

/// Renames the locals of `(fn name [params] -> R clauses... body...)` that
/// are declared again outside their first declaration's scope.
pub fn rename_locals(f: &mut Sx) {
    let Some(items) = f.items_mut() else { return };
    if items.len() < 5 {
        return;
    }
    let mut sc = Scopes::default();
    sc.stack.push(HashMap::new());
    // parameters: name : type, three by three
    for p in items[2].items().iter().step_by(3) {
        if let Some(n) = p.symbol() {
            sc.seen.insert(n.to_string(), 1);
            sc.stack[0].insert(n.to_string(), n.to_string());
        }
    }
    for i in &mut items[5..] {
        sc.walk(i);
    }
}
