use crate::ast::*;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Array(Vec<Value>),
    Ok(Box<Value>),
    Err(Box<Value>),
    Void,
}

pub struct VM {
    functions: HashMap<String, FnDef>,
    globals: HashMap<String, Value>,
    pub linear_memory: Vec<u8>,
    pub heap_ptr: usize,
    pub locks: HashMap<usize, bool>,
}

impl VM {
    pub fn new() -> Self {
        VM {
            functions: HashMap::new(),
            globals: HashMap::new(),
            linear_memory: vec![0u8; 1024 * 1024], // 1MB linear Wasm memory
            heap_ptr: 1024,
            locks: HashMap::new(),
        }
    }

    pub fn load_module(&mut self, module: Module) {
        for f in module.functions {
            self.functions.insert(f.name.clone(), f);
        }
    }

    pub fn invoke(&mut self, fn_name: &str, args: Vec<Value>) -> Result<Value, String> {
        let f = self
            .functions
            .get(fn_name)
            .ok_or_else(|| format!("Function '{}' not found in VM", fn_name))?
            .clone();

        if args.len() != f.params.len() {
            return Err(format!(
                "Function '{}' expects {} arguments, got {}",
                fn_name,
                f.params.len(),
                args.len()
            ));
        }

        let mut scope = HashMap::new();
        for (i, (param_name, _)) in f.params.iter().enumerate() {
            scope.insert(param_name.clone(), args[i].clone());
        }

        // Evaluate Pre-Condition Contracts (req ...)
        for contract in &f.contracts {
            if let Contract::Requires(expr) = contract {
                let res = self.eval_expr(expr, &mut scope)?;
                if res != Value::Bool(true) {
                    return Err(format!(
                        "Pre-condition (req {:?}) failed in function '{}'",
                        expr, fn_name
                    ));
                }
            }
        }

        // Execute function body
        let mut last_val = Value::Void;
        for expr in &f.body {
            last_val = self.eval_expr(expr, &mut scope)?;
        }

        // Evaluate Post-Condition Contracts (ens ...)
        for contract in &f.contracts {
            if let Contract::Ensures(expr) = contract {
                let mut contract_scope = scope.clone();
                contract_scope.insert("res".to_string(), last_val.clone());
                let res = self.eval_expr(expr, &mut contract_scope)?;
                if res != Value::Bool(true) {
                    return Err(format!(
                        "Post-condition (ens {:?}) failed in function '{}'",
                        expr, fn_name
                    ));
                }
            }
        }

        Ok(last_val)
    }

