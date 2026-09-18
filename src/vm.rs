use crate::ast::*;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

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

/// Linear memory and the bump-allocator cursor, shared (via `Arc<Mutex<..>>`
/// on `VM`) across every real OS thread spawned by `thread.spawn` - this is
/// what makes `atomic.*` and `mem.*` ops actually mean something under real
/// concurrency, instead of each thread getting its own disconnected copy.
pub struct SharedMemory {
    pub bytes: Vec<u8>,
    pub heap_ptr: usize,
}

pub struct VM {
    functions: Arc<HashMap<String, FnDef>>,
    globals: HashMap<String, Value>,
    pub shared: Arc<Mutex<SharedMemory>>,
    fd_table: HashMap<i32, File>,
    next_fd: i32,
    thread_handles: HashMap<i32, JoinHandle<Result<Value, String>>>,
    next_thread_id: i32,
}

impl VM {
    pub fn new() -> Self {
        VM {
            functions: Arc::new(HashMap::new()),
            globals: HashMap::new(),
            shared: Arc::new(Mutex::new(SharedMemory {
                bytes: vec![0u8; 1024 * 1024], // 1MB linear Wasm memory
                heap_ptr: 1024,
            })),
            fd_table: HashMap::new(),
            next_fd: 3,
            thread_handles: HashMap::new(),
            next_thread_id: 1,
        }
    }

    /// A fresh VM sharing this one's function table and linear memory - used
    /// by `thread.spawn` so the spawned OS thread runs against the same
    /// program and the same memory, but with its own call stack, its own
    /// file descriptors, and its own thread-handle table (join handles
    /// aren't transferable across threads in this model).
    fn spawn_child(&self) -> VM {
        VM {
            functions: Arc::clone(&self.functions),
            globals: HashMap::new(),
            shared: Arc::clone(&self.shared),
            fd_table: HashMap::new(),
            next_fd: 3,
            thread_handles: HashMap::new(),
            next_thread_id: 1,
        }
    }

    /// Convenience accessor for host code (CLI, agent server, tests) that
    /// needs to seed or inspect linear memory without reaching into the
    /// mutex directly.
    pub fn read_bytes(&self, ptr: usize, len: usize) -> Vec<u8> {
        self.shared.lock().unwrap().bytes[ptr..ptr + len].to_vec()
    }

    pub fn write_bytes(&self, ptr: usize, data: &[u8]) {
        let mut mem = self.shared.lock().unwrap();
        mem.bytes[ptr..ptr + data.len()].copy_from_slice(data);
    }

