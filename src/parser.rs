use crate::ast::*;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    LParen,
    RParen,
    LBracket,
    RBracket,
    Colon,
    Arrow,
    Symbol(String),
    IntLit(i64),
    Int64Lit(i64),
    /// the value and the literal as written (messages show tokens as written)
    FloatLit(f64, String),
    /// `1.5f32`: the value and the literal as written
    Float32Lit(f32, String),
    StringLit(String),
    BoolLit(bool),
}

/// A token as AIPL source writes it, for messages: `(`, `->`, `x`, `42`,
/// `42i64`, `1.5e3` (as written), `true`, `"a\n"` (escapes as AIPL's).
impl std::fmt::Display for TokenKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenKind::LParen => f.write_str("("),
            TokenKind::RParen => f.write_str(")"),
            TokenKind::LBracket => f.write_str("["),
            TokenKind::RBracket => f.write_str("]"),
            TokenKind::Colon => f.write_str(":"),
            TokenKind::Arrow => f.write_str("->"),
            TokenKind::Symbol(s) => f.write_str(s),
            TokenKind::IntLit(v) => write!(f, "{v}"),
            TokenKind::Int64Lit(v) => write!(f, "{v}i64"),
            TokenKind::FloatLit(_, text) => f.write_str(text),
            TokenKind::Float32Lit(_, text) => f.write_str(text),
            TokenKind::BoolLit(b) => write!(f, "{b}"),
            TokenKind::StringLit(s) => {
                f.write_str("\"")?;
                for c in s.chars() {
                    match c {
                        '"' => f.write_str("\\\"")?,
                        '\\' => f.write_str("\\\\")?,
                        '\n' => f.write_str("\\n")?,
                        '\t' => f.write_str("\\t")?,
                        '\r' => f.write_str("\\r")?,
                        '\0' => f.write_str("\\0")?,
                        c => write!(f, "{c}")?,
                    }
                }
                f.write_str("\"")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: u32,
    pub col: u32,
}

/// Every operator and keyword that can head a form `(head ...)`, for the
/// "did you mean" in the unknown-head error. aipl_src/parser.aipl's
/// form_table and op_table list the same names (tests/test_checker_aipl.rs
/// compares them).
/// An operator as C writes it, which AIPL spells as a form: what it means,
/// and the form (aipl_src/parser.aipl c_operator alike).
pub fn c_operator(op: &str) -> Option<(&'static str, &'static str)> {
    Some(match op {
        "=" | "==" => ("equality", "(eq a b)"),
        "!=" => ("inequality", "(neq a b)"),
        "<" => ("less than", "(lt a b)"),
        ">" => ("greater than", "(gt a b)"),
        "<=" => ("at most", "(lte a b)"),
        ">=" => ("at least", "(gte a b)"),
        "&&" => ("and", "(and a b)"),
        "||" => ("or", "(or a b)"),
        "!" => ("not", "(not a)"),
        _ => return None,
    })
}

pub const HEAD_NAMES: &[&str] = &[
    "when", "%", "*", "+", "-", "/", "^", "and", "args.get", "args.sizes", "arr.addr", "arr.cast",
    "arr.gen", "arr.get", "arr.len", "arr.make", "arr.new", "arr.null", "arr.of", "arr.set", "atomic.add",
    "atomic.cas", "atomic.lock", "atomic.unlock", "bitand", "bitor", "block", "break", "call",
    "call_ref", "checked.add", "checked.mul", "checked.sub", "cond", "continue", "dbg", "defer",
    "divu", "enum.cast", "enum.ord", "env.get", "env.sizes", "eprintln", "eq", "err", "expect",
    "f32.convert_i32_s", "f32.convert_i64_s", "f32.demote", "f32.reinterpret_i32",
    "f64.convert_i32_s", "f64.convert_i64_s", "f64.promote", "f64.reinterpret_i64", "f64.sqrt",
    "format", "format.dbg", "format.eprintln", "format.expect", "format.piece", "format.print",
    "format.println", "format.str", "format.to", "format_to", "fs.chmod", "fs.close", "fs.delete",
    "fs.open", "fs.opendir", "fs.read", "fs.readdir", "fs.rename", "fs.rmdir", "fs.stat", "fs.write", "get", "gt", "gte",
    "gteu", "gtu", "i32.reinterpret_f32", "i32.trunc_f32_s", "i32.trunc_f64_s", "i32.wrap",
    "i64.extend_s", "i64.extend_u", "i64.reinterpret_f64", "i64.trunc_f32_s", "i64.trunc_f64_s",
    "for", "if", "let", "counted_loop", "lt", "lte", "lteu", "ltu", "make", "match", "match_result",
    "mem.alloc", "mem.free", "mem.grow", "mem.load16_s", "mem.load16_u", "mem.load32",
    "mem.load64", "mem.load8", "mem.load8_s", "mem.load_f32", "mem.load_f64", "mem.store16",
    "mem.store32", "mem.store64", "mem.store8", "mem.store_f32", "mem.store_f64", "neq",
    "net.accept", "net.close", "net.listen", "net.recv", "net.send", "new", "not", "ok", "or",
    "print", "println", "proc.close", "proc.exec", "proc.read", "proc.spawn", "proc.stream", "proc.wait",
    "proc.write", "ptr.addr", "ptr.cast", "ptr.gen", "ptr.make", "ptr.null", "put", "ref",
    "remu", "return", "set!", "shl", "shr", "shru", "sizeof", "str.len", "str.ptr", "sys.exit",
    "sys.monotonic", "sys.print", "sys.random", "sys.time", "thread.join", "thread.spawn",
    "try", "try_opt", "try_or", "unsafe", "while", "with", "zero",
];

/// Edit distance between two names, by bytes (aipl_src/suggest.aipl is the
/// same rule).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, &ca) in a.iter().enumerate() {
        let mut diag = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = (diag + usize::from(ca != cb)).min(up + 1).min(row[j] + 1);
            diag = up;
        }
    }
    row[b.len()]
}

/// The candidate nearest to a misspelled name, if one is near enough to be
/// worth naming. Underscores are left out of both names before they are
/// compared (`lt_u` is `ltu`); then at most 1 edit for a name of 3 to 5
/// bytes, 2 for a longer one, none for a shorter one. The closest wins; of
/// equal ones, the one that begins with more of the name as written (`set`
/// is `set!`, not `get`), then the one that sorts first (aipl_src/suggest.aipl
/// is the same rule, so both toolchains name the same one).
pub fn nearest_name<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let squeeze = |s: &str| s.replace('_', "");
    let plain = squeeze(name);
    let limit = match plain.len() {
        0..=2 => return None,
        3..=5 => 1,
        _ => 2,
    };
    let shared = |c: &str| name.bytes().zip(c.bytes()).take_while(|(a, b)| a == b).count();
    candidates
        .filter(|c| *c != name)
        .map(|c| (edit_distance(&plain, &squeeze(c)), std::cmp::Reverse(shared(c)), c))
        .filter(|(d, _, _)| *d <= limit)
        .min()
        .map(|(_, _, c)| c)
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    last_pos: (u32, u32),
    /// Names of the program's unions: a bare type name is a union if listed,
    /// otherwise an enum (the checker reports unknown names).
    unions: std::collections::HashSet<String>,
    /// Names of the program's value structs: a bare type name listed here is
    /// the struct itself, and `(make NAME ...)` makes one.
    values: std::collections::HashSet<String>,
}

