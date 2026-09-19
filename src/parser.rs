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
        let tokens = Self::tokenize(input)?;
        let mut parser = Parser {
            tokens,
            pos: 0,
            last_pos: (1, 1),
        };
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
        let mut functions = Vec::new();
        while let Some(TokenKind::LParen) = self.peek_kind() {
            if self.is_import_ahead() {
                imports.push(self.parse_import()?);
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
            functions,
        })
    }

    fn is_import_ahead(&self) -> bool {
        if self.pos + 1 < self.tokens.len() {
            if let TokenKind::Symbol(s) = &self.tokens[self.pos + 1].kind {
                return s == "import";
            }
        }
        false
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

    fn parse_type(&mut self) -> Result<Type, String> {
        let (l, c) = self.cur_pos();
        match self.next() {
            Some(Token {
                kind: TokenKind::Symbol(s),
                line,
                col,
            }) => match s.as_str() {
                "i32" => Ok(Type::I32),
                "i64" => Err("i64 type is unsupported".to_string()),
                "f32" => Ok(Type::F32),
                "f64" => Ok(Type::F64),
                "bool" => Ok(Type::Bool),
                "str" => Ok(Type::Str),
                "void" => Ok(Type::Void),
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
                if kind == "arr" || kind == "vec" {
                    let elem_ty = self.parse_type()?;
                    let len = match self.next() {
                        Some(Token {
                            kind: TokenKind::IntLit(i),
                            ..
                        }) => i as usize,
                        Some(tok) => {
                            return Err(format!(
                                "{}:{}: Expected length integer, got {:?}",
                                tok.line, tok.col, tok.kind
                            ))
                        }
                        None => {
                            return Err(format!(
                                "{}:{}: Expected length integer, got EOF",
                                line, col
                            ))
                        }
                    };
                    self.expect_kind(TokenKind::RParen)?;
                    if kind == "arr" {
                        Ok(Type::Array(Box::new(elem_ty), len))
                    } else {
                        Ok(Type::Vector(Box::new(elem_ty), len))
                    }
                } else {
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
                        "ok" => {
                            let val = self.parse_expr()?;
                            Expr::Ok(Box::new(val), span)
                        }
                        "err" => {
                            let val = self.parse_expr()?;
                            Expr::Err(Box::new(val), span)
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
                                "gt" => OpCode::Gt,
                                "gte" => OpCode::Gte,
                                "and" => OpCode::And,
                                "or" => OpCode::Or,
                                "not" => OpCode::Not,
                                "arr.get" => OpCode::ArrGet,
                                "arr.set" => OpCode::ArrSet,
                                "sys.print" => OpCode::SysPrint,
                                "sys.time" => OpCode::SysTime,
                                "sys.exit" => OpCode::SysExit,
                                "fs.open" => OpCode::FsOpen,
                                "fs.read" => OpCode::FsRead,
                                "fs.write" => OpCode::FsWrite,
                                "fs.close" => OpCode::FsClose,
                                "fs.delete" => OpCode::FsDelete,
                                "thread.spawn" => OpCode::ThreadSpawn,
                                "thread.join" => OpCode::ThreadJoin,
                                "divu" => OpCode::DivU,
                                "remu" => OpCode::RemU,
                                "i64.extend_s" => OpCode::I64ExtendS,
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
