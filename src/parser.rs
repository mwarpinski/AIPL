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
    FloatLit(f64),
    StringLit(String),
    BoolLit(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub line: u32,
    pub col: u32,
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    last_pos: (u32, u32),
    /// Names of the program's unions: a bare type name is a union if listed,
    /// otherwise an enum (the checker reports unknown names).
    unions: std::collections::HashSet<String>,
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
                    } else if symbol.contains('.')
                        && symbol.chars().any(|c| c.is_ascii_digit())
                        && symbol.parse::<f64>().is_ok()
                    {
                        let f = symbol.parse::<f64>().unwrap();
                        tokens.push(Token {
                            kind: TokenKind::FloatLit(f),
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
        // the unions declared in these tokens: `( union NAME`
        let unions = tokens
            .windows(3)
            .filter_map(|w| match (&w[0].kind, &w[1].kind, &w[2].kind) {
                (TokenKind::LParen, TokenKind::Symbol(u), TokenKind::Symbol(n)) if u == "union" => Some(n.clone()),
                _ => None,
            })
            .collect();
        Self::parse_tokens_with(tokens, unions)
    }

    /// parse_tokens for one item of a larger program, given the program's
    /// union names (the resolver parses items one at a time).
    pub fn parse_tokens_with(tokens: Vec<Token>, unions: std::collections::HashSet<String>) -> Result<Module, String> {
        let mut parser = Parser { tokens, pos: 0, last_pos: (1, 1), unions };
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
                    "{}:{}: Expected {:?}, got {:?}",
                    tok.line, tok.col, expected, tok.kind
                ))
            }
        } else {
            Err(format!(
                "{}:{}: Expected {:?}, got EOF",
                pos.0, pos.1, expected
            ))
        }
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
                    "{}:{}: Expected 'module' symbol, got {:?}",
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
                    "{}:{}: Expected module name, got {:?}",
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
                        "{}:{}: an enum member is a name or (name INTEGER), got {:?}",
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
                    "{}:{}: Expected 'struct', got {:?}",
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
                    "{}:{}: Expected struct name, got {:?}",
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
                        "{}:{}: Expected field name, got {:?}",
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
        self.expect_kind(TokenKind::RParen)?;

        Ok(StructDef {
            name,
            fields,
            span,
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
                    "{}:{}: Expected 'import', got {:?}",
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
                    "{}:{}: Expected module name in import, got {:?}",
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
                            "{}:{}: Expected alias after 'as', got {:?}",
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
                    "{}:{}: Expected 'fn', got {:?}",
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
                    "{}:{}: Expected fn name, got {:?}",
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
                        "{}:{}: Expected param name, got {:?}",
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

        while let Some(kind) = self.peek_kind() {
            if *kind == TokenKind::RParen {
                break;
            }
            if *kind == TokenKind::LParen {
                if self.is_contract_ahead() {
                    contracts.push(self.parse_contract()?);
                    continue;
                }
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
            span,
        })
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
                    "{}:{}: Expected contract opcode, got {:?}",
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
        self.expect_kind(TokenKind::RParen)?;

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
            Some(tok) => Err(format!("{}:{}: Expected {}, got {:?}", tok.line, tok.col, what, tok.kind)),
            None => Err(format!("{}:{}: Expected {}, got EOF", span.0, span.1, what)),
        }
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
                // any other name is a union or an enum type; the checker reports unknown ones
                _ if self.unions.contains(&s) => Ok(Type::Union(s)),
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
                            "{}:{}: Expected type constructor, got {:?}",
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
                    self.expect_kind(TokenKind::LBracket)?;
                    let mut params = Vec::new();
                    while let Some(k) = self.peek_kind() {
                        if *k == TokenKind::RBracket {
                            break;
                        }
                        params.push(self.parse_type()?);
                    }
                    self.expect_kind(TokenKind::RBracket)?;
                    self.expect_kind(TokenKind::Arrow)?;
                    let ret = self.parse_type()?;
                    self.expect_kind(TokenKind::RParen)?;
                    Ok(Type::Fn(params, Box::new(ret)))
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
                "{}:{}: Expected type token, got {:?}",
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
                    Ok(Expr::Lit(Literal::Int(val), (tok.line, tok.col)))
                }
                TokenKind::Int64Lit(val) => {
                    self.next();
                    Ok(Expr::Lit(Literal::Int64(val), (tok.line, tok.col)))
                }
                TokenKind::FloatLit(val) => {
                    self.next();
                    Ok(Expr::Lit(Literal::Float(val), (tok.line, tok.col)))
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
                                "{}:{}: Expected operator or keyword in expr, got {:?}",
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
                                        "{}:{}: Expected variable name, got {:?}",
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
                            self.expect_kind(TokenKind::Colon)?;
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
                                        "{}:{}: Expected variable name, got {:?}",
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
                            let else_b = self.parse_expr()?;
                            Expr::If {
                                cond: Box::new(cond),
                                then_branch: Box::new(then_b),
                                else_branch: Box::new(else_b),
                                span,
                            }
                        }
                        "loop" => {
                            let var = match self.next() {
                                Some(Token {
                                    kind: TokenKind::Symbol(s),
                                    ..
                                }) => s,
                                Some(tok) => {
                                    return Err(format!(
                                        "{}:{}: Expected loop var, got {:?}",
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
                                        "{}:{}: Expected func name in call, got {:?}",
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
                            } else if s.starts_with("ok:") {
                                Some(parse_scalar_type_str(&s[3..], span)?)
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
                            } else if s.starts_with("err:") {
                                Some(parse_scalar_type_str(&s[4..], span)?)
                            } else {
                                None
                            };
                            let val = self.parse_expr()?;
                            Expr::Err(Box::new(val), explicit_ty, span)
                        }
                        "make" => {
                            let target = self.expect_symbol("Union.variant in make", span)?;
                            let Some(dot) = target.rfind('.') else {
                                return Err(format!("{}:{}: make names a variant as Union.variant, got {}", span.0, span.1, target));
                            };
                            let mut args = Vec::new();
                            while !matches!(self.peek_kind(), Some(TokenKind::RParen) | None) {
                                args.push(self.parse_expr()?);
                            }
                            Expr::Make { union_name: target[..dot].to_string(), variant: target[dot + 1..].to_string(), args, span }
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
                        "block" => {
                            let mut body = Vec::new();
                            while let Some(kind) = self.peek_kind() {
                                if *kind == TokenKind::RParen {
                                    break;
                                }
                                body.push(self.parse_expr()?);
                            }
                            Expr::Block(body, span)
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
                                        "{}:{}: Expected 'ok' arm in match_result, got {:?}",
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
                                        "{}:{}: Expected ok var, got {:?}",
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
                                        "{}:{}: Expected 'err' arm in match_result, got {:?}",
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
                                        "{}:{}: Expected err var, got {:?}",
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
                        "new" => {
                            let struct_name = match self.next() {
                                Some(Token {
                                    kind: TokenKind::Symbol(s),
                                    ..
                                }) => s,
                                Some(tok) => {
                                    return Err(format!(
                                        "{}:{}: Expected struct name in new, got {:?}",
                                        tok.line, tok.col, tok.kind
                                    ))
                                }
                                None => {
                                    return Err(format!(
                                        "{}:{}: Expected struct name in new, got EOF",
                                        span.0, span.1
                                    ))
                                }
                            };
                            Expr::NewStruct { struct_name, span }
                        }
                        "get" => {
                            let ptr = self.parse_expr()?;
                            let field_tok = match self.next() {
                                Some(Token {
                                    kind: TokenKind::Symbol(s),
                                    ..
                                }) => s,
                                Some(tok) => {
                                    return Err(format!(
                                        "{}:{}: Expected 'Struct.field' symbol in get, got {:?}",
                                        tok.line, tok.col, tok.kind
                                    ))
                                }
                                None => {
                                    return Err(format!(
                                        "{}:{}: Expected 'Struct.field' symbol in get, got EOF",
                                        span.0, span.1
                                    ))
                                }
                            };
                            // Split at the last '.': `compiler.Node.next` is struct `compiler.Node`, field `next`.
                            let parts: Vec<&str> = match field_tok.rsplit_once('.') {
                                Some((st, f)) if !st.is_empty() && !f.is_empty() => vec![st, f],
                                _ => {
                                    return Err(format!(
                                        "{}:{}: Expected 'Struct.field' symbol in get, got '{}'",
                                        span.0, span.1, field_tok
                                    ))
                                }
                            };
                            Expr::GetField {
                                struct_name: parts[0].to_string(),
                                field_name: parts[1].to_string(),
                                ptr: Box::new(ptr),
                                span,
                            }
                        }
                        "put" => {
                            let ptr = self.parse_expr()?;
                            let field_tok = match self.next() {
                                Some(Token {
                                    kind: TokenKind::Symbol(s),
                                    ..
                                }) => s,
                                Some(tok) => {
                                    return Err(format!(
                                        "{}:{}: Expected 'Struct.field' symbol in put, got {:?}",
                                        tok.line, tok.col, tok.kind
                                    ))
                                }
                                None => {
                                    return Err(format!(
                                        "{}:{}: Expected 'Struct.field' symbol in put, got EOF",
                                        span.0, span.1
                                    ))
                                }
                            };
                            let val = self.parse_expr()?;
                            // Split at the last '.': `compiler.Node.next` is struct `compiler.Node`, field `next`.
                            let parts: Vec<&str> = match field_tok.rsplit_once('.') {
                                Some((st, f)) if !st.is_empty() && !f.is_empty() => vec![st, f],
                                _ => {
                                    return Err(format!(
                                        "{}:{}: Expected 'Struct.field' symbol in put, got '{}'",
                                        span.0, span.1, field_tok
                                    ))
                                }
                            };
                            Expr::PutField {
                                struct_name: parts[0].to_string(),
                                field_name: parts[1].to_string(),
                                ptr: Box::new(ptr),
                                val: Box::new(val),
                                span,
                            }
                        }
                        "sizeof" => {
                            let struct_name = match self.next() {
                                Some(Token {
                                    kind: TokenKind::Symbol(s),
                                    ..
                                }) => s,
                                Some(tok) => {
                                    return Err(format!(
                                        "{}:{}: Expected struct name in sizeof, got {:?}",
                                        tok.line, tok.col, tok.kind
                                    ))
                                }
                                None => {
                                    return Err(format!(
                                        "{}:{}: Expected struct name in sizeof, got EOF",
                                        span.0, span.1
                                    ))
                                }
                            };
                            Expr::Sizeof { struct_name, span }
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
                            Expr::Cast { ty: Type::Ptr(Box::new(Type::Struct(name))), addr: Box::new(addr), span }
                        }
                        "arr.cast" => {
                            let elem = self.parse_type()?;
                            let addr = self.parse_expr()?;
                            Expr::Cast { ty: Type::Array(Box::new(elem)), addr: Box::new(addr), span }
                        }
                        "ptr.addr" | "arr.addr" | "enum.ord" => {
                            let val = self.parse_expr()?;
                            let kind = match head.as_str() {
                                "ptr.addr" => AddrKind::Ptr,
                                "arr.addr" => AddrKind::Arr,
                                _ => AddrKind::Enum,
                            };
                            Expr::Addr { val: Box::new(val), kind, span }
                        }
                        "enum.cast" => {
                            let name = self.expect_symbol("enum name in enum.cast", span)?;
                            let val = self.parse_expr()?;
                            Expr::Cast { ty: Type::Enum(name), addr: Box::new(val), span }
                        }
                        "arr.new" => {
                            let elem_ty = self.parse_type()?;
                            let size = self.parse_expr()?;
                            Expr::ArrNew {
                                elem_ty,
                                size: Box::new(size),
                                span,
                            }
                        }
                        "arr.get" => {
                            let elem_ty = self.parse_type()?;
                            let ptr = self.parse_expr()?;
                            let index = self.parse_expr()?;
                            Expr::ArrGet {
                                elem_ty,
                                ptr: Box::new(ptr),
                                index: Box::new(index),
                                span,
                            }
                        }
                        "arr.set" => {
                            let elem_ty = self.parse_type()?;
                            let ptr = self.parse_expr()?;
                            let index = self.parse_expr()?;
                            let val = self.parse_expr()?;
                            Expr::ArrSet {
                                elem_ty,
                                ptr: Box::new(ptr),
                                index: Box::new(index),
                                val: Box::new(val),
                                span,
                            }
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
                                "mem.load_f32" => OpCode::MemLoadF32,
                                "mem.load_f64" => OpCode::MemLoadF64,
                                "mem.store8" => OpCode::MemStore8,
                                "mem.store32" => OpCode::MemStore32,
                                "mem.store64" => OpCode::MemStore64,
                                "mem.store_f32" => OpCode::MemStoreF32,
                                "mem.store_f64" => OpCode::MemStoreF64,
                                "mem.alloc" => OpCode::MemAlloc,
                                "mem.free" => OpCode::MemFree,
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
                                "f64.sqrt" => OpCode::F64Sqrt,
                                "i64.trunc_f64_s" => OpCode::I64TruncF64S,
                                "f64.reinterpret_i64" => OpCode::F64ReinterpretI64,
                                "i64.reinterpret_f64" => OpCode::I64ReinterpretF64,
                                "i64.extend_u" => OpCode::I64ExtendU,
                                "i32.wrap" => OpCode::I32Wrap,
                                other => {
                                    return Err(format!(
                                        "{}:{}: Unknown op/keyword: {}",
                                        span.0, span.1, other
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
                    "{}:{}: Unexpected token parsing expression: {:?}",
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