/// The value structs declared in `tokens`: `( struct NAME [ ... ] ( value ) )`.
pub fn value_struct_names(tokens: &[Token]) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for i in 0..tokens.len() {
        let (TokenKind::LParen, Some(TokenKind::Symbol(kw)), Some(TokenKind::Symbol(name)), Some(TokenKind::LBracket)) =
            (&tokens[i].kind, tokens.get(i + 1).map(|t| &t.kind), tokens.get(i + 2).map(|t| &t.kind), tokens.get(i + 3).map(|t| &t.kind))
        else {
            continue;
        };
        if kw != "struct" {
            continue;
        }
        // the field list's close, then (value)
        let mut depth = 0i32;
        let mut j = i + 3;
        while j < tokens.len() {
            match tokens[j].kind {
                TokenKind::LBracket | TokenKind::LParen => depth += 1,
                TokenKind::RBracket | TokenKind::RParen => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        if matches!(tokens.get(j + 1).map(|t| &t.kind), Some(TokenKind::LParen))
            && matches!(tokens.get(j + 2).map(|t| &t.kind), Some(TokenKind::Symbol(v)) if v == "value")
            && matches!(tokens.get(j + 3).map(|t| &t.kind), Some(TokenKind::RParen))
        {
            out.insert(name.clone());
        }
    }
    out
}

impl Parser {
    pub fn tokenize(input: &str) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();
        let mut chars = input.chars().peekable();
        let mut line = 1u32;
        let mut col = 1u32;

        while let Some(&ch) = chars.peek() {
            let start_line = line;
            let start_col = col;

            match ch {
                ' ' | '\t' | '\r' => {
                    chars.next();
                    col += 1;
                }
                '\n' => {
                    chars.next();
                    line += 1;
                    col = 1;
                }
                '(' => {
                    chars.next();
                    col += 1;
                    tokens.push(Token {
                        kind: TokenKind::LParen,
                        line: start_line,
                        col: start_col,
                    });
                }
                ')' => {
                    chars.next();
                    col += 1;
                    tokens.push(Token {
                        kind: TokenKind::RParen,
                        line: start_line,
                        col: start_col,
                    });
                }
                '[' => {
                    chars.next();
                    col += 1;
                    tokens.push(Token {
                        kind: TokenKind::LBracket,
                        line: start_line,
                        col: start_col,
                    });
                }
                ']' => {
                    chars.next();
                    col += 1;
                    tokens.push(Token {
                        kind: TokenKind::RBracket,
                        line: start_line,
                        col: start_col,
                    });
                }
                ':' => {
                    chars.next();
                    col += 1;
                    tokens.push(Token {
                        kind: TokenKind::Colon,
                        line: start_line,
                        col: start_col,
                    });
                }
                '"' => {
                    chars.next(); // consume open quote
                    col += 1;
                    let mut s = String::new();
                    let mut closed = false;
                    while let Some(&c) = chars.peek() {
                        if c == '"' {
                            chars.next();
                            col += 1;
                            closed = true;
                            break;
                        }
                        if c == '\n' {
                            return Err(format!(
                                "{}:{}: Unterminated string literal",
                                start_line, start_col
                            ));
                        }
                        if c == '\\' {
                            // Escape sequences: \n \t \r \0 \\ \"
                            chars.next();
                            col += 1;
                            let escaped = match chars.peek() {
                                Some('n') => '\n',
                                Some('t') => '\t',
                                Some('r') => '\r',
                                Some('0') => '\0',
                                Some('\\') => '\\',
                                Some('"') => '"',
                                Some(other) => {
                                    return Err(format!(
                                        "{}:{}: Unknown escape sequence \\{} in string literal",
                                        line, col, other
                                    ))
                                }
                                None => {
                                    return Err(format!(
                                        "{}:{}: Unterminated string literal",
                                        start_line, start_col
                                    ))
                                }
                            };
                            s.push(escaped);
                            chars.next();
                            col += 1;
                            continue;
                        }
                        s.push(c);
                        chars.next();
                        col += 1;
                    }
                    if !closed {
                        return Err(format!(
                            "{}:{}: Unterminated string literal",
                            start_line, start_col
                        ));
                    }
                    tokens.push(Token {
                        kind: TokenKind::StringLit(s),
                        line: start_line,
                        col: start_col,
                    });
                }
                ';' => {
                    // Line comment
                    while let Some(&c) = chars.peek() {
                        if c == '\n' {
                            chars.next();
                            line += 1;
                            col = 1;
                            break;
                        }
                        chars.next();
                        col += 1;
                    }
                }
                // any other whitespace (a form feed, a no-break space) is
                // invisible in most editors: name it rather than guess
                c if c.is_whitespace() => {
                    return Err(format!(
                        "{}:{}: unexpected whitespace character U+{:04X}; separate tokens with spaces, tabs, or newlines",
                        line, col, c as u32
                    ));
                }
                _ => {
                    let mut symbol = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_whitespace()
                            || c == '('
                            || c == ')'
                            || c == '['
                            || c == ']'
                            || c == ':'
                        {
                            break;
                        }
                        symbol.push(c);
                        chars.next();
                        col += 1;
                    }

                    if symbol == "->" {
                        tokens.push(Token {
                            kind: TokenKind::Arrow,
                            line: start_line,
                            col: start_col,
                        });
                    } else if symbol == "true" {
                        tokens.push(Token {
                            kind: TokenKind::BoolLit(true),
                            line: start_line,
                            col: start_col,
                        });
                    } else if symbol == "false" {
                        tokens.push(Token {
                            kind: TokenKind::BoolLit(false),
                            line: start_line,
                            col: start_col,
                        });
                    } else if let Ok(i) = symbol.parse::<i64>() {
                        tokens.push(Token {
                            kind: TokenKind::IntLit(i),
                            line: start_line,
                            col: start_col,
                        });
                    } else if let Some(i) = symbol
                        .strip_suffix("i64")
                        .filter(|s| !s.is_empty())
                        .and_then(|s| s.parse::<i64>().ok())
                    {
                        // `42i64` / `-7i64`: an explicit 64-bit integer literal.
                        tokens.push(Token {
                            kind: TokenKind::Int64Lit(i),
                            line: start_line,
                            col: start_col,
                        });
                    } else if let Some(f) = symbol
                        .strip_suffix("f32")
                        .filter(|s| s.contains('.') && s.chars().any(|c| c.is_ascii_digit()))
                        .and_then(|s| s.parse::<f64>().ok())
                    {
                        // `1.5f32`: the nearest f64, then the nearest f32
                        tokens.push(Token {
                            kind: TokenKind::Float32Lit(f as f32, symbol.clone()),
                            line: start_line,
                            col: start_col,
                        });
                    } else if symbol.contains('.')
                        && symbol.chars().any(|c| c.is_ascii_digit())
                        && symbol.parse::<f64>().is_ok()
                    {
                        let f = symbol.parse::<f64>().unwrap();
                        tokens.push(Token {
                            kind: TokenKind::FloatLit(f, symbol.clone()),
                            line: start_line,
                            col: start_col,
                        });
                    } else if !symbol.is_empty() {
                        tokens.push(Token {
                            kind: TokenKind::Symbol(symbol),
                            line: start_line,
                            col: start_col,
                        });
                    }
                }
            }
        }
        Ok(tokens)
    }

    pub fn parse(input: &str) -> Result<Module, String> {
        Self::parse_tokens(Self::tokenize(input)?)
    }

    /// Parses an already-tokenized `(module ...)`. The resolver uses it to
    /// parse each item of a flattened program with its original positions.
    pub fn parse_tokens(tokens: Vec<Token>) -> Result<Module, String> {
        let tokens = strip_marks(tokens);
        // the unions declared in these tokens: `( union NAME`
        let unions = tokens
            .windows(3)
            .filter_map(|w| match (&w[0].kind, &w[1].kind, &w[2].kind) {
                (TokenKind::LParen, TokenKind::Symbol(u), TokenKind::Symbol(n)) if u == "union" => Some(n.clone()),
                _ => None,
            })
            .collect();
        let values = value_struct_names(&tokens);
        Self::parse_tokens_with(tokens, unions, values)
    }

    /// parse_tokens for one item of a larger program, given the program's
    /// union and value struct names (the resolver parses items one at a time).
    pub fn parse_tokens_with(
        tokens: Vec<Token>,
        unions: std::collections::HashSet<String>,
        values: std::collections::HashSet<String>,
    ) -> Result<Module, String> {
        let mut parser = Parser { tokens: strip_marks(tokens), pos: 0, last_pos: (1, 1), unions, values };
        parser.parse_module()
    }

    fn cur_pos(&self) -> (u32, u32) {
        if let Some(tok) = self.tokens.get(self.pos) {
            (tok.line, tok.col)
        } else if let Some(last) = self.tokens.last() {
            (last.line, last.col)
        } else {
            self.last_pos
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_kind(&self) -> Option<&TokenKind> {
        self.tokens.get(self.pos).map(|t| &t.kind)
    }

    fn next(&mut self) -> Option<Token> {
        if self.pos < self.tokens.len() {
            let t = self.tokens[self.pos].clone();
            self.last_pos = (t.line, t.col);
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }

    fn expect_kind(&mut self, expected: TokenKind) -> Result<Token, String> {
        let pos = self.cur_pos();
        let t = self.next();
        if let Some(tok) = t {
            if tok.kind == expected {
                Ok(tok)
            } else {
                Err(format!(
                    "{}:{}: Expected '{}', got '{}'",
                    tok.line, tok.col, expected, tok.kind
                ))
            }
        } else {
            Err(format!(
                "{}:{}: Expected '{}', got EOF",
                pos.0, pos.1, expected
            ))
        }
    }

    /// expect_kind, with `hint` after the usual message: what to write.
    fn expect_kind_hint(&mut self, expected: TokenKind, hint: &str) -> Result<Token, String> {
        self.expect_kind(expected).map_err(|e| format!("{e}; {hint}"))
    }

    fn parse_module(&mut self) -> Result<Module, String> {
        let (l, c) = self.cur_pos();
        self.expect_kind(TokenKind::LParen)?;
        match self.next() {
            Some(Token {
                kind: TokenKind::Symbol(s),
                ..
            }) if s == "module" => {}
            Some(tok) => {
                return Err(format!(
                    "{}:{}: Expected 'module' symbol, got '{}'",
                    tok.line, tok.col, tok.kind
                ))
            }
            None => return Err(format!("{}:{}: Expected 'module' symbol, got EOF", l, c)),
        }

        let name = match self.next() {
            Some(Token {
                kind: TokenKind::Symbol(s),
                ..
            }) => s,
            Some(tok) => {
                return Err(format!(
                    "{}:{}: Expected module name, got '{}'",
                    tok.line, tok.col, tok.kind
                ))
            }
            None => return Err(format!("{}:{}: Expected module name, got EOF", l, c)),
        };

        let mut imports = Vec::new();
        let mut structs = Vec::new();
        let mut enums = Vec::new();
        let mut unions = Vec::new();
        let mut functions = Vec::new();
        while let Some(TokenKind::LParen) = self.peek_kind() {
            if self.is_import_ahead() {
                imports.push(self.parse_import()?);
            } else if self.is_struct_ahead() {
                structs.push(self.parse_struct_def()?);
            } else if self.is_head_ahead("enum") {
                enums.push(self.parse_enum_def()?);
            } else if self.is_head_ahead("union") {
                unions.push(self.parse_union_def()?);
            } else if self.is_head_ahead("const") {
                let (l, c) = self.cur_pos();
                return Err(format!(
                    "{}:{}: (const ...) is expanded during import resolution; parse the file with Resolver, not Parser::parse",
                    l, c
                ));
            } else {
                functions.push(self.parse_fn_def()?);
            }
        }

        self.expect_kind(TokenKind::RParen)?;

        if let Some(tok) = self.peek() {
            return Err(format!(
                "{}:{}: unexpected tokens after module end — check for an extra ')'",
                tok.line, tok.col
            ));
        }

        Ok(Module {
            name,
            imports,
            structs,
            enums,
            unions,
            functions,
            sources: Default::default(),
            file_names: Default::default(),
        })
    }

    /// `(union Name [(variant field:T ...) ...])`; a variant without fields
    /// is `(variant)`.
    fn parse_union_def(&mut self) -> Result<UnionDef, String> {
        let tok = self.expect_kind(TokenKind::LParen)?;
        let span = (tok.line, tok.col);
        self.next(); // `union`
        let name = self.expect_symbol("union name", span)?;
        self.expect_kind(TokenKind::LBracket)?;
        let mut variants = Vec::new();
        while !matches!(self.peek_kind(), Some(TokenKind::RBracket) | None) {
            let (l, c) = self.cur_pos();
            if self.peek_kind() != Some(&TokenKind::LParen) {
                return Err(format!("{}:{}: a union variant is (name field:type ...), e.g. (circle radius:f64) or (empty)", l, c));
            }
            self.next();
            let vname = self.expect_symbol("variant name", (l, c))?;
            let mut fields = Vec::new();
            while !matches!(self.peek_kind(), Some(TokenKind::RParen) | None) {
                let fname = self.expect_symbol("field name", (l, c))?;
                self.expect_kind(TokenKind::Colon)?;
                fields.push(StructField { name: fname, ty: self.parse_type()? });
            }
            self.expect_kind(TokenKind::RParen)?;
            variants.push(Variant { name: vname, fields });
        }
        self.expect_kind(TokenKind::RBracket)?;
        self.expect_kind(TokenKind::RParen)?;
        Ok(UnionDef { name, variants, span })
    }

    fn is_head_ahead(&self, head: &str) -> bool {
        matches!(self.tokens.get(self.pos + 1), Some(Token { kind: TokenKind::Symbol(s), .. }) if s == head)
    }

    /// `(enum Name [a b (c 10) ...])`. Member values are i32 literals; the
    /// checker validates names and uniqueness.
    fn parse_enum_def(&mut self) -> Result<EnumDef, String> {
        let tok = self.expect_kind(TokenKind::LParen)?;
        let span = (tok.line, tok.col);
        self.next(); // `enum`
        let name = self.expect_symbol("enum name", span)?;
        let open = self.expect_kind(TokenKind::LBracket)?;
        let mut members = Vec::new();
        let mut next_value: i64 = 0;
        loop {
            let (l, c) = self.cur_pos();
            match self.next() {
                Some(Token { kind: TokenKind::RBracket, .. }) => break,
                Some(Token { kind: TokenKind::Symbol(m), .. }) => {
                    members.push((m, next_value));
                }
                Some(Token { kind: TokenKind::LParen, .. }) => {
                    let m = self.expect_symbol("enum member name", (l, c))?;
                    let v = match self.next() {
                        Some(Token { kind: TokenKind::IntLit(v), .. }) => v,
                        _ => return Err(format!("{}:{}: an enum member with a value is (name INTEGER), e.g. (blue 5)", l, c)),
                    };
                    self.expect_kind(TokenKind::RParen)?;
                    members.push((m, v));
                }
                None => return Err(format!("{}:{}: this bracket is never closed", open.line, open.col)),
                Some(t) => {
                    return Err(format!(
                        "{}:{}: an enum member is a name or (name INTEGER), got '{}'",
                        t.line, t.col, t.kind
                    ))
                }
            }
            let v = members.last().unwrap().1;
            if v < i32::MIN as i64 || v > i32::MAX as i64 {
                return Err(format!("{}:{}: enum member value {} is not an i32", l, c, v));
            }
            next_value = v + 1;
        }
        self.expect_kind(TokenKind::RParen)?;
        let members = members.into_iter().map(|(m, v)| (m, v as i32)).collect();
        Ok(EnumDef { name, members, span })
    }

    fn is_import_ahead(&self) -> bool {
        if self.pos + 1 < self.tokens.len() {
            if let TokenKind::Symbol(s) = &self.tokens[self.pos + 1].kind {
                return s == "import";
            }
        }
        false
    }

    fn is_struct_ahead(&self) -> bool {
        if self.pos + 1 < self.tokens.len() {
            if let TokenKind::Symbol(s) = &self.tokens[self.pos + 1].kind {
                return s == "struct";
            }
        }
        false
    }

    fn parse_struct_def(&mut self) -> Result<StructDef, String> {
        let tok = self.expect_kind(TokenKind::LParen)?;
        let span = (tok.line, tok.col);
        match self.next() {
            Some(Token {
                kind: TokenKind::Symbol(s),
                ..
            }) if s == "struct" => {}
            Some(tok) => {
                return Err(format!(
                    "{}:{}: Expected 'struct', got '{}'",
                    tok.line, tok.col, tok.kind
                ))
            }
            None => return Err(format!("{}:{}: Expected 'struct', got EOF", span.0, span.1)),
        }

        let name = match self.next() {
            Some(Token {
                kind: TokenKind::Symbol(s),
                ..
            }) => s,
            Some(tok) => {
                return Err(format!(
                    "{}:{}: Expected struct name, got '{}'",
                    tok.line, tok.col, tok.kind
                ))
            }
            None => return Err(format!("{}:{}: Expected struct name, got EOF", span.0, span.1)),
        };

        self.expect_kind(TokenKind::LBracket)?;
        let mut fields = Vec::new();
        while let Some(kind) = self.peek_kind() {
            if *kind == TokenKind::RBracket {
                break;
            }
            let field_name = match self.next() {
                Some(Token {
                    kind: TokenKind::Symbol(s),
                    ..
                }) => s,
                Some(tok) => {
                    return Err(format!(
                        "{}:{}: Expected field name, got '{}'",
                        tok.line, tok.col, tok.kind
                    ))
                }
                None => {
                    return Err(format!(
                        "{}:{}: Expected field name, got EOF",
                        span.0, span.1
                    ))
                }
            };
            self.expect_kind(TokenKind::Colon)?;
            let ty = self.parse_type()?;
            fields.push(StructField {
                name: field_name,
                ty,
            });
        }
        self.expect_kind(TokenKind::RBracket)?;
        // (value): a value struct
        let mut is_value = false;
        if self.peek_kind() == Some(&TokenKind::LParen) {
            let open = self.next().unwrap();
            match self.next() {
                Some(Token { kind: TokenKind::Symbol(m), .. }) if m == "value" => is_value = true,
                Some(tok) => return Err(format!("{}:{}: Expected (value) or ')' after a struct's fields, got '{}'", tok.line, tok.col, tok.kind)),
                None => return Err(format!("{}:{}: Expected (value) or ')' after a struct's fields, got EOF", open.line, open.col)),
            }
            self.expect_kind(TokenKind::RParen)?;
        }
        self.expect_kind(TokenKind::RParen)?;

        Ok(StructDef {
            name,
            fields,
            span,
            is_value,
        })
    }

    fn parse_import(&mut self) -> Result<Import, String> {
        let (l, c) = self.cur_pos();
        self.expect_kind(TokenKind::LParen)?;
        match self.next() {
            Some(Token {
                kind: TokenKind::Symbol(s),
                ..
            }) if s == "import" => {}
            Some(tok) => {
                return Err(format!(
                    "{}:{}: Expected 'import', got '{}'",
                    tok.line, tok.col, tok.kind
                ))
            }
            None => return Err(format!("{}:{}: Expected 'import', got EOF", l, c)),
        }
        let name = match self.next() {
            Some(Token {
                kind: TokenKind::Symbol(s),
                ..
            }) => s,
            Some(tok) => {
                return Err(format!(
                    "{}:{}: Expected module name in import, got '{}'",
                    tok.line, tok.col, tok.kind
                ))
            }
            None => {
                return Err(format!(
                    "{}:{}: Expected module name in import, got EOF",
                    l, c
                ))
            }
        };
        let alias = match self.peek_kind() {
            Some(TokenKind::Symbol(s)) if s == "as" => {
                self.next();
                match self.next() {
                    Some(Token {
                        kind: TokenKind::Symbol(a),
                        ..
                    }) => Some(a),
                    Some(tok) => {
                        return Err(format!(
                            "{}:{}: Expected alias after 'as', got '{}'",
                            tok.line, tok.col, tok.kind
                        ))
                    }
                    None => {
                        return Err(format!(
                            "{}:{}: Expected alias after 'as', got EOF",
                            l, c
                        ))
                    }
                }
            }
            _ => None,
        };
        self.expect_kind(TokenKind::RParen)?;
        Ok(Import { name, alias })
    }

    fn parse_fn_def(&mut self) -> Result<FnDef, String> {
        let fn_tok = self.expect_kind(TokenKind::LParen)?;
        let span = (fn_tok.line, fn_tok.col);
        match self.next() {
            Some(Token {
                kind: TokenKind::Symbol(s),
                ..
            }) if s == "fn" => {}
            Some(tok) => {
                return Err(format!(
                    "{}:{}: Expected 'fn', got '{}'",
                    tok.line, tok.col, tok.kind
                ))
            }
            None => return Err(format!("{}:{}: Expected 'fn', got EOF", span.0, span.1)),
        }

        let name = match self.next() {
            Some(Token {
                kind: TokenKind::Symbol(s),
                ..
            }) => s,
            Some(tok) => {
                return Err(format!(
                    "{}:{}: Expected fn name, got '{}'",
                    tok.line, tok.col, tok.kind
                ))
            }
            None => return Err(format!("{}:{}: Expected fn name, got EOF", span.0, span.1)),
        };

        // Params in [param1:type1 param2:type2]
        self.expect_kind(TokenKind::LBracket)?;
        let mut params = Vec::new();
        while let Some(kind) = self.peek_kind() {
            if *kind == TokenKind::RBracket {
                break;
            }
            let param_name = match self.next() {
                Some(Token {
                    kind: TokenKind::Symbol(s),
                    ..
                }) => s,
                Some(tok) => {
                    return Err(format!(
                        "{}:{}: Expected param name, got '{}'",
                        tok.line, tok.col, tok.kind
                    ))
                }
                None => {
                    return Err(format!(
                        "{}:{}: Expected param name, got EOF",
                        span.0, span.1
                    ))
                }
            };
            self.expect_kind(TokenKind::Colon)?;
            let ty = self.parse_type()?;
            params.push((param_name, ty));
        }
        self.expect_kind(TokenKind::RBracket)?;

        self.expect_kind(TokenKind::Arrow)?;
        let return_type = self.parse_type()?;

        let mut contracts = Vec::new();
        let mut body = Vec::new();
        let mut is_unsafe = false;
        let mut is_no_leak = false;

        while let Some(kind) = self.peek_kind() {
            if *kind == TokenKind::RParen {
                break;
            }
            if *kind == TokenKind::LParen
                && self.is_contract_ahead() {
                    contracts.push(self.parse_contract()?);
                    continue;
                }
            // the clause (unsafe), which an (unsafe e...) block is not
            if self.is_clause_ahead("unsafe") {
                self.pos += 3;
                is_unsafe = true;
                continue;
            }
            if self.is_clause_ahead("no_leak") {
                self.pos += 3;
                is_no_leak = true;
                continue;
            }
            body.push(self.parse_expr()?);
        }

        self.expect_kind(TokenKind::RParen)?;
        Ok(FnDef {
            name,
            params,
            return_type,
            contracts,
            body,
            is_unsafe,
            is_no_leak,
            span,
        })
    }

    /// Whether `(word)` is next: a clause such as (unsafe) or (no_leak).
    fn is_clause_ahead(&self, word: &str) -> bool {
        matches!(self.tokens.get(self.pos).map(|t| &t.kind), Some(TokenKind::LParen))
            && matches!(self.tokens.get(self.pos + 1).map(|t| &t.kind), Some(TokenKind::Symbol(s)) if s == word)
            && matches!(self.tokens.get(self.pos + 2).map(|t| &t.kind), Some(TokenKind::RParen))
    }

    fn is_contract_ahead(&self) -> bool {
        if self.pos + 1 < self.tokens.len() {
            if let TokenKind::Symbol(s) = &self.tokens[self.pos + 1].kind {
                return s == "req" || s == "ens" || s == "inv";
            }
        }
        false
    }

    fn parse_contract(&mut self) -> Result<Contract, String> {
        let (l, c) = self.cur_pos();
        self.expect_kind(TokenKind::LParen)?;
        let kind = match self.next() {
            Some(Token {
                kind: TokenKind::Symbol(s),
                ..
            }) => s,
            Some(tok) => {
                return Err(format!(
                    "{}:{}: Expected contract opcode, got '{}'",
                    tok.line, tok.col, tok.kind
                ))
            }
            None => {
                return Err(format!(
                    "{}:{}: Expected contract opcode, got EOF",
                    l, c
                ))
            }
        };
        let expr = self.parse_expr()?;
        self.expect_kind_hint(TokenKind::RParen, &format!("a contract is one expression: ({kind} (gt n 0))"))?;

        match kind.as_str() {
            "req" => Ok(Contract::Requires(expr)),
            "ens" => Ok(Contract::Ensures(expr)),
            "inv" => Ok(Contract::Invariant(expr)),
            _ => Err(format!("{}:{}: Unknown contract opcode: {}", l, c, kind)),
        }
    }

    /// Next token as a symbol, or an error naming what was expected.
    fn expect_symbol(&mut self, what: &str, span: (u32, u32)) -> Result<String, String> {
        match self.next() {
            Some(Token { kind: TokenKind::Symbol(s), .. }) => Ok(s),
            Some(tok) => Err(format!("{}:{}: Expected {}, got '{}'", tok.line, tok.col, what, tok.kind)),
            None => Err(format!("{}:{}: Expected {}, got EOF", span.0, span.1, what)),
        }
    }

    /// A `Struct.field` symbol (split at the last '.': `compiler.Node.next`
    /// is struct `compiler.Node`, field `next`).
    fn field_ref(&mut self, form: &str, span: (u32, u32)) -> Result<(String, String), String> {
        match self.next() {
            Some(Token { kind: TokenKind::Symbol(s), .. }) => match s.rsplit_once('.') {
                Some((st, f)) if !st.is_empty() && !f.is_empty() => Ok((st.to_string(), f.to_string())),
                _ => Err(format!("{}:{}: Expected 'Struct.field' symbol in {}, got '{}'{}", span.0, span.1, form, s, Self::FIELD_HINT)),
            },
            Some(tok) => Err(format!("{}:{}: Expected 'Struct.field' symbol in {}, got '{}'{}", tok.line, tok.col, form, tok.kind, Self::FIELD_HINT)),
            None => Err(format!("{}:{}: Expected 'Struct.field' symbol in {}, got EOF", span.0, span.1, form)),
        }
    }

    /// What to add when a function type is written wrongly.
const FN_TYPE_HINT: &str = "a function type is (fn [i32] -> i32): the parameters in brackets, then ->";

/// A number straight after a `for`'s end: a step carried over from `loop`, which
/// would be taken as the first statement of the body and dropped
/// (CANONICAL_PLAN.md step 3).
pub const FOR_HAS_NO_STEP: &str = "a for has no step: (for i start end body...) counts up by one. A number here would be the first statement of the body, not a step; for another step, use while";

/// What to add to a field-reference error: the struct and field are one symbol.
const FIELD_HINT: &str = "; the struct and field are one symbol, as in (get p Point.x)";

/// The rest of a put or arr.set: field references, then the value (the
    /// last item before the closing parenthesis).
    fn path_and_value(&mut self, form: &str, span: (u32, u32)) -> Result<(Vec<(String, String)>, Expr), String> {
        let mut items = Vec::new();
        while !matches!(self.peek_kind(), Some(TokenKind::RParen) | None) {
            items.push(self.parse_expr()?);
        }
        let Some(val) = items.pop() else {
            let hint = if form == "arr.set" { "(arr.set i32 a i v) names the element type first" } else { "(put p Point.x v)" };
            return Err(format!("{}:{}: {} needs a value to store: {}", span.0, span.1, form, hint));
        };
        let mut path = Vec::new();
        for item in items {
            match &item {
                Expr::Var(name, at) => match name.rsplit_once('.') {
                    Some((st, f)) if !st.is_empty() && !f.is_empty() => path.push((st.to_string(), f.to_string())),
                    _ => return Err(format!("{}:{}: Expected 'Struct.field' symbol in {}, got '{}'{}", at.0, at.1, form, name, Self::FIELD_HINT)),
                },
                other => {
                    let at = other.span();
                    return Err(format!("{}:{}: {} takes one value to store, after its field path", at.0, at.1, form));
                }
            }
        }
        Ok((path, val))
    }

    fn parse_type(&mut self) -> Result<Type, String> {
        let (l, c) = self.cur_pos();
        match self.next() {
            Some(Token {
                kind: TokenKind::Symbol(s),
                line,
                col,
            }) => match s.as_str() {
                "i32" => Ok(Type::I32),
                "i64" => Ok(Type::I64),
                "f32" => Ok(Type::F32),
                "f64" => Ok(Type::F64),
                "bool" => Ok(Type::Bool),
                "str" => Ok(Type::Str),
                "void" => Ok(Type::Void),
                _ if Narrow::from_name(&s).is_some() => Ok(Type::Narrow(Narrow::from_name(&s).unwrap())),
                // any other name is a union or an enum type; the checker reports unknown ones
                _ if self.unions.contains(&s) => Ok(Type::Union(s)),
                _ if self.values.contains(&s) => Ok(Type::Struct(s)),
                _ if !s.is_empty() && !s.starts_with(|ch: char| ch.is_ascii_digit() || ch == '-') => Ok(Type::Enum(s)),
                _ => Err(format!("{}:{}: Unknown scalar type: {}", line, col, s)),
            },
            Some(Token {
                kind: TokenKind::LParen,
                line,
                col,
            }) => {
                let kind = match self.next() {
                    Some(Token {
                        kind: TokenKind::Symbol(s),
                        ..
                    }) => s,
                    Some(tok) => {
                        return Err(format!(
                            "{}:{}: Expected type constructor, got '{}'",
                            tok.line, tok.col, tok.kind
                        ))
                    }
                    None => {
                        return Err(format!(
                            "{}:{}: Expected type constructor, got EOF",
                            line, col
                        ))
                    }
                };
                if kind == "result" {
                    let ok_ty = self.parse_type()?;
                    let err_ty = self.parse_type()?;
                    self.expect_kind(TokenKind::RParen)?;
                    Ok(Type::ResultType(Box::new(ok_ty), Box::new(err_ty)))
                } else if kind == "arr" {
                    let elem_ty = self.parse_type()?;
                    if let Some(TokenKind::IntLit(_)) = self.peek_kind() {
                        return Err(format!(
                            "{}:{}: (arr T) takes no length; an array's length is set by (arr.new T n)",
                            line, col
                        ));
                    }
                    self.expect_kind(TokenKind::RParen)?;
                    Ok(Type::Array(Box::new(elem_ty)))
                } else if kind == "fn" {
                    // (fn [t1 t2] -> r)
                    self.expect_kind_hint(TokenKind::LBracket, Self::FN_TYPE_HINT)?;
                    let mut params = Vec::new();
                    while let Some(k) = self.peek_kind() {
                        if *k == TokenKind::RBracket {
                            break;
                        }
                        params.push(self.parse_type()?);
                    }
                    self.expect_kind(TokenKind::RBracket)?;
                    self.expect_kind_hint(TokenKind::Arrow, Self::FN_TYPE_HINT)?;
                    let ret = self.parse_type()?;
                    // (fn [..] -> r (no_leak)): only functions that keep no memory
                    let no_leak = self.peek_kind() == Some(&TokenKind::LParen);
                    if no_leak {
                        self.expect_kind(TokenKind::LParen)?;
                        let (l, c) = self.cur_pos();
                        let w = self.expect_symbol("(no_leak) in a function type", (l, c))?;
                        if w != "no_leak" {
                            return Err(format!("{}:{}: a function type is (fn [params] -> ret), or (fn [params] -> ret (no_leak)); got ({} ...)", l, c, w));
                        }
                        self.expect_kind(TokenKind::RParen)?;
                    }
                    self.expect_kind(TokenKind::RParen)?;
                    Ok(Type::Fn(params, Box::new(ret), no_leak))
                } else if kind == "ptr" {
                    let name = self.expect_symbol("struct name in (ptr S)", (line, col))?;
                    if parse_scalar_type_str(&name, (line, col)).is_ok() {
                        return Err(format!(
                            "{}:{}: (ptr {}) is not a type: ptr points to a struct; for a sequence of {} use (arr {})",
                            line, col, name, name, name
                        ));
                    }
                    self.expect_kind(TokenKind::RParen)?;
                    Ok(Type::Ptr(Box::new(Type::Struct(name))))                } else {
                    Err(format!(
                        "{}:{}: Unknown compound type: {}",
                        line, col, kind
                    ))
                }
            }
            Some(tok) => Err(format!(
                "{}:{}: Expected type token, got '{}'",
                tok.line, tok.col, tok.kind
            )),
            None => Err(format!("{}:{}: Expected type token, got EOF", l, c)),
        }
    }

    pub fn parse_expr(&mut self) -> Result<Expr, String> {
        let (l, c) = self.cur_pos();
        let tok_opt = self.peek().cloned();
        if let Some(tok) = tok_opt {
            match tok.kind {
                TokenKind::IntLit(val) => {
                    self.next();
                    // 32 bits, read as signed or unsigned: 4294967295 is the bit pattern of -1
                    if !(-2147483648..=4294967295).contains(&val) {
                        return Err(format!(
                            "{}:{}: integer literal {} does not fit in 32 bits; write {}i64 for an i64",
                            tok.line, tok.col, val, val
                        ));
                    }
                    Ok(Expr::Lit(Literal::Int(val), (tok.line, tok.col)))
                }
                TokenKind::Int64Lit(val) => {
                    self.next();
                    Ok(Expr::Lit(Literal::Int64(val), (tok.line, tok.col)))
                }
                TokenKind::FloatLit(val, _) => {
                    self.next();
                    Ok(Expr::Lit(Literal::Float(val), (tok.line, tok.col)))
                }
                TokenKind::Float32Lit(val, text) => {
                    self.next();
                    Ok(Expr::Lit(Literal::Float32(val, text), (tok.line, tok.col)))
                }
                TokenKind::BoolLit(val) => {
                    self.next();
                    Ok(Expr::Lit(Literal::Bool(val), (tok.line, tok.col)))
                }
                TokenKind::StringLit(ref s) => {
                    let val = s.clone();
                    self.next();
                    Ok(Expr::Lit(Literal::Str(val), (tok.line, tok.col)))
                }
                TokenKind::Symbol(ref s) => {
                    let val = s.clone();
                    self.next();
                    Ok(Expr::Var(val, (tok.line, tok.col)))
                }
                TokenKind::LParen => {
                    let lparen_tok = self.next().unwrap(); // consume LParen
                    let span = (lparen_tok.line, lparen_tok.col);
                    let head = match self.next() {
                        Some(Token {
                            kind: TokenKind::Symbol(s),
                            ..
                        }) => s,
                        Some(tok) => {
                            return Err(format!(
                                "{}:{}: Expected operator or keyword in expr, got '{}'",
                                tok.line, tok.col, tok.kind
                            ))
                        }
                        None => {
                            return Err(format!(
                                "{}:{}: Expected operator or keyword in expr, got EOF",
                                span.0, span.1
                            ))
                        }
                    };

                    let expr = match head.as_str() {
                        "let" => {
                            let name = match self.next() {
                                Some(Token {
                                    kind: TokenKind::Symbol(s),
                                    ..
                                }) => s,
                                Some(tok) => {
                                    return Err(format!(
                                        "{}:{}: Expected variable name, got '{}'",
                                        tok.line, tok.col, tok.kind
                                    ))
                                }
                                None => {
                                    return Err(format!(
                                        "{}:{}: Expected variable name, got EOF",
                                        span.0, span.1
                                    ))
                                }
                            };
                            self.expect_kind_hint(TokenKind::Colon, &format!("every let names its type: (let {name}:i32 5)"))?;
                            let ty = self.parse_type()?;
                            let val = self.parse_expr()?;
                            Expr::Let {
                                name,
                                ty,
                                val: Box::new(val),
                                span,
                            }
                        }
                        "set!" => {
                            let name = match self.next() {
                                Some(Token {
                                    kind: TokenKind::Symbol(s),
                                    ..
                                }) => s,
                                Some(tok) => {
                                    return Err(format!(
                                        "{}:{}: Expected variable name, got '{}'",
                                        tok.line, tok.col, tok.kind
                                    ))
                                }
                                None => {
                                    return Err(format!(
                                        "{}:{}: Expected variable name, got EOF",
                                        span.0, span.1
                                    ))
                                }
                            };
                            let val = self.parse_expr()?;
                            Expr::Set {
                                name,
                                val: Box::new(val),
                                span,
                            }
                        }
                        "if" => {
                            let cond = self.parse_expr()?;
                            let then_b = self.parse_expr()?;
                            if let Some(tok) = self.peek().filter(|t| t.kind == TokenKind::RParen) {
                                return Err(format!(
                                    "{}:{}: Unexpected token parsing expression: ')'; if needs both branches: (if c then else); for one branch, write (when c then)",
                                    tok.line, tok.col
                                ));
                            }
                            let else_b = self.parse_expr()?;
                            // one branch is when (CANONICAL_PLAN.md step 4)
                            let empty = |e: &Expr| matches!(e, Expr::Block(items, _) if items.is_empty());
                            for (branch, why) in [(&else_b, "an if has two real branches, and this else is an empty (block): for one branch, write (when c then...)"), (&then_b, "an if has two real branches, and this then is an empty (block): for an else alone, write (when (not c) else...)")] {
                                if empty(branch) {
                                    let (l, c) = branch.span();
                                    return Err(format!("{l}:{c}: {why}"));
                                }
                            }
                            Expr::If {
                                cond: Box::new(cond),
                                then_branch: Box::new(then_b),
                                else_branch: Box::new(else_b),
                                span,
                            }
                        }
                        "counted_loop" => {
                            let var = match self.next() {
                                Some(Token {
                                    kind: TokenKind::Symbol(s),
                                    ..
                                }) => s,
                                Some(tok) => {
                                    return Err(format!(
                                        "{}:{}: Expected loop var, got '{}'",
                                        tok.line, tok.col, tok.kind
                                    ))
                                }
                                None => {
                                    return Err(format!(
                                        "{}:{}: Expected loop var, got EOF",
                                        span.0, span.1
                                    ))
                                }
                            };
                            let start = self.parse_expr()?;
                            let end = self.parse_expr()?;
                            let step = self.parse_expr()?;
                            let mut body = Vec::new();
                            while let Some(kind) = self.peek_kind() {
                                if *kind == TokenKind::RParen {
                                    break;
                                }
                                body.push(self.parse_expr()?);
                            }
                            Expr::Loop {
                                var,
                                start: Box::new(start),
                                end: Box::new(end),
                                step: Box::new(step),
                                body,
                                span,
                            }
                        }
                        // (for i start end body...): the resolver rewrites it as
                        // counted_loop (src/sugar.rs); read here too, for a module
                        // parsed on its own, with the same end and step
                        "for" => {
                            let var = self.expect_symbol("loop var", span)?;
                            let start = self.parse_expr()?;
                            let end = self.parse_expr()?;
                            if let Some(tok) = self.peek().filter(|t| matches!(t.kind, TokenKind::IntLit(_))) {
                                return Err(format!("{}:{}: {}", tok.line, tok.col, Self::FOR_HAS_NO_STEP));
                            }
                            let mut body = Vec::new();
                            while let Some(kind) = self.peek_kind() {
                                if *kind == TokenKind::RParen {
                                    break;
                                }
                                body.push(self.parse_expr()?);
                            }
                            let one = || Expr::Lit(Literal::Int(1), span);
                            Expr::Loop {
                                var,
                                start: Box::new(start),
                                end: Box::new(Expr::Op { op: OpCode::Sub, args: vec![end, one()], span }),
                                step: Box::new(one()),
                                body,
                                span,
                            }
                        }
                        // (when c body...): the resolver rewrites it as (if c (block
                        // body...) (block)) (src/sugar.rs); read here too, into that tree,
                        // every part at the when's place
                        "when" => {
                            if self.peek_kind() == Some(&TokenKind::RParen) {
                                return Err(format!("{}:{}: when takes a condition and a body: (when c body...)", span.0, span.1));
                            }
                            let cond = self.parse_expr()?;
                            let mut body = Vec::new();
                            while let Some(kind) = self.peek_kind() {
                                if *kind == TokenKind::RParen {
                                    break;
                                }
                                body.push(self.parse_expr()?);
                            }
                            // an empty block last: every expression of the body is a
                            // statement, its value dropped, and the when has none
                            body.push(Expr::Block(Vec::new(), span));
                            Expr::If {
                                cond: Box::new(cond),
                                then_branch: Box::new(Expr::Block(body, span)),
                                else_branch: Box::new(Expr::Block(Vec::new(), span)),
                                span,
                            }
                        }
                        "while" => {
                            let cond = self.parse_expr()?;
                            let mut body = Vec::new();
                            while let Some(kind) = self.peek_kind() {
                                if *kind == TokenKind::RParen {
                                    break;
                                }
                                body.push(self.parse_expr()?);
                            }
                            Expr::While {
                                cond: Box::new(cond),
                                body,
                                span,
                            }
                        }
                        "call" => {
                            let func = match self.next() {
                                Some(Token {
                                    kind: TokenKind::Symbol(s),
                                    ..
                                }) => s,
                                Some(tok) => {
                                    return Err(format!(
                                        "{}:{}: Expected func name in call, got '{}'",
                                        tok.line, tok.col, tok.kind
                                    ))
                                }
                                None => {
                                    return Err(format!(
                                        "{}:{}: Expected func name in call, got EOF",
                                        span.0, span.1
                                    ))
                                }
                            };
                            let mut args = Vec::new();
                            while let Some(kind) = self.peek_kind() {
                                if *kind == TokenKind::RParen {
                                    break;
                                }
                                args.push(self.parse_expr()?);
                            }
                            Expr::Call { func, args, span }
                        }
                        s if s == "ok" || s.starts_with("ok:") => {
                            let explicit_ty = if self.peek_kind() == Some(&TokenKind::Colon) {
                                self.next();
                                Some(self.parse_type()?)
                            } else if let Some(t) = s.strip_prefix("ok:") {
                                Some(parse_scalar_type_str(t, span)?)
                            } else {
                                None
                            };
                            let val = self.parse_expr()?;
                            Expr::Ok(Box::new(val), explicit_ty, span)
                        }
                        s if s == "err" || s.starts_with("err:") => {
                            let explicit_ty = if self.peek_kind() == Some(&TokenKind::Colon) {
                                self.next();
                                Some(self.parse_type()?)
                            } else if let Some(t) = s.strip_prefix("err:") {
                                Some(parse_scalar_type_str(t, span)?)
                            } else {
                                None
                            };
                            let val = self.parse_expr()?;
                            Expr::Err(Box::new(val), explicit_ty, span)
                        }
                        "make" => {
                            let target = self.expect_symbol("Union.variant in make", span)?;
                            let mut args = Vec::new();
                            if self.values.contains(&target) {
                                while !matches!(self.peek_kind(), Some(TokenKind::RParen) | None) {
                                    args.push(self.parse_expr()?);
                                }
                                Expr::Record { struct_name: target, args, span }
                            } else {
                                let Some(dot) = target.rfind('.') else {
                                    return Err(format!("{}:{}: make names a value struct or a union variant (Union.variant), got {}", span.0, span.1, target));
                                };
                                while !matches!(self.peek_kind(), Some(TokenKind::RParen) | None) {
                                    args.push(self.parse_expr()?);
                                }
                                Expr::Make { union_name: target[..dot].to_string(), variant: target[dot + 1..].to_string(), args, span }
                            }
                        }
                        "match" => {
                            let value = self.parse_expr()?;
                            let mut arms = Vec::new();
                            let mut else_body = None;
                            while !matches!(self.peek_kind(), Some(TokenKind::RParen) | None) {
                                let open = self.expect_kind(TokenKind::LParen)?;
                                let arm_span = (open.line, open.col);
                                let member = self.expect_symbol("Name.member or else in a match arm", arm_span)?;
                                if else_body.is_some() {
                                    return Err(format!("{}:{}: the (else ...) arm of a match comes last", arm_span.0, arm_span.1));
                                }
                                let binders = if self.peek_kind() == Some(&TokenKind::LBracket) {
                                    self.next();
                                    let mut names = Vec::new();
                                    while !matches!(self.peek_kind(), Some(TokenKind::RBracket) | None) {
                                        names.push(self.expect_symbol("a binder name", arm_span)?);
                                    }
                                    self.expect_kind(TokenKind::RBracket)?;
                                    Some(names)
                                } else {
                                    None
                                };
                                let mut body = Vec::new();
                                while !matches!(self.peek_kind(), Some(TokenKind::RParen) | None) {
                                    body.push(self.parse_expr()?);
                                }
                                self.expect_kind(TokenKind::RParen)?;
                                if member == "else" {
                                    if binders.is_some() {
                                        return Err(format!("{}:{}: the else arm of a match binds nothing", arm_span.0, arm_span.1));
                                    }
                                    else_body = Some(body);
                                } else {
                                    arms.push(MatchArm { member, binders, body, span: arm_span });
                                }
                            }
                            Expr::Match { value: Box::new(value), arms, else_body, span }
                        }
                        "block" | "unsafe" => {
                            let mut body = Vec::new();
                            while let Some(kind) = self.peek_kind() {
                                if *kind == TokenKind::RParen {
                                    break;
                                }
                                body.push(self.parse_expr()?);
                            }
                            if head == "unsafe" { Expr::Unsafe(body, span) } else { Expr::Block(body, span) }
                        }
                        "match_result" => {
                            let res_expr = self.parse_expr()?;
                            self.expect_kind(TokenKind::LParen)?;
                            match self.next() {
                                Some(Token {
                                    kind: TokenKind::Symbol(s),
                                    ..
                                }) if s == "ok" => {}
                                Some(tok) => {
                                    return Err(format!(
                                        "{}:{}: Expected 'ok' arm in match_result, got '{}'",
                                        tok.line, tok.col, tok.kind
                                    ))
                                }
                                None => {
                                    return Err(format!(
                                        "{}:{}: Expected 'ok' arm in match_result, got EOF",
                                        span.0, span.1
                                    ))
                                }
                            }
                            let ok_var = match self.next() {
                                Some(Token {
                                    kind: TokenKind::Symbol(s),
                                    ..
                                }) => s,
                                Some(tok) => {
                                    return Err(format!(
                                        "{}:{}: Expected ok var, got '{}'; match_result arms bind without brackets: (ok v ...) (err e ...)",
                                        tok.line, tok.col, tok.kind
                                    ))
                                }
                                None => {
                                    return Err(format!(
                                        "{}:{}: Expected ok var, got EOF",
                                        span.0, span.1
                                    ))
                                }
                            };
                            let mut ok_body = Vec::new();
                            while let Some(kind) = self.peek_kind() {
                                if *kind == TokenKind::RParen {
                                    break;
                                }
                                ok_body.push(self.parse_expr()?);
                            }
                            self.expect_kind(TokenKind::RParen)?;

                            self.expect_kind(TokenKind::LParen)?;
                            match self.next() {
                                Some(Token {
                                    kind: TokenKind::Symbol(s),
                                    ..
                                }) if s == "err" => {}
                                Some(tok) => {
                                    return Err(format!(
                                        "{}:{}: Expected 'err' arm in match_result, got '{}'",
                                        tok.line, tok.col, tok.kind
                                    ))
                                }
                                None => {
                                    return Err(format!(
                                        "{}:{}: Expected 'err' arm in match_result, got EOF",
                                        span.0, span.1
                                    ))
                                }
                            }
                            let err_var = match self.next() {
                                Some(Token {
                                    kind: TokenKind::Symbol(s),
                                    ..
                                }) => s,
                                Some(tok) => {
                                    return Err(format!(
                                        "{}:{}: Expected err var, got '{}'; match_result arms bind without brackets: (ok v ...) (err e ...)",
                                        tok.line, tok.col, tok.kind
                                    ))
                                }
                                None => {
                                    return Err(format!(
                                        "{}:{}: Expected err var, got EOF",
                                        span.0, span.1
                                    ))
                                }
                            };
                            let mut err_body = Vec::new();
                            while let Some(kind) = self.peek_kind() {
                                if *kind == TokenKind::RParen {
                                    break;
                                }
                                err_body.push(self.parse_expr()?);
                            }
                            self.expect_kind(TokenKind::RParen)?;

                            Expr::MatchResult {
                                expr: Box::new(res_expr),
                                ok_var,
                                ok_body,
                                err_var,
                                err_body,
                                span,
                            }
                        }
                        "get" => {
                            // (get e S.f), or a path (get e S.f V.g ...): one field of a
                            // value struct held in the field before it
                            let mut e = self.parse_expr()?;
                            loop {
                                let (struct_name, field_name) = self.field_ref("get", span)?;
                                e = Expr::GetField { struct_name, field_name, ptr: Box::new(e), span };
                                if matches!(self.peek_kind(), Some(TokenKind::RParen) | None) {
                                    break;
                                }
                            }
                            e
                        }
                        "with" => {
                            let value = self.parse_expr()?;
                            let (struct_name, field_name) = self.field_ref("with", span)?;
                            let new_val = self.parse_expr()?;
                            Expr::With { struct_name, field_name, value: Box::new(value), new_val: Box::new(new_val), span }
                        }
                        "put" => {
                            // (put p S.f v), or a path (put p S.f V.g ... v): the last
                            // item is the value, the dotted names before it the path
                            let ptr = self.parse_expr()?;
                            let (struct_name, field_name) = self.field_ref("put", span)?;
                            let (path, val) = self.path_and_value("put", span)?;
                            Expr::PutField { struct_name, field_name, ptr: Box::new(ptr), val: Box::new(val), path, span }
                        }
                        "sizeof" => {
                            // a struct by its bare name, or any type
                            let ty = match self.peek_kind() {
                                Some(TokenKind::Symbol(s))
                                    if !matches!(s.as_str(), "i32" | "i64" | "f32" | "f64" | "bool" | "str" | "void")
                                        && Narrow::from_name(s).is_none()
                                        && !self.unions.contains(s) =>
                                {
                                    let s = s.clone();
                                    self.next();
                                    Type::Struct(s)
                                }
                                Some(_) => self.parse_type()?,
                                None => {
                                    return Err(format!("{}:{}: Expected a type in sizeof, got EOF", span.0, span.1))
                                }
                            };
                            Expr::Sizeof { ty, span }
                        }
                        "return" => {
                            let val = if self.peek_kind() == Some(&TokenKind::RParen) {
                                None
                            } else {
                                Some(Box::new(self.parse_expr()?))
                            };
                            Expr::Return { val, span }
                        }
                        "break" => Expr::Break(span),
                        "continue" => Expr::Continue(span),
                        // (cond (c1 e...) (c2 e...) ... (else e...)) is sugar for
                        // (if c1 (block e...) (if c2 (block e...) ... (block e...))).
                        "cond" => {
                            let mut clauses: Vec<(Expr, Vec<Expr>, (u32, u32))> = Vec::new();
                            let mut else_body: Option<(Vec<Expr>, (u32, u32))> = None;
                            while self.peek_kind() == Some(&TokenKind::LParen) {
                                let ctok = self.expect_kind(TokenKind::LParen)?;
                                let cspan = (ctok.line, ctok.col);
                                if else_body.is_some() {
                                    return Err(format!("{}:{}: cond: the else clause must be last", cspan.0, cspan.1));
                                }
                                let is_else = matches!(self.peek_kind(), Some(TokenKind::Symbol(s)) if s == "else");
                                let test = if is_else {
                                    self.next();
                                    None
                                } else {
                                    Some(self.parse_expr()?)
                                };
                                let mut body = Vec::new();
                                while self.peek_kind() != Some(&TokenKind::RParen) && self.peek_kind().is_some() {
                                    body.push(self.parse_expr()?);
                                }
                                self.expect_kind(TokenKind::RParen)?;
                                if body.is_empty() {
                                    return Err(format!("{}:{}: cond clause needs a body", cspan.0, cspan.1));
                                }
                                match test {
                                    Some(t) => clauses.push((t, body, cspan)),
                                    None => else_body = Some((body, cspan)),
                                }
                            }
                            let (else_exprs, else_span) = else_body.ok_or_else(|| {
                                format!("{}:{}: cond needs a final (else ...) clause, as if needs an else branch", span.0, span.1)
                            })?;
                            if clauses.is_empty() {
                                return Err(format!("{}:{}: cond needs at least one (test body...) clause before else", span.0, span.1));
                            }
                            let mut acc = Expr::Block(else_exprs, else_span);
                            for (test, body, cspan) in clauses.into_iter().rev() {
                                acc = Expr::If {
                                    cond: Box::new(test),
                                    then_branch: Box::new(Expr::Block(body, cspan)),
                                    else_branch: Box::new(acc),
                                    span: cspan,
                                };
                            }
                            acc
                        }
                        "ref" => {
                            let name = self.expect_symbol("function name in ref", span)?;
                            Expr::Ref { name, span }
                        }
                        "call_ref" => {
                            if let Some(tok) = self.peek().filter(|t| matches!(t.kind, TokenKind::Symbol(_))) {
                                return Err(format!(
                                    "{}:{}: Expected the function's type, got '{}'; call_ref names the type first: (call_ref (fn [i32] -> i32) f args...)",
                                    tok.line, tok.col, tok.kind
                                ));
                            }
                            let sig = self.parse_type()?;
                            let func = self.parse_expr()?;
                            let mut args = Vec::new();
                            while let Some(kind) = self.peek_kind() {
                                if *kind == TokenKind::RParen {
                                    break;
                                }
                                args.push(self.parse_expr()?);
                            }
                            Expr::CallRef { sig, func: Box::new(func), args, span }
                        }
                        "arr.len" => {
                            let arr = self.parse_expr()?;
                            Expr::ArrLen { arr: Box::new(arr), span }
                        }
                        "ptr.null" => {
                            let name = self.expect_symbol("struct name in ptr.null", span)?;
                            Expr::Null { ty: Type::Ptr(Box::new(Type::Struct(name))), span }
                        }
                        "arr.null" => {
                            let elem = self.parse_type()?;
                            Expr::Null { ty: Type::Array(Box::new(elem)), span }
                        }
                        "ptr.cast" => {
                            let name = self.expect_symbol("struct name in ptr.cast", span)?;
                            let addr = self.parse_expr()?;
                            Expr::Cast { ty: Type::Ptr(Box::new(Type::Struct(name))), addr: Box::new(addr), gen: None, span }
                        }
                        "arr.cast" => {
                            let elem = self.parse_type()?;
                            let addr = self.parse_expr()?;
                            Expr::Cast { ty: Type::Array(Box::new(elem)), addr: Box::new(addr), gen: None, span }
                        }
                        "ptr.make" => {
                            let name = self.expect_symbol("struct name in ptr.make", span)?;
                            let addr = self.parse_expr()?;
                            let gen = self.parse_expr()?;
                            Expr::Cast { ty: Type::Ptr(Box::new(Type::Struct(name))), addr: Box::new(addr), gen: Some(Box::new(gen)), span }
                        }
                        "arr.make" => {
                            let elem = self.parse_type()?;
                            let addr = self.parse_expr()?;
                            let gen = self.parse_expr()?;
                            Expr::Cast { ty: Type::Array(Box::new(elem)), addr: Box::new(addr), gen: Some(Box::new(gen)), span }
                        }
                        "ptr.addr" | "arr.addr" | "enum.ord" | "ptr.gen" | "arr.gen" => {
                            let val = self.parse_expr()?;
                            let kind = match head.as_str() {
                                "ptr.addr" => AddrKind::Ptr,
                                "arr.addr" => AddrKind::Arr,
                                "ptr.gen" => AddrKind::PtrGen,
                                "arr.gen" => AddrKind::ArrGen,
                                _ => AddrKind::Enum,
                            };
                            Expr::Addr { val: Box::new(val), kind, span }
                        }
                        "enum.cast" => {
                            let name = self.expect_symbol("enum name in enum.cast", span)?;
                            let val = self.parse_expr()?;
                            Expr::Cast { ty: Type::Enum(name), addr: Box::new(val), gen: None, span }
                        }
                        "arr.get" => {
                            let elem_ty = self.parse_type()?;
                            let ptr = self.parse_expr()?;
                            if let Some(tok) = self.peek().filter(|t| t.kind == TokenKind::RParen) {
                                return Err(format!(
                                    "{}:{}: Unexpected token parsing expression: ')'; arr.get names the element type first: (arr.get i32 a i)",
                                    tok.line, tok.col
                                ));
                            }
                            let index = self.parse_expr()?;
                            // a path reads one field of a value struct element
                            let mut e = Expr::ArrGet { elem_ty, ptr: Box::new(ptr), index: Box::new(index), span };
                            while !matches!(self.peek_kind(), Some(TokenKind::RParen) | None) {
                                let (struct_name, field_name) = self.field_ref("arr.get", span)?;
                                e = Expr::GetField { struct_name, field_name, ptr: Box::new(e), span };
                            }
                            e
                        }
                        "arr.set" => {
                            let elem_ty = self.parse_type()?;
                            let ptr = self.parse_expr()?;
                            let index = self.parse_expr()?;
                            let (path, val) = self.path_and_value("arr.set", span)?;
                            Expr::ArrSet { elem_ty, ptr: Box::new(ptr), index: Box::new(index), val: Box::new(val), path, span }
                        }
                        "mem.free" => {
                            return Err(format!(
                                "{}:{}: there is no mem.free: memory from mem.alloc is never freed. Free single objects with std/heap, or everything in a region at once with std/arena",
                                span.0, span.1
                            ))
                        }
                        float_mem @ ("mem.load_f32" | "mem.load_f64" | "mem.store_f32" | "mem.store_f64") => {
                            return Err(format!(
                                "{}:{}: there is no {}: keep floats in struct fields or (arr f64), or move their bits with mem.load64/mem.store64 and f64.reinterpret_i64/i64.reinterpret_f64",
                                span.0, span.1, float_mem
                            ))
                        }
                        op_str => {
                            let op = match op_str {
                                "+" => OpCode::Add,
                                "-" => OpCode::Sub,
                                "*" => OpCode::Mul,
                                "/" => OpCode::Div,
                                "%" => OpCode::Mod,
                                "^" => OpCode::BitXor,
                                "shl" => OpCode::Shl,
                                "shr" => OpCode::Shr,
                                "shru" => OpCode::ShrU,
                                "bitand" => OpCode::BitAnd,
                                "bitor" => OpCode::BitOr,
                                "mem.load8" => OpCode::MemLoad8,
                                "mem.load32" => OpCode::MemLoad32,
                                "mem.load64" => OpCode::MemLoad64,
                                "mem.store8" => OpCode::MemStore8,
                                "mem.load8_s" => OpCode::MemLoad8S,
                                "mem.load16_u" => OpCode::MemLoad16U,
                                "mem.load16_s" => OpCode::MemLoad16S,
                                "mem.store16" => OpCode::MemStore16,
                                "mem.store32" => OpCode::MemStore32,
                                "mem.store64" => OpCode::MemStore64,
                                "mem.alloc" => OpCode::MemAlloc,
                                "mem.grow" => OpCode::MemGrow,
                                "str.len" => OpCode::StrLen,
                                "str.ptr" => OpCode::StrPtr,
                                "atomic.add" => OpCode::AtomicAdd,
                                "atomic.cas" => OpCode::AtomicCas,
                                "atomic.lock" => OpCode::AtomicLock,
                                "atomic.unlock" => OpCode::AtomicUnlock,
                                "eq" => OpCode::Eq,
                                "neq" => OpCode::Neq,
                                "lt" => OpCode::Lt,
                                "lte" => OpCode::Lte,
                                "ltu" => OpCode::LtU,
                                "checked.add" => OpCode::CheckedAdd,
                                "checked.sub" => OpCode::CheckedSub,
                                "checked.mul" => OpCode::CheckedMul,
                                "lteu" => OpCode::LteU,
                                "gtu" => OpCode::GtU,
                                "gteu" => OpCode::GteU,
                                "gt" => OpCode::Gt,
                                "gte" => OpCode::Gte,
                                "and" => OpCode::And,
                                "or" => OpCode::Or,
                                "not" => OpCode::Not,
                                "sys.print" => OpCode::SysPrint,
                                "sys.time" => OpCode::SysTime,
                                "sys.monotonic" => OpCode::SysMonotonic,
                                "sys.random" => OpCode::SysRandom,
                                "sys.exit" => OpCode::SysExit,
                                "fs.open" => OpCode::FsOpen,
                                "fs.read" => OpCode::FsRead,
                                "fs.write" => OpCode::FsWrite,
                                "fs.close" => OpCode::FsClose,
                                "fs.delete" => OpCode::FsDelete,
                                "fs.opendir" => OpCode::FsOpendir,
                                "fs.readdir" => OpCode::FsReaddir,
                                "fs.stat" => OpCode::FsStat,
                                "fs.chmod" => OpCode::FsChmod,
                                "fs.rename" => OpCode::FsRename,
                                "fs.rmdir" => OpCode::FsRmdir,
                                "net.listen" => OpCode::NetListen,
                                "net.accept" => OpCode::NetAccept,
                                "net.recv" => OpCode::NetRecv,
                                "net.send" => OpCode::NetSend,
                                "net.close" => OpCode::NetClose,
                                "proc.spawn" => OpCode::ProcSpawn,
                                "proc.stream" => OpCode::ProcStream,
                                "proc.read" => OpCode::ProcRead,
                                "proc.write" => OpCode::ProcWrite,
                                "proc.close" => OpCode::ProcClose,
                                "proc.wait" => OpCode::ProcWait,
                                "proc.exec" => OpCode::ProcExec,
                                "args.sizes" => OpCode::ArgsSizes,
                                "args.get" => OpCode::ArgsGet,
                                "env.sizes" => OpCode::EnvSizes,
                                "env.get" => OpCode::EnvGet,
                                "thread.spawn" => OpCode::ThreadSpawn,
                                "thread.join" => OpCode::ThreadJoin,
                                "divu" => OpCode::DivU,
                                "remu" => OpCode::RemU,
                                "i64.extend_s" => OpCode::I64ExtendS,
                                "f64.convert_i64_s" => OpCode::F64ConvertI64S,
                                "f32.demote" => OpCode::F32DemoteF64,
                                "f64.promote" => OpCode::F64PromoteF32,
                                "f32.convert_i32_s" => OpCode::F32ConvertI32S,
                                "f32.convert_i64_s" => OpCode::F32ConvertI64S,
                                "f64.convert_i32_s" => OpCode::F64ConvertI32S,
                                "i32.trunc_f32_s" => OpCode::I32TruncF32S,
                                "i32.trunc_f64_s" => OpCode::I32TruncF64S,
                                "i64.trunc_f32_s" => OpCode::I64TruncF32S,
                                "f32.reinterpret_i32" => OpCode::F32ReinterpretI32,
                                "i32.reinterpret_f32" => OpCode::I32ReinterpretF32,
                                "f64.sqrt" => OpCode::F64Sqrt,
                                "i64.trunc_f64_s" => OpCode::I64TruncF64S,
                                "f64.reinterpret_i64" => OpCode::F64ReinterpretI64,
                                "i64.reinterpret_f64" => OpCode::I64ReinterpretF64,
                                "i64.extend_u" => OpCode::I64ExtendU,
                                "i32.wrap" => OpCode::I32Wrap,
                                "format.str" => OpCode::FormatStr,
                                "format.to" => OpCode::FormatTo,
                                "format.print" => OpCode::FormatPrint,
                                "format.println" => OpCode::FormatPrintln,
                                "format.eprintln" => OpCode::FormatEprintln,
                                "format.piece" => OpCode::FormatPiece,
                                "try" => OpCode::Try,
                                "try_opt" => OpCode::TryOpt,
                                "try_or" => OpCode::TryOr,
                                "defer" => OpCode::Defer,
                                "format.dbg" => OpCode::FormatDbg,
                                "format.expect" => OpCode::FormatExpect,
                                "format" | "format_to" | "print" | "println" | "eprintln" | "dbg" | "expect" | "new" | "arr.new" | "arr.of" | "zero" => {
                                    return Err(format!(
                                        "{}:{}: ({} ...) is expanded during import resolution; parse the file with Resolver, not Parser::parse",
                                        span.0, span.1, op_str
                                    ))
                                }
                                other => {
                                    let hint = if matches!(other, "else" | "elif" | "switch" | "case" | "default") {
                                        "there is no else if, elif, switch, or case: use (cond (test body...) ... (else body...))".to_string()
                                    } else if let Some((what, form)) = c_operator(other) {
                                        format!("'{other}' is not an operator; {what} is {form}")
                                    } else if other == "loop" {
                                        "loop is retired: (for i start end body...) counts from start up to end, end left out; for another step, use while".to_string()
                                    } else if let Some(near) = nearest_name(other, HEAD_NAMES.iter().copied()) {
                                        format!("did you mean {near}?")
                                    } else {
                                        format!("to call a function, write (call {other} args...)")
                                    };
                                    return Err(format!(
                                        "{}:{}: Unknown op/keyword: {}; {}",
                                        span.0, span.1, other, hint
                                    ))
                                }
                            };

                            let mut args = Vec::new();
                            while let Some(kind) = self.peek_kind() {
                                if *kind == TokenKind::RParen {
                                    break;
                                }
                                args.push(self.parse_expr()?);
                            }
                            Expr::Op { op, args, span }
                        }
                    };

                    self.expect_kind(TokenKind::RParen)?;
                    Ok(expr)
                }
                _ => Err(format!(
                    "{}:{}: Unexpected token parsing expression: '{}'",
                    tok.line, tok.col, tok.kind
                )),
            }
        } else {
            Err(format!("{}:{}: Unexpected EOF parsing expression", l, c))
        }
    }
}

fn parse_scalar_type_str(s: &str, span: (u32, u32)) -> Result<Type, String> {
    match s {
        "i32" => Ok(Type::I32),
        "i64" => Ok(Type::I64),
        "f32" => Ok(Type::F32),
        "f64" => Ok(Type::F64),
        "bool" => Ok(Type::Bool),
        "str" => Ok(Type::Str),
        "void" => Ok(Type::Void),
        _ => Err(format!("{}:{}: Unknown scalar type: {}", span.0, span.1, s)),
    }
}

/// Visibility marks (AIPL_SPEC.md 11) left out: `(pub)`, and the `(pub` /
/// `(read` and matching `)` of a field `(pub x:T)` / `(read x:T)`. They
/// matter only between modules, which the resolver checks before it takes
/// them out; a module parsed on its own may still have them.
/// (compiler.aipl strip_marks does the same for the AIPL toolchain.)
pub fn strip_marks(tokens: Vec<Token>) -> Vec<Token> {
    let sym = |t: Option<&Token>, s: &str| matches!(t, Some(Token { kind: TokenKind::Symbol(x), .. }) if x == s);
    let mut out = Vec::with_capacity(tokens.len());
    // per open bracket: whether its close is to be left out
    let mut opens: Vec<bool> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = &tokens[i];
        match t.kind {
            TokenKind::LParen if sym(tokens.get(i + 1), "pub") && matches!(tokens.get(i + 2).map(|t| &t.kind), Some(TokenKind::RParen)) => {
                i += 3;
                continue;
            }
            TokenKind::LParen
                if (sym(tokens.get(i + 1), "pub") || sym(tokens.get(i + 1), "read"))
                    && matches!(tokens.get(i + 2).map(|t| &t.kind), Some(TokenKind::Symbol(_)))
                    && matches!(tokens.get(i + 3).map(|t| &t.kind), Some(TokenKind::Colon)) =>
            {
                opens.push(true);
                i += 2;
                continue;
            }
            TokenKind::LParen | TokenKind::LBracket => opens.push(false),
            TokenKind::RParen | TokenKind::RBracket if opens.pop() == Some(true) => {
                i += 1;
                continue;
            }
            _ => {}
        }
        out.push(t.clone());
        i += 1;
    }
    out
}