    pub fn eval_expr(&mut self, expr: &Expr, scope: &mut HashMap<String, Value>) -> Result<Value, String> {
        match expr {
            Expr::Lit(lit) => match lit {
                Literal::Int(i) => Ok(Value::Int(*i)),
                Literal::Float(f) => Ok(Value::Float(*f)),
                Literal::Bool(b) => Ok(Value::Bool(*b)),
                Literal::Str(s) => Ok(Value::Str(s.clone())),
            },
            Expr::Var(name) => {
                if let Some(val) = scope.get(name) {
                    Ok(val.clone())
                } else if let Some(val) = self.globals.get(name) {
                    Ok(val.clone())
                } else {
                    Err(format!("VM: Variable '{}' not found in scope", name))
                }
            }
            Expr::Let { name, ty: _, val } => {
                let v = self.eval_expr(val, scope)?;
                scope.insert(name.clone(), v.clone());
                Ok(v)
            }
            Expr::Set { name, val } => {
                let v = self.eval_expr(val, scope)?;
                if scope.contains_key(name) {
                    scope.insert(name.clone(), v.clone());
                } else {
                    self.globals.insert(name.clone(), v.clone());
                }
                Ok(v)
            }
            Expr::If { cond, then_branch, else_branch } => {
                let c = self.eval_expr(cond, scope)?;
                if let Value::Bool(b) = c {
                    if b {
                        self.eval_expr(then_branch, scope)
                    } else {
                        self.eval_expr(else_branch, scope)
                    }
                } else {
                    Err("If condition must evaluate to boolean".to_string())
                }
            }
            Expr::Loop { var, start, end, step, body } => {
                let s_val = match self.eval_expr(start, scope)? {
                    Value::Int(i) => i,
                    _ => return Err("Loop start must be Int".to_string()),
                };
                let e_val = match self.eval_expr(end, scope)? {
                    Value::Int(i) => i,
                    _ => return Err("Loop end must be Int".to_string()),
                };
                let st_val = match self.eval_expr(step, scope)? {
                    Value::Int(i) => i,
                    _ => return Err("Loop step must be Int".to_string()),
                };

                let mut curr = s_val;
                while curr <= e_val {
                    scope.insert(var.clone(), Value::Int(curr));
                    for stmt in body {
                        self.eval_expr(stmt, scope)?;
                    }
                    curr += st_val;
                }
                Ok(Value::Void)
            }
            Expr::While { cond, body } => {
                while let Value::Bool(true) = self.eval_expr(cond, scope)? {
                    for stmt in body {
                        self.eval_expr(stmt, scope)?;
                    }
                }
                Ok(Value::Void)
            }
            Expr::Call { func, args } => {
                let mut evaluated_args = Vec::new();
                for arg in args {
                    evaluated_args.push(self.eval_expr(arg, scope)?);
                }
                self.invoke(func, evaluated_args)
            }
            Expr::Op { op, args } => self.eval_op(op, args, scope),
            Expr::Ok(val) => {
                let inner = self.eval_expr(val, scope)?;
                Ok(Value::Ok(Box::new(inner)))
            }
            Expr::Err(err) => {
                let inner = self.eval_expr(err, scope)?;
                Ok(Value::Err(Box::new(inner)))
            }
            Expr::MatchResult { expr, ok_var, ok_body, err_var, err_body } => {
                let res_val = self.eval_expr(expr, scope)?;
                match res_val {
                    Value::Ok(inner) => {
                        let mut local_scope = scope.clone();
                        local_scope.insert(ok_var.clone(), *inner);
                        let mut last = Value::Void;
                        for stmt in ok_body {
                            last = self.eval_expr(stmt, &mut local_scope)?;
                        }
                        Ok(last)
                    }
                    Value::Err(inner) => {
                        let mut local_scope = scope.clone();
                        local_scope.insert(err_var.clone(), *inner);
                        let mut last = Value::Void;
                        for stmt in err_body {
                            last = self.eval_expr(stmt, &mut local_scope)?;
                        }
                        Ok(last)
                    }
                    other => Err(format!("Expected Result type in match_result, got {:?}", other)),
                }
            }
            Expr::Block(exprs) => {
                let mut last = Value::Void;
                for e in exprs {
                    last = self.eval_expr(e, scope)?;
                }
                Ok(last)
            }
        }
    }

