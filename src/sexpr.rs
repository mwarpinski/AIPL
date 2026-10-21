//! S-expressions over the parser's tokens. The resolver and the generics
//! expander work at this level (as `aipl_src/resolver.aipl` and
//! `aipl_src/generics.aipl` do), before the typed parser sees a concrete
//! program; every atom keeps the position of the token it came from.

use crate::parser::{Token, TokenKind};

#[derive(Debug, Clone, PartialEq)]
pub enum Sx {
    Atom(Token),
    /// `(...)` or `[...]`; `open` and `close` are the bracket tokens.
    List { open: Token, items: Vec<Sx>, close: Token },
}

/// What a bracket error says when a string literal ends inside a placeholder.
const OPEN_PLACEHOLDER: &str =
    "this string ends inside a placeholder whose { is never closed: a placeholder cannot hold a string literal; bind the value with let first";

/// Whether a string's last `{` placeholder is never closed (`{{` and `}}`
/// are braces, not placeholders).
fn ends_in_placeholder(s: &str) -> bool {
    let b = s.as_bytes();
    let (mut open, mut i) = (false, 0);
    while i < b.len() {
        match b[i] {
            b'{' if !open && b.get(i + 1) == Some(&b'{') => i += 1,
            b'{' => open = true,
            b'}' if open => open = false,
            b'}' if b.get(i + 1) == Some(&b'}') => i += 1,
            _ => {}
        }
        i += 1;
    }
    open
}

impl Sx {
    /// Reads one form from `tokens` starting at `*pos`.
    pub fn read(tokens: &[Token], pos: &mut usize) -> Result<Sx, String> {
        let tok = tokens.get(*pos).ok_or("unexpected end of input")?.clone();
        *pos += 1;
        match tok.kind {
            TokenKind::LParen | TokenKind::LBracket => {
                let closer = if tok.kind == TokenKind::LParen { TokenKind::RParen } else { TokenKind::RBracket };
                let mut items = Vec::new();
                loop {
                    match tokens.get(*pos) {
                        None => return Err(format!("{}:{}: this bracket is never closed", tok.line, tok.col)),
                        Some(t) if t.kind == closer => {
                            let close = t.clone();
                            *pos += 1;
                            return Ok(Sx::List { open: tok, items, close });
                        }
                        Some(t) if matches!(t.kind, TokenKind::RParen | TokenKind::RBracket) => {
                            return Err(format!("{}:{}: mismatched closing bracket", t.line, t.col));
                        }
                        Some(_) => items.push(Sx::read(tokens, pos)?),
                    }
                }
            }
            TokenKind::RParen | TokenKind::RBracket => Err(format!("{}:{}: unexpected closing bracket", tok.line, tok.col)),
            _ => Ok(Sx::Atom(tok)),
        }
    }

    /// The whole input as one form; anything after it is an error (as in the parser).
    /// A bracket error in a program with a string that ends inside a
    /// placeholder says so instead: the quote that ended the string is
    /// what unbalanced the brackets (aipl_src/resolver.aipl bracket_error).
    pub fn read_one(tokens: &[Token]) -> Result<Sx, String> {
        let read = || {
            let mut pos = 0;
            let sx = Sx::read(tokens, &mut pos)?;
            if let Some(t) = tokens.get(pos) {
                return Err(format!("{}:{}: unexpected input after the module's closing ')'", t.line, t.col));
            }
            Ok(sx)
        };
        read().map_err(|e| match tokens.iter().find(|t| matches!(&t.kind, TokenKind::StringLit(s) if ends_in_placeholder(s))) {
            Some(t) => format!("{}:{}: {}", t.line, t.col, OPEN_PLACEHOLDER),
            None => e,
        })
    }

    /// Appends this form's tokens to `out`.
    pub fn flatten(&self, out: &mut Vec<Token>) {
        match self {
            Sx::Atom(t) => out.push(t.clone()),
            Sx::List { open, items, close } => {
                out.push(open.clone());
                for i in items {
                    i.flatten(out);
                }
                out.push(close.clone());
            }
        }
    }

    pub fn symbol(&self) -> Option<&str> {
        match self {
            Sx::Atom(Token { kind: TokenKind::Symbol(s), .. }) => Some(s),
            _ => None,
        }
    }

    /// A `(...)` list (not `[...]`).
    pub fn is_paren(&self) -> bool {
        matches!(self, Sx::List { open: Token { kind: TokenKind::LParen, .. }, .. })
    }

    pub fn items(&self) -> &[Sx] {
        match self {
            Sx::List { items, .. } => items,
            Sx::Atom(_) => &[],
        }
    }

    pub fn items_mut(&mut self) -> Option<&mut Vec<Sx>> {
        match self {
            Sx::List { items, .. } => Some(items),
            Sx::Atom(_) => None,
        }
    }

    /// The head symbol of a `(...)` list.
    pub fn head(&self) -> Option<&str> {
        if self.is_paren() {
            self.items().first().and_then(|h| h.symbol())
        } else {
            None
        }
    }

    pub fn position(&self) -> (u32, u32) {
        match self {
            Sx::Atom(t) | Sx::List { open: t, .. } => (t.line, t.col),
        }
    }

    /// Where the first `(name ...)` form in this one is (itself included),
    /// not looking inside `[...]` lists, which hold no expressions.
    pub fn find_head(&self, name: &str) -> Option<(u32, u32)> {
        if !self.is_paren() {
            return None;
        }
        if self.head() == Some(name) {
            return Some(self.position());
        }
        self.items().iter().find_map(|i| i.find_head(name))
    }

    /// A symbol atom with `text` at the position of `at`.
    pub fn symbol_at(text: String, at: &Sx) -> Sx {
        let (line, col) = at.position();
        Sx::Atom(Token { kind: TokenKind::Symbol(text), line, col })
    }

    /// Rewrites every symbol atom in place.
    pub fn map_symbols(&mut self, f: &mut dyn FnMut(&mut String)) {
        match self {
            Sx::Atom(Token { kind: TokenKind::Symbol(s), .. }) => f(s),
            Sx::Atom(_) => {}
            Sx::List { items, .. } => {
                for i in items {
                    i.map_symbols(f);
                }
            }
        }
    }
}