    pub fn load_module(&mut self, module: Module) {
        let mut map = (*self.functions).clone();
        for f in module.functions {
            map.insert(f.name.clone(), f);
        }
        self.functions = Arc::new(map);
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
                Literal::Int(i) => Ok(Value::Int((*i as i32) as i64)),
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
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int((x as i32).wrapping_add(y as i32) as i64)),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x + y)),
                    (Value::Str(x), Value::Str(y)) => Ok(Value::Str(format!("{}{}", x, y))),
                    _ => Err("Invalid types for +".to_string()),
                }
            }
            OpCode::Sub => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int((x as i32).wrapping_sub(y as i32) as i64)),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x - y)),
                    _ => Err("Invalid types for -".to_string()),
                }
            }
            OpCode::BitXor => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(((x as i32) ^ (y as i32)) as i64)),
                    _ => Err("Invalid types for ^".to_string()),
                }
            }
            OpCode::Shl => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => {
                        let shift = (y as u32) & 31;
                        Ok(Value::Int((x as i32).wrapping_shl(shift) as i64))
                    }
                    _ => Err("Invalid types for shl".to_string()),
                }
            }
            OpCode::Shr => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => {
                        let shift = (y as u32) & 31;
                        Ok(Value::Int((x as i32).wrapping_shr(shift) as i64))
                    }
                    _ => Err("Invalid types for shr".to_string()),
                }
            }
            OpCode::ShrU => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => {
                        let shift = (y as u32) & 31;
                        Ok(Value::Int(((x as i32 as u32).wrapping_shr(shift) as i32) as i64))
                    }
                    _ => Err("Invalid types for shru".to_string()),
                }
            }
            OpCode::BitAnd => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(((x as i32) & (y as i32)) as i64)),
                    _ => Err("Invalid types for bitand".to_string()),
                }
            }
            OpCode::BitOr => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(((x as i32) | (y as i32)) as i64)),
                    _ => Err("Invalid types for bitor".to_string()),
                }
            }
            OpCode::MemLoad8 => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("mem.load8 requires Int ptr".to_string()),
                };
                let mem = self.shared.lock().unwrap();
                if ptr >= mem.bytes.len() {
                    return Err(format!("Memory load out of bounds: ptr {}", ptr));
                }
                Ok(Value::Int(mem.bytes[ptr] as i64))
            }
            OpCode::MemStore8 => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("mem.store8 requires Int ptr".to_string()),
                };
                let val = match self.eval_expr(&args[1], scope)? {
                    Value::Int(i) => (i & 0xFF) as u8,
                    _ => return Err("mem.store8 requires Int val".to_string()),
                };
                let mut mem = self.shared.lock().unwrap();
                if ptr >= mem.bytes.len() {
                    return Err(format!("Memory store out of bounds: ptr {}", ptr));
                }
                mem.bytes[ptr] = val;
                Ok(Value::Void)
            }
            OpCode::MemLoad32 => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("mem.load32 requires Int ptr".to_string()),
                };
                let mem = self.shared.lock().unwrap();
                if ptr + 4 > mem.bytes.len() {
                    return Err(format!("Memory load out of bounds: ptr {}", ptr));
                }
                let bytes: [u8; 4] = mem.bytes[ptr..ptr + 4].try_into().unwrap();
                Ok(Value::Int(i32::from_le_bytes(bytes) as i64))
            }
            OpCode::MemLoad64 => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("mem.load64 requires Int ptr".to_string()),
                };
                let mem = self.shared.lock().unwrap();
                if ptr + 8 > mem.bytes.len() {
                    return Err(format!("Memory load out of bounds: ptr {}", ptr));
                }
                let bytes: [u8; 8] = mem.bytes[ptr..ptr + 8].try_into().unwrap();
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
                let mut mem = self.shared.lock().unwrap();
                if ptr + 4 > mem.bytes.len() {
                    return Err(format!("Memory store out of bounds: ptr {}", ptr));
                }
                mem.bytes[ptr..ptr + 4].copy_from_slice(&val.to_le_bytes());
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
                let mut mem = self.shared.lock().unwrap();
                if ptr + 8 > mem.bytes.len() {
                    return Err(format!("Memory store out of bounds: ptr {}", ptr));
                }
                mem.bytes[ptr..ptr + 8].copy_from_slice(&val.to_le_bytes());
                Ok(Value::Void)
            }
            OpCode::MemAlloc => {
                let size = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("mem.alloc requires Int size".to_string()),
                };
                let mut mem = self.shared.lock().unwrap();
                let allocated_ptr = mem.heap_ptr;
                mem.heap_ptr += size;
                Ok(Value::Int(allocated_ptr as i64))
            }
            OpCode::MemFree => Ok(Value::Void),
            // Atomics: the whole read-modify-write happens while holding the
            // one lock on `shared`, so these are genuinely atomic across real
            // OS threads spawned by thread.spawn, not just single-threaded
            // bookkeeping.
            OpCode::AtomicAdd => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("atomic.add requires Int ptr".to_string()),
                };
                let val = match self.eval_expr(&args[1], scope)? {
                    Value::Int(i) => i as i32,
                    _ => return Err("atomic.add requires Int val".to_string()),
                };
                let mut mem = self.shared.lock().unwrap();
                let bytes: [u8; 4] = mem.bytes[ptr..ptr + 4].try_into().unwrap();
                let prev = i32::from_le_bytes(bytes);
                let new_val = prev.wrapping_add(val);
                mem.bytes[ptr..ptr + 4].copy_from_slice(&new_val.to_le_bytes());
                Ok(Value::Int(prev as i64))
            }
            OpCode::AtomicCas => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("atomic.cas requires Int ptr".to_string()),
                };
                let expected = match self.eval_expr(&args[1], scope)? {
                    Value::Int(i) => i as i32,
                    _ => return Err("atomic.cas requires Int expected".to_string()),
                };
                let new_val = match self.eval_expr(&args[2], scope)? {
                    Value::Int(i) => i as i32,
                    _ => return Err("atomic.cas requires Int new_val".to_string()),
                };
                let mut mem = self.shared.lock().unwrap();
                let bytes: [u8; 4] = mem.bytes[ptr..ptr + 4].try_into().unwrap();
                let prev = i32::from_le_bytes(bytes);
                if prev == expected {
                    mem.bytes[ptr..ptr + 4].copy_from_slice(&new_val.to_le_bytes());
                    Ok(Value::Bool(true))
                } else {
                    Ok(Value::Bool(false))
                }
            }
            // Real spinlock: the memory word at `ptr` (0=unlocked, 1=locked)
            // IS the lock, shared for real across threads. Each attempt takes
            // the mutex just long enough to check-and-set, then releases it
            // before retrying, so a thread holding the AIPL-level lock isn't
            // blocked from calling atomic.unlock by a spinning contender.
            OpCode::AtomicLock => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("atomic.lock requires Int ptr".to_string()),
                };
                loop {
                    {
                        let mut mem = self.shared.lock().unwrap();
                        if ptr + 4 > mem.bytes.len() {
                            return Err(format!("atomic.lock out of bounds: ptr {}", ptr));
                        }
                        let bytes: [u8; 4] = mem.bytes[ptr..ptr + 4].try_into().unwrap();
                        if i32::from_le_bytes(bytes) == 0 {
                            mem.bytes[ptr..ptr + 4].copy_from_slice(&1i32.to_le_bytes());
                            return Ok(Value::Void);
                        }
                    }
                    std::thread::yield_now();
                }
            }
            OpCode::AtomicUnlock => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("atomic.unlock requires Int ptr".to_string()),
                };
                let mut mem = self.shared.lock().unwrap();
                if ptr + 4 > mem.bytes.len() {
                    return Err(format!("atomic.unlock out of bounds: ptr {}", ptr));
                }
                mem.bytes[ptr..ptr + 4].copy_from_slice(&0i32.to_le_bytes());
                Ok(Value::Void)
            }
            OpCode::Mul => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int((x as i32).wrapping_mul(y as i32) as i64)),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x * y)),
                    _ => Err("Invalid types for *".to_string()),
                }
            }
            OpCode::Div => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => {
                        let x32 = x as i32;
                        let y32 = y as i32;
                        if y32 == 0 {
                            Err("Division by zero".to_string())
                        } else if x32 == i32::MIN && y32 == -1 {
                            Err("Integer overflow".to_string())
                        } else {
                            Ok(Value::Int((x32 / y32) as i64))
                        }
                    }
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x / y)),
                    _ => Err("Invalid types for /".to_string()),
                }
            }
            OpCode::DivU => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => {
                        let x32 = x as i32 as u32;
                        let y32 = y as i32 as u32;
                        if y32 == 0 {
                            Err("Division by zero".to_string())
                        } else {
                            Ok(Value::Int(((x32 / y32) as i32) as i64))
                        }
                    }
                    _ => Err("Invalid types for divu".to_string()),
                }
            }
            OpCode::Mod => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => {
                        let x32 = x as i32;
                        let y32 = y as i32;
                        if y32 == 0 {
                            Err("Division by zero".to_string())
                        } else if x32 == i32::MIN && y32 == -1 {
                            Ok(Value::Int(0))
                        } else {
                            Ok(Value::Int((x32 % y32) as i64))
                        }
                    }
                    _ => Err("Invalid types for %".to_string()),
                }
            }
            OpCode::RemU => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => {
                        let x32 = x as i32 as u32;
                        let y32 = y as i32 as u32;
                        if y32 == 0 {
                            Err("Division by zero".to_string())
                        } else {
                            Ok(Value::Int(((x32 % y32) as i32) as i64))
                        }
                    }
                    _ => Err("Invalid types for remu".to_string()),
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
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Bool((x as i32) < (y as i32))),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Bool(x < y)),
                    _ => Err("Invalid types for <".to_string()),
                }
            }
            OpCode::Lte => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Bool((x as i32) <= (y as i32))),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Bool(x <= y)),
                    _ => Err("Invalid types for <=".to_string()),
                }
            }
            OpCode::Gt => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Bool((x as i32) > (y as i32))),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Bool(x > y)),
                    _ => Err("Invalid types for >".to_string()),
                }
            }
            OpCode::Gte => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Bool((x as i32) >= (y as i32))),
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
            // Real file I/O: path is read as UTF-8 bytes out of linear memory
            // (pointer+length, not a language-level string) so this matches
            // the pointer/length convention WASI's path_open needs too - the
            // signature won't need to change when a wasm+WASI backend for
            // this lands. flags: 0 = read-only, non-zero = write/create/truncate.
            OpCode::FsOpen => {
                let path_ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("fs.open requires Int path_ptr".to_string()),
                };
                let path_len = match self.eval_expr(&args[1], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("fs.open requires Int path_len".to_string()),
                };
                let flags = match self.eval_expr(&args[2], scope)? {
                    Value::Int(i) => i,
                    _ => return Err("fs.open requires Int flags".to_string()),
                };
                let path_bytes = self.read_bytes(path_ptr, path_len);
                let path_str = match std::str::from_utf8(&path_bytes) {
                    Ok(s) => s,
                    Err(_) => return Ok(Value::Int(-1)),
                };
                let opened = if flags == 0 {
                    File::open(path_str)
                } else {
                    OpenOptions::new().write(true).create(true).truncate(true).open(path_str)
                };
                match opened {
                    Ok(file) => {
                        let fd = self.next_fd;
                        self.next_fd += 1;
                        self.fd_table.insert(fd, file);
                        Ok(Value::Int(fd as i64))
                    }
                    Err(_) => Ok(Value::Int(-1)),
                }
            }
            OpCode::FsRead => {
                let fd = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as i32,
                    _ => return Err("fs.read requires Int fd".to_string()),
                };
                let buf_ptr = match self.eval_expr(&args[1], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("fs.read requires Int buf_ptr".to_string()),
                };
                let max_len = match self.eval_expr(&args[2], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("fs.read requires Int max_len".to_string()),
                };
                let file = match self.fd_table.get_mut(&fd) {
                    Some(f) => f,
                    None => return Ok(Value::Int(-1)),
                };
                let mut buf = vec![0u8; max_len];
                match file.read(&mut buf) {
                    Ok(n) => {
                        self.write_bytes(buf_ptr, &buf[..n]);
                        Ok(Value::Int(n as i64))
                    }
                    Err(_) => Ok(Value::Int(-1)),
                }
            }
            OpCode::FsWrite => {
                let fd = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as i32,
                    _ => return Err("fs.write requires Int fd".to_string()),
                };
                let buf_ptr = match self.eval_expr(&args[1], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("fs.write requires Int buf_ptr".to_string()),
                };
                let len = match self.eval_expr(&args[2], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("fs.write requires Int len".to_string()),
                };
                let data = self.read_bytes(buf_ptr, len);
                let file = match self.fd_table.get_mut(&fd) {
                    Some(f) => f,
                    None => return Ok(Value::Int(-1)),
                };
                match file.write(&data) {
                    Ok(n) => Ok(Value::Int(n as i64)),
                    Err(_) => Ok(Value::Int(-1)),
                }
            }
            OpCode::FsClose => {
                let fd = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as i32,
                    _ => return Err("fs.close requires Int fd".to_string()),
                };
                if self.fd_table.remove(&fd).is_some() {
                    Ok(Value::Int(0))
                } else {
                    Ok(Value::Int(-1))
                }
            }
            OpCode::FsDelete => {
                let path_ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("fs.delete requires Int path_ptr".to_string()),
                };
                let path_len = match self.eval_expr(&args[1], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("fs.delete requires Int path_len".to_string()),
                };
                let path_bytes = self.read_bytes(path_ptr, path_len);
                let path_str = match std::str::from_utf8(&path_bytes) {
                    Ok(s) => s,
                    Err(_) => return Ok(Value::Int(-1)),
                };
                match std::fs::remove_file(path_str) {
                    Ok(_) => Ok(Value::Int(0)),
                    Err(_) => Ok(Value::Int(-1)),
                }
            }
            // Real OS thread spawn: the named function is looked up in the
            // SAME function table (Arc-shared, not copied) and run on a real
            // std::thread with a fresh child VM that shares `self.shared`
            // linear memory. AIPL has no first-class function values yet, so
            // the target function is named by (ptr,len) into linear memory -
            // matching the fs.* pointer/length convention above - rather than
            // passed as a function pointer.
            OpCode::ThreadSpawn => {
                let name_ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("thread.spawn requires Int fn_name_ptr".to_string()),
                };
                let name_len = match self.eval_expr(&args[1], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("thread.spawn requires Int fn_name_len".to_string()),
                };
                let arg = match self.eval_expr(&args[2], scope)? {
                    Value::Int(i) => i,
                    _ => return Err("thread.spawn requires Int arg".to_string()),
                };
                let name_bytes = self.read_bytes(name_ptr, name_len);
                let fn_name = match String::from_utf8(name_bytes) {
                    Ok(s) => s,
                    Err(_) => return Err("thread.spawn: function name is not valid UTF-8".to_string()),
                };
                if !self.functions.contains_key(&fn_name) {
                    return Err(format!("thread.spawn: unknown function '{}'", fn_name));
                }
                let mut child = self.spawn_child();
                let handle = std::thread::spawn(move || child.invoke(&fn_name, vec![Value::Int(arg)]));
                let tid = self.next_thread_id;
                self.next_thread_id += 1;
                self.thread_handles.insert(tid, handle);
                Ok(Value::Int(tid as i64))
            }
            OpCode::ThreadJoin => {
                let tid = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as i32,
                    _ => return Err("thread.join requires Int handle".to_string()),
                };
                let handle = match self.thread_handles.remove(&tid) {
                    Some(h) => h,
                    None => return Err(format!("thread.join: unknown thread handle {}", tid)),
                };
                match handle.join() {
                    Ok(Ok(Value::Int(i))) => Ok(Value::Int(i)),
                    Ok(Ok(_)) => Ok(Value::Int(0)),
                    Ok(Err(e)) => Err(format!("Spawned thread's function failed: {}", e)),
                    Err(_) => Err("Spawned thread panicked".to_string()),
                }
            }
            OpCode::MemLoadF32 | OpCode::MemLoadF64 | OpCode::MemStoreF32 | OpCode::MemStoreF64 => {
                Err(format!("{:?} not supported in VM backend: floating point memory ops not implemented", op))
            }
            OpCode::ArrGet | OpCode::ArrSet => {
                Err(format!("{:?} not supported in VM backend: array ops not implemented", op))
            }
            OpCode::SysTime | OpCode::SysExit => {
                Err(format!("{:?} not supported in VM backend: system ops not implemented", op))
            }
        }
    }
}
