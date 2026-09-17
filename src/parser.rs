use crate::ast::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    LParen,
    RParen,
    LBracket,
    RBracket,
    Colon,
    Arrow,
    Symbol(String),
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    BoolLit(bool),
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn tokenize(input: &str) -> Vec<Token> {
        let mut tokens = Vec::new();
        let mut chars = input.chars().peekable();

        while let Some(&ch) = chars.peek() {
            match ch {
                ' ' | '\t' | '\r' | '\n' => {
                    chars.next();
                }
                '(' => {
                    tokens.push(Token::LParen);
                    chars.next();
                }
                ')' => {
                    tokens.push(Token::RParen);
                    chars.next();
                }
                '[' => {
                    tokens.push(Token::LBracket);
                    chars.next();
                }
                ']' => {
                    tokens.push(Token::RBracket);
                    chars.next();
                }
                ':' => {
                    tokens.push(Token::Colon);
                    chars.next();
                }
                '"' => {
                    chars.next(); // consume open quote
                    let mut s = String::new();
                    while let Some(&c) = chars.peek() {
                        if c == '"' {
                            chars.next();
                            break;
                        }
                        s.push(c);
                        chars.next();
                    }
                    tokens.push(Token::StringLit(s));
                }
                ';' => {
                    // Line comment
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c == '\n' {
                            break;
                        }
                    }
                }
                _ => {
                    let mut symbol = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_whitespace() || c == '(' || c == ')' || c == '[' || c == ']' || c == ':' {
                            break;
                        }
                        symbol.push(c);
                        chars.next();
                    }

                    if symbol == "->" {
                        tokens.push(Token::Arrow);
                    } else if symbol == "true" {
                        tokens.push(Token::BoolLit(true));
                    } else if symbol == "false" {
                        tokens.push(Token::BoolLit(false));
                    } else if let Ok(i) = symbol.parse::<i64>() {
                        tokens.push(Token::IntLit(i));
                    } else if let Ok(f) = symbol.parse::<f64>() {
                        tokens.push(Token::FloatLit(f));
                    } else if !symbol.is_empty() {
                        tokens.push(Token::Symbol(symbol));
                    }
                }
            }
        }
        tokens
    }

    pub fn parse(input: &str) -> Result<Module, String> {
        let tokens = Self::tokenize(input);
        let mut parser = Parser { tokens, pos: 0 };
        parser.parse_module()
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        if self.pos < self.tokens.len() {
            let t = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(t)
        } else {
            None
        }
    }

    fn expect(&mut self, expected: Token) -> Result<(), String> {
        let t = self.next();
        if t.as_ref() == Some(&expected) {
            Ok(())
        } else {
            Err(format!("Expected {:?}, got {:?}", expected, t))
        }
    }

    fn parse_module(&mut self) -> Result<Module, String> {
        self.expect(Token::LParen)?;
        match self.next() {
            Some(Token::Symbol(s)) if s == "module" => {}
            other => return Err(format!("Expected 'module' symbol, got {:?}", other)),
        }

        let name = match self.next() {
            Some(Token::Symbol(s)) => s,
            other => return Err(format!("Expected module name, got {:?}", other)),
        };

        let mut imports = Vec::new();
        let mut functions = Vec::new();
        while let Some(Token::LParen) = self.peek() {
            if self.is_import_ahead() {
                imports.push(self.parse_import()?);
            } else {
                functions.push(self.parse_fn_def()?);
            }
        }

        self.expect(Token::RParen)?;
        Ok(Module { name, imports, functions })
    }

    fn is_import_ahead(&self) -> bool {
        if self.pos + 1 < self.tokens.len() {
            if let Token::Symbol(s) = &self.tokens[self.pos + 1] {
                return s == "import";
            }
        }
        false
    }

    fn parse_import(&mut self) -> Result<Import, String> {
        self.expect(Token::LParen)?;
        match self.next() {
            Some(Token::Symbol(s)) if s == "import" => {}
            other => return Err(format!("Expected 'import', got {:?}", other)),
        }
        let name = match self.next() {
            Some(Token::Symbol(s)) => s,
            other => return Err(format!("Expected module name in import, got {:?}", other)),
        };
        let alias = match self.peek() {
            Some(Token::Symbol(s)) if s == "as" => {
                self.next();
                match self.next() {
                    Some(Token::Symbol(a)) => Some(a),
                    other => return Err(format!("Expected alias after 'as', got {:?}", other)),
                }
            }
            _ => None,
        };
        self.expect(Token::RParen)?;
        Ok(Import { name, alias })
    }

    fn parse_fn_def(&mut self) -> Result<FnDef, String> {
        self.expect(Token::LParen)?;
        match self.next() {
            Some(Token::Symbol(s)) if s == "fn" => {}
            other => return Err(format!("Expected 'fn', got {:?}", other)),
        }

        let name = match self.next() {
            Some(Token::Symbol(s)) => s,
            other => return Err(format!("Expected fn name, got {:?}", other)),
        };

        // Params in [param1:type1 param2:type2]
        self.expect(Token::LBracket)?;
        let mut params = Vec::new();
        while let Some(tok) = self.peek() {
            if *tok == Token::RBracket {
                break;
            }
            let param_name = match self.next() {
                Some(Token::Symbol(s)) => s,
                other => return Err(format!("Expected param name, got {:?}", other)),
            };
            self.expect(Token::Colon)?;
            let ty = self.parse_type()?;
            params.push((param_name, ty));
        }
        self.expect(Token::RBracket)?;

        self.expect(Token::Arrow)?;
        let return_type = self.parse_type()?;

        let mut contracts = Vec::new();
        let mut body = Vec::new();

        while let Some(tok) = self.peek() {
            if *tok == Token::RParen {
                break;
            }
            if *tok == Token::LParen {
                // Check if it's a contract (req ...), (ens ...) or an expression
                if self.is_contract_ahead() {
                    contracts.push(self.parse_contract()?);
                    continue;
                }
            }
            body.push(self.parse_expr()?);
        }

        self.expect(Token::RParen)?;
        Ok(FnDef {
            name,
            params,
            return_type,
            contracts,
            body,
        })
    }

    fn is_contract_ahead(&self) -> bool {
        if self.pos + 1 < self.tokens.len() {
            if let Token::Symbol(s) = &self.tokens[self.pos + 1] {
                return s == "req" || s == "ens" || s == "inv";
            }
        }
        false
    }

    fn parse_contract(&mut self) -> Result<Contract, String> {
        self.expect(Token::LParen)?;
        let kind = match self.next() {
            Some(Token::Symbol(s)) => s,
            other => return Err(format!("Expected contract opcode, got {:?}", other)),
        };
        let expr = self.parse_expr()?;
        self.expect(Token::RParen)?;

        match kind.as_str() {
            "req" => Ok(Contract::Requires(expr)),
            "ens" => Ok(Contract::Ensures(expr)),
            "inv" => Ok(Contract::Invariant(expr)),
            _ => Err(format!("Unknown contract opcode: {}", kind)),
        }
    }

    fn parse_type(&mut self) -> Result<Type, String> {
        match self.next() {
            Some(Token::Symbol(s)) => match s.as_str() {
                "i32" => Ok(Type::I32),
                "i64" => Ok(Type::I64),
                "f32" => Ok(Type::F32),
                "f64" => Ok(Type::F64),
                "bool" => Ok(Type::Bool),
                "str" => Ok(Type::Str),
                "void" => Ok(Type::Void),
                _ => Err(format!("Unknown scalar type: {}", s)),
            },
            Some(Token::LParen) => {
                let kind = match self.next() {
                    Some(Token::Symbol(s)) => s,
                    other => return Err(format!("Expected type constructor, got {:?}", other)),
                };
                if kind == "arr" || kind == "vec" {
                    let elem_ty = self.parse_type()?;
                    let len = match self.next() {
                        Some(Token::IntLit(i)) => i as usize,
                        other => return Err(format!("Expected length integer, got {:?}", other)),
                    };
                    self.expect(Token::RParen)?;
                    if kind == "arr" {
                        Ok(Type::Array(Box::new(elem_ty), len))
                    } else {
                        Ok(Type::Vector(Box::new(elem_ty), len))
                    }
                } else {
                    Err(format!("Unknown compound type: {}", kind))
                }
            }
            other => Err(format!("Expected type token, got {:?}", other)),
        }
    }

    pub fn parse_expr(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Token::IntLit(i)) => {
                let val = *i;
                self.next();
                Ok(Expr::Lit(Literal::Int(val)))
            }
            Some(Token::FloatLit(f)) => {
                let val = *f;
                self.next();
                Ok(Expr::Lit(Literal::Float(val)))
            }
            Some(Token::BoolLit(b)) => {
                let val = *b;
                self.next();
                Ok(Expr::Lit(Literal::Bool(val)))
            }
            Some(Token::StringLit(s)) => {
                let val = s.clone();
                self.next();
                Ok(Expr::Lit(Literal::Str(val)))
            }
            Some(Token::Symbol(s)) => {
                let val = s.clone();
                self.next();
                Ok(Expr::Var(val))
            }
            Some(Token::LParen) => {
                self.next(); // consume LParen
                let head = match self.next() {
                    Some(Token::Symbol(s)) => s,
                    other => return Err(format!("Expected operator or keyword in expr, got {:?}", other)),
                };

                let expr = match head.as_str() {
                    "let" => {
                        let name = match self.next() {
                            Some(Token::Symbol(s)) => s,
                            other => return Err(format!("Expected variable name, got {:?}", other)),
                        };
                        self.expect(Token::Colon)?;
                        let ty = self.parse_type()?;
                        let val = self.parse_expr()?;
                        Expr::Let {
                            name,
                            ty,
                            val: Box::new(val),
                        }
                    }
                    "set!" => {
                        let name = match self.next() {
                            Some(Token::Symbol(s)) => s,
                            other => return Err(format!("Expected variable name, got {:?}", other)),
                        };
                        let val = self.parse_expr()?;
                        Expr::Set {
                            name,
                            val: Box::new(val),
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
                        }
                    }
                    "loop" => {
                        let var = match self.next() {
                            Some(Token::Symbol(s)) => s,
                            other => return Err(format!("Expected loop var, got {:?}", other)),
                        };
                        let start = self.parse_expr()?;
                        let end = self.parse_expr()?;
                        let step = self.parse_expr()?;
                        let mut body = Vec::new();
                        while let Some(tok) = self.peek() {
                            if *tok == Token::RParen {
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
                        }
                    }
                    "while" => {
                        let cond = self.parse_expr()?;
                        let mut body = Vec::new();
                        while let Some(tok) = self.peek() {
                            if *tok == Token::RParen {
                                break;
                            }
                            body.push(self.parse_expr()?);
                        }
                        Expr::While {
                            cond: Box::new(cond),
                            body,
                        }
                    }
                    "call" => {
                        let func = match self.next() {
                            Some(Token::Symbol(s)) => s,
                            other => return Err(format!("Expected func name in call, got {:?}", other)),
                        };
                        let mut args = Vec::new();
                        while let Some(tok) = self.peek() {
                            if *tok == Token::RParen {
                                break;
                            }
                            args.push(self.parse_expr()?);
                        }
                        Expr::Call { func, args }
                    }
                    "ok" => {
                        let val = self.parse_expr()?;
                        Expr::Ok(Box::new(val))
                    }
                    "err" => {
                        let val = self.parse_expr()?;
                        Expr::Err(Box::new(val))
                    }
                    "block" => {
                        let mut body = Vec::new();
                        while let Some(tok) = self.peek() {
                            if *tok == Token::RParen {
                                break;
                            }
                            body.push(self.parse_expr()?);
                        }
                        Expr::Block(body)
                    }
                    "match_result" => {
                        let res_expr = self.parse_expr()?;
                        self.expect(Token::LParen)?;
                        match self.next() {
                            Some(Token::Symbol(s)) if s == "ok" => {}
                            other => return Err(format!("Expected 'ok' arm in match_result, got {:?}", other)),
                        }
                        let ok_var = match self.next() {
                            Some(Token::Symbol(s)) => s,
                            other => return Err(format!("Expected ok var, got {:?}", other)),
                        };
                        let mut ok_body = Vec::new();
                        while let Some(tok) = self.peek() {
                            if *tok == Token::RParen {
                                break;
                            }
                            ok_body.push(self.parse_expr()?);
                        }
                        self.expect(Token::RParen)?;

                        self.expect(Token::LParen)?;
                        match self.next() {
                            Some(Token::Symbol(s)) if s == "err" => {}
                            other => return Err(format!("Expected 'err' arm in match_result, got {:?}", other)),
                        }
                        let err_var = match self.next() {
                            Some(Token::Symbol(s)) => s,
                            other => return Err(format!("Expected err var, got {:?}", other)),
                        };
                        let mut err_body = Vec::new();
                        while let Some(tok) = self.peek() {
                            if *tok == Token::RParen {
                                break;
                            }
                            err_body.push(self.parse_expr()?);
                        }
                        self.expect(Token::RParen)?;

                        Expr::MatchResult {
                            expr: Box::new(res_expr),
                            ok_var,
                            ok_body,
                            err_var,
                            err_body,
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
                            "vec.dot" => OpCode::VecDot,
                            "matmul" => OpCode::MatMul,
                            "arr.get" => OpCode::ArrGet,
                            "arr.set" => OpCode::ArrSet,
                            "dom.elem" => OpCode::DomElem,
                            "dom.mount" => OpCode::DomMount,
                            "dom.append" => OpCode::DomAppend,
                            "dom.on" => OpCode::DomOnEvent,
                            "web.alert" => OpCode::WebAlert,
                            "sys.print" => OpCode::SysPrint,
                            "sys.time" => OpCode::SysTime,
                            "sys.exit" => OpCode::SysExit,
                            other => return Err(format!("Unknown op/keyword: {}", other)),
                        };

                        let mut args = Vec::new();
                        while let Some(tok) = self.peek() {
                            if *tok == Token::RParen {
                                break;
                            }
                            args.push(self.parse_expr()?);
                        }
                        Expr::Op { op, args }
                    }
                };

                self.expect(Token::RParen)?;
                Ok(expr)
            }
            other => Err(format!("Unexpected token parsing expression: {:?}", other)),
        }
    }
}