    fn eval_op(&mut self, op: &OpCode, args: &[Expr], scope: &mut HashMap<String, Value>) -> Result<Value, String> {
        match op {
            OpCode::Add => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x + y)),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x + y)),
                    (Value::Str(x), Value::Str(y)) => Ok(Value::Str(format!("{}{}", x, y))),
                    _ => Err("Invalid types for +".to_string()),
                }
            }
            OpCode::Sub => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x - y)),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x - y)),
                    _ => Err("Invalid types for -".to_string()),
                }
            }
            OpCode::BitXor => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x ^ y)),
                    _ => Err("Invalid types for ^".to_string()),
                }
            }
            OpCode::Shl => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x << y)),
                    _ => Err("Invalid types for shl".to_string()),
                }
            }
            OpCode::Shr => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x >> y)),
                    _ => Err("Invalid types for shr".to_string()),
                }
            }
            OpCode::BitAnd => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x & y)),
                    _ => Err("Invalid types for bitand".to_string()),
                }
            }
            OpCode::BitOr => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x | y)),
                    _ => Err("Invalid types for bitor".to_string()),
                }
            }
            OpCode::MemLoad32 => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("mem.load32 requires Int ptr".to_string()),
                };
                if ptr + 4 > self.linear_memory.len() {
                    return Err(format!("Memory load out of bounds: ptr {}", ptr));
                }
                let bytes: [u8; 4] = self.linear_memory[ptr..ptr + 4].try_into().unwrap();
                Ok(Value::Int(i32::from_le_bytes(bytes) as i64))
            }
            OpCode::MemLoad64 => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("mem.load64 requires Int ptr".to_string()),
                };
                if ptr + 8 > self.linear_memory.len() {
                    return Err(format!("Memory load out of bounds: ptr {}", ptr));
                }
                let bytes: [u8; 8] = self.linear_memory[ptr..ptr + 8].try_into().unwrap();
                Ok(Value::Int(i64::from_le_bytes(bytes)))
            }
            OpCode::MemStore32 => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("mem.store32 requires Int ptr".to_string()),
                };
                let val = match self.eval_expr(&args[1], scope)? {
                    Value::Int(i) => i as i32,
                    _ => return Err("mem.store32 requires Int val".to_string()),
                };
                if ptr + 4 > self.linear_memory.len() {
                    return Err(format!("Memory store out of bounds: ptr {}", ptr));
                }
                self.linear_memory[ptr..ptr + 4].copy_from_slice(&val.to_le_bytes());
                Ok(Value::Void)
            }
            OpCode::MemStore64 => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("mem.store64 requires Int ptr".to_string()),
                };
                let val = match self.eval_expr(&args[1], scope)? {
                    Value::Int(i) => i,
                    _ => return Err("mem.store64 requires Int val".to_string()),
                };
                if ptr + 8 > self.linear_memory.len() {
                    return Err(format!("Memory store out of bounds: ptr {}", ptr));
                }
                self.linear_memory[ptr..ptr + 8].copy_from_slice(&val.to_le_bytes());
                Ok(Value::Void)
            }
            OpCode::MemAlloc => {
                let size = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("mem.alloc requires Int size".to_string()),
                };
                let allocated_ptr = self.heap_ptr;
                self.heap_ptr += size;
                Ok(Value::Int(allocated_ptr as i64))
            }
            OpCode::MemFree => Ok(Value::Void),
            OpCode::AtomicAdd => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("atomic.add requires Int ptr".to_string()),
                };
                let val = match self.eval_expr(&args[1], scope)? {
                    Value::Int(i) => i as i32,
                    _ => return Err("atomic.add requires Int val".to_string()),
                };
                let bytes: [u8; 4] = self.linear_memory[ptr..ptr + 4].try_into().unwrap();
                let prev = i32::from_le_bytes(bytes);
                let new_val = prev + val;
                self.linear_memory[ptr..ptr + 4].copy_from_slice(&new_val.to_le_bytes());
                Ok(Value::Int(prev as i64))
            }
            OpCode::AtomicLock => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("atomic.lock requires Int ptr".to_string()),
                };
                self.locks.insert(ptr, true);
                Ok(Value::Void)
            }
            OpCode::AtomicUnlock => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("atomic.unlock requires Int ptr".to_string()),
                };
                self.locks.insert(ptr, false);
                Ok(Value::Void)
            }
            OpCode::Mul => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x * y)),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x * y)),
                    _ => Err("Invalid types for *".to_string()),
                }
            }
            OpCode::Div => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => {
                        if y == 0 {
                            Err("Division by zero".to_string())
                        } else {
                            Ok(Value::Int(x / y))
                        }
                    }
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x / y)),
                    _ => Err("Invalid types for /".to_string()),
                }
            }
            OpCode::Eq => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                Ok(Value::Bool(a == b))
            }
            OpCode::Neq => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                Ok(Value::Bool(a != b))
            }
            OpCode::Lt => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Bool(x < y)),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Bool(x < y)),
                    _ => Err("Invalid types for <".to_string()),
                }
            }
            OpCode::Lte => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Bool(x <= y)),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Bool(x <= y)),
                    _ => Err("Invalid types for <=".to_string()),
                }
            }
            OpCode::Gt => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Bool(x > y)),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Bool(x > y)),
                    _ => Err("Invalid types for >".to_string()),
                }
            }
            OpCode::Gte => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Bool(x >= y)),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Bool(x >= y)),
                    _ => Err("Invalid types for >=".to_string()),
                }
            }
            OpCode::And => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Bool(x), Value::Bool(y)) => Ok(Value::Bool(x && y)),
                    _ => Err("Invalid types for and".to_string()),
                }
            }
            OpCode::Or => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Bool(x), Value::Bool(y)) => Ok(Value::Bool(x || y)),
                    _ => Err("Invalid types for or".to_string()),
                }
            }
            OpCode::Not => {
                let a = self.eval_expr(&args[0], scope)?;
                match a {
                    Value::Bool(x) => Ok(Value::Bool(!x)),
                    _ => Err("Invalid type for not".to_string()),
                }
            }
            OpCode::SysPrint => {
                for arg in args {
                    let v = self.eval_expr(arg, scope)?;
                    match v {
                        Value::Str(s) => println!("{}", s),
                        v => println!("{:?}", v),
                    }
                }
                Ok(Value::Void)
            }
            _ => Ok(Value::Int(0)),
        }
    }
}
