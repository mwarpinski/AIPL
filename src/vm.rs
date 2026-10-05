use crate::ast::*;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// An `i32`. Stored in an i64 but every operation truncates both operands
    /// to i32 first, so no value wider than 32 bits is ever observable.
    Int(i64),
    /// An `i64`. Operations wrap at 64 bits, matching wasm `i64.*`.
    Int64(i64),
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
}

/// One wasm page.
pub const PAGE_SIZE: usize = 65536;
/// Initial linear memory in pages (1 MiB). Matches the wasm backend's memory
/// minimum so `mem.grow` returns the same old-size in both backends.
pub const INITIAL_PAGES: usize = 16;
/// Maximum linear memory in pages. Matches the wasm backend's memory maximum.
pub const MAX_PAGES: usize = 1024;
/// Address of the heap cursor word read and written by `mem.alloc`.
pub const HEAP_PTR_ADDR: usize = 0;
/// First heap address handed out by `mem.alloc`. Bytes below it are the
/// runtime block (see AIPL_SPEC.md, Memory layout).
pub const HEAP_START: u32 = 1024;

/// A `return`, `break`, or `continue` that is unwinding. eval_expr sets it and
/// returns Value::Void; statement sequences stop when it is set, loops consume
/// Break/Continue, and invoke consumes Return. The checker makes these
/// statements (type void), so they only ever occur where a sequence, loop, or
/// if branch is evaluating them.
#[derive(Debug, Clone, PartialEq)]
enum Flow {
    Break,
    Continue,
    Return(Value),
}

pub struct VM {
    /// Shared bodies: a call takes a reference-counted handle, not a deep copy
    /// of the function's AST (audit B13).
    functions: Arc<HashMap<String, Arc<FnDef>>>,
    /// Function names in load order: `(ref f)` is f's position here, which is
    /// also its slot in the wasm backend's function table.
    fn_order: Arc<Vec<String>>,
    structs: Arc<HashMap<String, StructDef>>,
    flow: Option<Flow>,
    pub shared: Arc<Mutex<SharedMemory>>,
    fd_table: HashMap<i32, File>,
    next_fd: i32,
    thread_handles: HashMap<i32, JoinHandle<Result<Value, String>>>,
    /// String literal -> address of its interned bytes, laid out exactly as
    /// the wasm backend's data segment (wasm::string_layout).
    strings: Arc<HashMap<String, u32>>,
    /// First heap address; bytes 1024..heap_start hold the literals.
    heap_start: u32,
    /// What `args.*` report: the program's command line, argv[0] first.
    /// Empty unless the host sets it (`set_args`).
    args: Arc<Vec<String>>,
    /// What `env.*` report: `KEY=VALUE` entries. The process environment
    /// unless the host replaces it (`set_env`).
    env: Arc<Vec<String>>,
    /// Bytes `fs.read` on fd 0 returns, if the host set them (`set_stdin`);
    /// otherwise fd 0 is the process's stdin.
    stdin: Option<Arc<Mutex<Vec<u8>>>>,
}

impl VM {
    pub fn new() -> Self {
        VM {
            functions: Arc::new(HashMap::new()),
            fn_order: Arc::new(Vec::new()),
            structs: Arc::new(HashMap::new()),
            flow: None,
            shared: Arc::new(Mutex::new(SharedMemory {
                bytes: {
                    // 16 pages, with the heap cursor at address 0 pre-set to
                    // HEAP_START - exactly what the wasm backend's data
                    // segment does, so both backends start from one layout.
                    let mut bytes = vec![0u8; INITIAL_PAGES * PAGE_SIZE];
                    bytes[HEAP_PTR_ADDR..HEAP_PTR_ADDR + 4].copy_from_slice(&HEAP_START.to_le_bytes());
                    bytes
                },
            })),
            fd_table: HashMap::new(),
            next_fd: 3,
            thread_handles: HashMap::new(),
            strings: Arc::new(HashMap::new()),
            heap_start: HEAP_START,
            args: Arc::new(Vec::new()),
            env: Arc::new(std::env::vars().map(|(k, v)| format!("{k}={v}")).collect()),
            stdin: None,
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
            fn_order: Arc::clone(&self.fn_order),
            structs: Arc::clone(&self.structs),
            flow: None,
            shared: Arc::clone(&self.shared),
            fd_table: HashMap::new(),
            next_fd: 3,
            thread_handles: HashMap::new(),
            strings: Arc::clone(&self.strings),
            heap_start: self.heap_start,
            args: Arc::clone(&self.args),
            env: Arc::clone(&self.env),
            stdin: self.stdin.clone(),
        }
    }

    /// The command line `args.*` report, argv[0] first.
    pub fn set_args(&mut self, args: Vec<String>) {
        self.args = Arc::new(args);
    }

    /// What `fs.read` on fd 0 returns, instead of the process's stdin.
    pub fn set_stdin(&mut self, bytes: Vec<u8>) {
        self.stdin = Some(Arc::new(Mutex::new(bytes)));
    }

    /// The environment `env.*` report, as (key, value) pairs.
    pub fn set_env(&mut self, env: Vec<(String, String)>) {
        self.env = Arc::new(env.into_iter().map(|(k, v)| format!("{k}={v}")).collect());
    }

    /// args.sizes / env.sizes (`sizes`) and args.get / env.get: the WASI
    /// preview1 layout, so std/os reads both backends the same way. Like
    /// fs.read, the host writes without the store guard; an address outside
    /// memory is a failure (-1), as WASI reports a fault.
    fn wasi_strings(&self, items: &[String], sizes: bool, a: usize, b: usize) -> Value {
        let total: usize = items.iter().map(|s| s.len() + 1).sum();
        let mem_len = self.shared.lock().unwrap().bytes.len();
        if sizes {
            if a + 4 > mem_len || b + 4 > mem_len {
                return Value::Int(-1);
            }
            self.write_bytes(a, &(items.len() as u32).to_le_bytes());
            self.write_bytes(b, &(total as u32).to_le_bytes());
        } else {
            if a + 4 * items.len() > mem_len || b + total > mem_len {
                return Value::Int(-1);
            }
            let mut at = b;
            for (i, s) in items.iter().enumerate() {
                self.write_bytes(a + 4 * i, &(at as u32).to_le_bytes());
                self.write_bytes(at, s.as_bytes());
                self.write_bytes(at + s.len(), &[0]);
                at += s.len() + 1;
            }
        }
        Value::Int(0)
    }

    fn heap_cursor(&self) -> u32 {
        u32::from_le_bytes(self.read_bytes(HEAP_PTR_ADDR, 4).try_into().unwrap())
    }

    /// Rejects stores below the heap other than to cells 4..64 (see
    /// check_write_address); `heap_start` covers this module's literals.
    fn check_write(&self, op: &str, ptr: usize) -> Result<(), String> {
        check_write_address(op, ptr, self.heap_start as usize)
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
        // The first module loaded into a fresh VM gets the wasm backend's
        // string layout: literals at 1024, the heap after them. Literals of
        // later modules are copied onto the heap when used (materialize_str).
        if self.strings.is_empty() && self.heap_cursor() == HEAP_START {
            if let Ok(layout) = crate::compiler::wasm::string_layout(&module) {
                if !layout.blob.is_empty() {
                    self.write_bytes(HEAP_START as usize, &layout.blob);
                    self.write_bytes(HEAP_PTR_ADDR, &layout.heap_start.to_le_bytes());
                    self.heap_start = layout.heap_start;
                    self.strings = Arc::new(layout.addrs);
                }
            }
        }
        let mut f_map = (*self.functions).clone();
        let mut order = (*self.fn_order).clone();
        for f in module.functions {
            if !f_map.contains_key(&f.name) {
                order.push(f.name.clone());
            }
            f_map.insert(f.name.clone(), Arc::new(f));
        }
        self.fn_order = Arc::new(order);
        self.functions = Arc::new(f_map);

        let mut s_map = (*self.structs).clone();
        for s in module.structs {
            s_map.insert(s.name.clone(), s);
        }
        self.structs = Arc::new(s_map);
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
                    return Err(contract_failure("Pre-condition", "req", expr, fn_name, &f.params, &scope, None));
                }
            }
        }

        // Execute function body; a pending return ends it with its value.
        let mut last_val = Value::Void;
        for expr in &f.body {
            last_val = self.eval_expr(expr, &mut scope)?;
            if let Some(flow) = self.flow.take() {
                if let Flow::Return(v) = flow {
                    last_val = v;
                }
                break;
            }
        }

        // Evaluate Post-Condition Contracts (ens ...)
        for contract in &f.contracts {
            if let Contract::Ensures(expr) = contract {
                let mut contract_scope = scope.clone();
                contract_scope.insert("res".to_string(), last_val.clone());
                let res = self.eval_expr(expr, &mut contract_scope)?;
                if res != Value::Bool(true) {
                    return Err(contract_failure("Post-condition", "ens", expr, fn_name, &f.params, &scope, Some(&last_val)));
                }
            }
        }

        Ok(last_val)
    }

    /// Evaluates `expr`. While a `return`/`break`/`continue` is unwinding
    /// (`flow` is set), nothing more is evaluated: an operand that jumped
    /// (`(+ 1 (block (break) 2))`) stops its enclosing expression, so later
    /// operands and their side effects never run, as in wasm, where `br`
    /// leaves the expression immediately. The enclosing op may then see a
    /// `Void` operand and fail; that failure is discarded because the jump
    /// supersedes it.
    pub fn eval_expr(&mut self, expr: &Expr, scope: &mut HashMap<String, Value>) -> Result<Value, String> {
        if self.flow.is_some() {
            return Ok(Value::Void);
        }
        match self.eval_expr_inner(expr, scope) {
            Err(_) if self.flow.is_some() => Ok(Value::Void),
            r => r,
        }
    }

    fn eval_expr_inner(&mut self, expr: &Expr, scope: &mut HashMap<String, Value>) -> Result<Value, String> {
        match expr {
            Expr::Lit(lit, _) => match lit {
                Literal::Int(i) => Ok(Value::Int((*i as i32) as i64)),
                Literal::Int64(i) => Ok(Value::Int64(*i)),
                Literal::Float(f) => Ok(Value::Float(*f)),
                Literal::Bool(b) => Ok(Value::Bool(*b)),
                Literal::Str(s) => Ok(Value::Str(s.clone())),
            },
            Expr::Var(name, _) => {
                if let Some(val) = scope.get(name) {
                    Ok(val.clone())
                } else {
                    Err(format!("VM: Variable '{}' not found in scope", name))
                }
            }
            Expr::Let { name, val, .. } => {
                let v = self.eval_expr(val, scope)?;
                scope.insert(name.clone(), v);
                Ok(Value::Void)
            }
            Expr::Set { name, val, .. } => {
                let v = self.eval_expr(val, scope)?;
                if scope.contains_key(name) {
                    scope.insert(name.clone(), v);
                    Ok(Value::Void)
                } else {
                    Err(format!("VM: Undefined variable '{}' in set!", name))
                }
            }
            Expr::If { cond, then_branch, else_branch, .. } => {
                let c = self.eval_expr(cond, scope)?;
                let keys_before: std::collections::HashSet<String> = scope.keys().cloned().collect();
                let res = if let Value::Bool(b) = c {
                    if b {
                        self.eval_expr(then_branch, scope)
                    } else {
                        self.eval_expr(else_branch, scope)
                    }
                } else {
                    Err("If condition must evaluate to boolean".to_string())
                };
                scope.retain(|k, _| keys_before.contains(k));
                res
            }
            // Same order as the wasm lowering: start once; then each iteration
            // evaluates end, exits if var > end, runs the body, evaluates step,
            // and adds it to var (which the body may have set!).
            Expr::Loop { var, start, end, step, body, .. } => {
                let as_i32 = |v: Value, what: &str| match v {
                    Value::Int(i) => Ok(i as i32),
                    _ => Err(format!("Loop {} must be Int", what)),
                };
                // start, then end and step, each evaluated once, in that order
                // (as in wasm, where end and step go into two hidden locals)
                let s_val = as_i32(self.eval_expr(start, scope)?, "start")?;
                let keys_before: std::collections::HashSet<String> = scope.keys().cloned().collect();
                scope.insert(var.clone(), Value::Int(s_val as i64));
                let e_val = as_i32(self.eval_expr(end, scope)?, "end")?;
                let st_val = as_i32(self.eval_expr(step, scope)?, "step")?;
                if self.flow.is_some() {
                    scope.retain(|k, _| keys_before.contains(k));
                    return Ok(Value::Void);
                }
                loop {
                    let curr = as_i32(scope.get(var).cloned().unwrap_or(Value::Int(0)), "variable")?;
                    if curr > e_val {
                        break;
                    }
                    let iter_keys: std::collections::HashSet<String> = scope.keys().cloned().collect();
                    self.eval_seq(body, scope)?;
                    scope.retain(|k, _| iter_keys.contains(k));
                    match self.flow {
                        Some(Flow::Break) => {
                            self.flow = None;
                            break;
                        }
                        Some(Flow::Continue) => self.flow = None,
                        Some(Flow::Return(_)) => break,
                        None => {}
                    }
                    let curr = as_i32(scope.get(var).cloned().unwrap_or(Value::Int(0)), "variable")?;
                    scope.insert(var.clone(), Value::Int(curr.wrapping_add(st_val) as i64));
                }
                scope.retain(|k, _| keys_before.contains(k));
                Ok(Value::Void)
            }
            Expr::While { cond, body, .. } => {
                let keys_before: std::collections::HashSet<String> = scope.keys().cloned().collect();
                while let Value::Bool(true) = self.eval_expr(cond, scope)? {
                    let iter_keys: std::collections::HashSet<String> = scope.keys().cloned().collect();
                    self.eval_seq(body, scope)?;
                    scope.retain(|k, _| iter_keys.contains(k));
                    match self.flow {
                        Some(Flow::Break) => {
                            self.flow = None;
                            break;
                        }
                        Some(Flow::Continue) => self.flow = None,
                        Some(Flow::Return(_)) => break,
                        None => {}
                    }
                }
                scope.retain(|k, _| keys_before.contains(k));
                Ok(Value::Void)
            }
            Expr::Call { func, args, .. } => {
                let mut evaluated_args = Vec::new();
                for arg in args {
                    evaluated_args.push(self.eval_expr(arg, scope)?);
                }
                self.invoke(func, evaluated_args)
            }
            Expr::Op { op, args, .. } => self.eval_op(op, args, scope),
            Expr::Return { val, .. } => {
                let v = match val {
                    Some(e) => self.eval_expr(e, scope)?,
                    None => Value::Void,
                };
                self.flow = Some(Flow::Return(v));
                Ok(Value::Void)
            }
            Expr::Break(_) => {
                self.flow = Some(Flow::Break);
                Ok(Value::Void)
            }
            Expr::Continue(_) => {
                self.flow = Some(Flow::Continue);
                Ok(Value::Void)
            }
            Expr::Ref { name, .. } => match self.fn_order.iter().position(|n| n == name) {
                Some(i) => Ok(Value::Int(i as i64)),
                None => Err(format!("ref: unknown function '{}'", name)),
            },
            // Arguments first, then the function value, as call_indirect evaluates them.
            Expr::CallRef { func, args, .. } => {
                let mut evaluated_args = Vec::new();
                for arg in args {
                    evaluated_args.push(self.eval_expr(arg, scope)?);
                }
                let name = self.fn_ref_name(func, scope, "call_ref")?;
                self.invoke(&name, evaluated_args)
            }
            // A result occupies 8 heap bytes [tag:i32 payload:i32] (tag 0 = ok,
            // 1 = err), allocated before the payload is evaluated, exactly as the
            // wasm backend lays it out, so both backends leave the same heap.
            Expr::Ok(val, _, _) => {
                let inner = self.eval_result_cell(0, val, scope)?;
                Ok(Value::Ok(Box::new(inner)))
            }
            Expr::Err(err, _, _) => {
                let inner = self.eval_result_cell(1, err, scope)?;
                Ok(Value::Err(Box::new(inner)))
            }
            Expr::MatchResult { expr, ok_var, ok_body, err_var, err_body, .. } => {
                let res_val = self.eval_expr(expr, scope)?;
                let keys_before: std::collections::HashSet<String> = scope.keys().cloned().collect();
                let res = match res_val {
                    Value::Ok(inner) => {
                        scope.insert(ok_var.clone(), *inner);
                        self.eval_seq(ok_body, scope)
                    }
                    Value::Err(inner) => {
                        scope.insert(err_var.clone(), *inner);
                        self.eval_seq(err_body, scope)
                    }
                    other => Err(format!("Expected Result type in match_result, got {:?}", other)),
                };
                scope.retain(|k, _| keys_before.contains(k));
                res
            }
            Expr::Block(exprs, _) => {
                let keys_before: std::collections::HashSet<String> = scope.keys().cloned().collect();
                let last = self.eval_seq(exprs, scope);
                scope.retain(|k, _| keys_before.contains(k));
                last
            }
            Expr::NewStruct { struct_name, .. } => {
                let def = self
                    .structs
                    .get(struct_name)
                    .ok_or_else(|| format!("VM: Unknown struct '{}'", struct_name))?
                    .clone();
                let size = crate::checker::get_struct_size(&def)?;
                let ptr = self.alloc_bytes(size);
                Ok(Value::Int(ptr as i64))
            }
            Expr::GetField { struct_name, field_name, ptr, .. } => {
                let def = self
                    .structs
                    .get(struct_name)
                    .ok_or_else(|| format!("VM: Unknown struct '{}'", struct_name))?
                    .clone();
                let (offset, field_ty) = crate::checker::get_field_offset(&def, field_name)?;
                let ptr_val = match self.eval_expr(ptr, scope)? {
                    Value::Int(i) => i as u32 as usize,
                    other => return Err(format!("VM: Expected Int pointer for get, got {:?}", other)),
                };
                let addr = ptr_val + offset;
                self.load_val_at(addr, &field_ty)
            }
            Expr::PutField { struct_name, field_name, ptr, val, .. } => {
                let def = self
                    .structs
                    .get(struct_name)
                    .ok_or_else(|| format!("VM: Unknown struct '{}'", struct_name))?
                    .clone();
                let (offset, field_ty) = crate::checker::get_field_offset(&def, field_name)?;
                let ptr_val = match self.eval_expr(ptr, scope)? {
                    Value::Int(i) => i as u32 as usize,
                    other => return Err(format!("VM: Expected Int pointer for put, got {:?}", other)),
                };
                let addr = ptr_val + offset;
                self.check_write("put", addr)?;
                let val_v = self.eval_expr(val, scope)?;
                self.store_val_at(addr, &field_ty, val_v)?;
                Ok(Value::Void)
            }
            Expr::Sizeof { struct_name, .. } => {
                let def = self
                    .structs
                    .get(struct_name)
                    .ok_or_else(|| format!("VM: Unknown struct '{}'", struct_name))?
                    .clone();
                let size = crate::checker::get_struct_size(&def)?;
                Ok(Value::Int(size as i64))
            }
            Expr::ArrNew { elem_ty, size, .. } => {
                let count = match self.eval_expr(size, scope)? {
                    Value::Int(i) if i >= 0 => i as usize,
                    Value::Int(i) => return Err(format!("Array index out of bounds: invalid array size {}", i)),
                    other => return Err(format!("VM: Expected Int size for arr.new, got {:?}", other)),
                };
                let (elem_size, _) = crate::checker::type_size_and_align(elem_ty)?;
                let total_size = 4 + count * elem_size;
                let raw_ptr = self.alloc_bytes(total_size);
                // Store element count in the 4 bytes before the base pointer
                self.store_val_at(raw_ptr as u32 as usize, &Type::I32, Value::Int(count as i64))?;
                let base_ptr = raw_ptr + 4;
                Ok(Value::Int(base_ptr as i64))
            }
            Expr::ArrGet { elem_ty, ptr, index, .. } => {
                let ptr_val = match self.eval_expr(ptr, scope)? {
                    Value::Int(i) => i as u32 as usize,
                    other => return Err(format!("VM: Expected Int pointer for arr.get, got {:?}", other)),
                };
                let idx_val = match self.eval_expr(index, scope)? {
                    Value::Int(i) => i,
                    other => return Err(format!("VM: Expected Int index for arr.get, got {:?}", other)),
                };
                if ptr_val < 4 {
                    return Err(format!("VM: Invalid array pointer {}", ptr_val));
                }
                let count = self.array_len_at(ptr_val)?;
                if idx_val < 0 || idx_val >= count {
                    return Err(format!(
                        "Array index out of bounds: index {} for array of length {}",
                        idx_val, count
                    ));
                }
                let (elem_size, _) = crate::checker::type_size_and_align(elem_ty)?;
                let addr = ptr_val + (idx_val as usize) * elem_size;
                self.load_val_at(addr, elem_ty)
            }
            // Pointers and arrays are i32 addresses at run time.
            Expr::Null { .. } => Ok(Value::Int(0)),
            Expr::Cast { addr: inner, .. } | Expr::Addr { val: inner, .. } => self.eval_expr(inner, scope),
            Expr::ArrLen { arr, .. } => {
                let p = match self.eval_expr(arr, scope)? {
                    Value::Int(i) => i as u32 as usize,
                    other => return Err(format!("VM: Expected Int array for arr.len, got {:?}", other)),
                };
                if p < 4 {
                    return Err(format!("VM: Invalid array pointer {}", p));
                }
                Ok(Value::Int(self.array_len_at(p)?))
            }
            Expr::ArrSet { elem_ty, ptr, index, val, .. } => {
                let ptr_val = match self.eval_expr(ptr, scope)? {
                    Value::Int(i) => i as u32 as usize,
                    other => return Err(format!("VM: Expected Int pointer for arr.set, got {:?}", other)),
                };
                let idx_val = match self.eval_expr(index, scope)? {
                    Value::Int(i) => i,
                    other => return Err(format!("VM: Expected Int index for arr.set, got {:?}", other)),
                };
                let val_v = self.eval_expr(val, scope)?;
                if ptr_val < 4 {
                    return Err(format!("VM: Invalid array pointer {}", ptr_val));
                }
                let count = self.array_len_at(ptr_val)?;
                if idx_val < 0 || idx_val >= count {
                    return Err(format!(
                        "Array index out of bounds: index {} for array of length {}",
                        idx_val, count
                    ));
                }
                let (elem_size, _) = crate::checker::type_size_and_align(elem_ty)?;
                let addr = ptr_val + (idx_val as usize) * elem_size;
                self.check_write("arr.set", addr)?;
                self.store_val_at(addr, elem_ty, val_v)?;
                Ok(Value::Void)
            }
        }
    }

    fn eval_result_cell(&mut self, tag: i32, payload: &Expr, scope: &mut HashMap<String, Value>) -> Result<Value, String> {
        let cell = self.alloc_bytes(8) as u32 as usize;
        self.store_val_at(cell, &Type::I32, Value::Int(tag as i64))?;
        let inner = self.eval_expr(payload, scope)?;
        if let Value::Int(_) | Value::Bool(_) = inner {
            self.store_val_at(cell + 4, &Type::I32, inner.clone())?;
        }
        Ok(inner)
    }

    /// The address of `s`'s bytes: its interned literal (the same address as
    /// in wasm), or else a fresh heap copy as `[len u32 LE][bytes]`.
    fn materialize_str(&self, op: &str, s: &str) -> Result<usize, String> {
        if let Some(&addr) = self.strings.get(s) {
            return Ok(addr as usize);
        }
        let bytes = s.as_bytes();
        let base = self.alloc_bytes(4 + bytes.len()) as u32 as usize;
        let end = base + 4 + bytes.len();
        let mut mem = self.shared.lock().unwrap();
        if end > mem.bytes.len() {
            return Err(format!("{}: out of memory materialising a {}-byte string", op, bytes.len()));
        }
        mem.bytes[base..base + 4].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        mem.bytes[base + 4..end].copy_from_slice(bytes);
        Ok(base + 4)
    }

    /// Evaluates a statement sequence, stopping at a pending return/break/continue.
    fn eval_seq(&mut self, exprs: &[Expr], scope: &mut HashMap<String, Value>) -> Result<Value, String> {
        let mut last = Value::Void;
        for e in exprs {
            last = self.eval_expr(e, scope)?;
            if self.flow.is_some() {
                break;
            }
        }
        Ok(last)
    }

    /// The function a `(fn ...)` value refers to: its index into `fn_order`.
    /// An index outside the table is an error, as it traps in wasm.
    fn fn_ref_name(&mut self, func: &Expr, scope: &mut HashMap<String, Value>, op: &str) -> Result<String, String> {
        let idx = match self.eval_expr(func, scope)? {
            Value::Int(i) => i,
            other => return Err(format!("{}: expected a function reference, got {:?}", op, other)),
        };
        usize::try_from(idx)
            .ok()
            .and_then(|i| self.fn_order.get(i).cloned())
            .ok_or_else(|| format!("{}: function reference {} is out of range", op, idx))
    }

    /// Element count stored in the 4 bytes before an `arr.new` pointer.
    fn array_len_at(&self, ptr: usize) -> Result<i64, String> {
        let mem = self.shared.lock().unwrap();
        if ptr > mem.bytes.len() {
            return Err(format!("Memory load out of bounds: array pointer {}", ptr));
        }
        let bytes: [u8; 4] = mem.bytes[ptr - 4..ptr].try_into().unwrap();
        Ok(i32::from_le_bytes(bytes) as i64)
    }

    /// Bumps the heap cursor by `size` and returns the old cursor. If the new
    /// cursor is past the end of memory, memory grows by the pages needed to
    /// cover it (the wasm lowering does the same with memory.grow); past the
    /// 1024-page cap nothing grows and the first access beyond the end fails.
    /// Claims `size` bytes rounded up to a multiple of 8, so every block is
    /// 8-aligned (the heap start is), as in compiled code.
    fn alloc_bytes(&self, size: usize) -> i32 {
        let size = (size as i32).wrapping_add(7) & -8;
        let mut mem = self.shared.lock().unwrap();
        let cur: [u8; 4] = mem.bytes[HEAP_PTR_ADDR..HEAP_PTR_ADDR + 4].try_into().unwrap();
        let allocated_ptr = i32::from_le_bytes(cur);
        let next = allocated_ptr.wrapping_add(size as i32);
        mem.bytes[HEAP_PTR_ADDR..HEAP_PTR_ADDR + 4].copy_from_slice(&next.to_le_bytes());
        let have = mem.bytes.len() as u32;
        if next as u32 > have {
            let pages = ((next as u32).wrapping_sub(have).wrapping_add(65535) >> 16) as usize;
            let old_pages = mem.bytes.len() / PAGE_SIZE;
            if old_pages + pages <= MAX_PAGES {
                mem.bytes.resize((old_pages + pages) * PAGE_SIZE, 0);
            }
        }
        allocated_ptr
    }

    fn load_val_at(&self, addr: usize, ty: &Type) -> Result<Value, String> {
        let mem = self.shared.lock().unwrap();
        match ty {
            Type::I32 | Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _) | Type::Enum(_) => {
                if addr + 4 > mem.bytes.len() {
                    return Err(format!("VM memory load out of bounds: address {}", addr));
                }
                let bytes: [u8; 4] = mem.bytes[addr..addr + 4].try_into().unwrap();
                let v = i32::from_le_bytes(bytes);
                Ok(Value::Int(v as i64))
            }
            Type::Bool => {
                if addr + 4 > mem.bytes.len() {
                    return Err(format!("VM memory load out of bounds: address {}", addr));
                }
                let bytes: [u8; 4] = mem.bytes[addr..addr + 4].try_into().unwrap();
                let v = i32::from_le_bytes(bytes);
                Ok(Value::Bool(v != 0))
            }
            Type::Str => {
                // The word is the address of the bytes; the length is in the 4 bytes before them.
                if addr + 4 > mem.bytes.len() {
                    return Err(format!("VM memory load out of bounds: address {}", addr));
                }
                let p = u32::from_le_bytes(mem.bytes[addr..addr + 4].try_into().unwrap()) as usize;
                if p < 4 || p > mem.bytes.len() {
                    return Err(format!("VM: str field at address {} holds invalid string pointer {}", addr, p));
                }
                let len = u32::from_le_bytes(mem.bytes[p - 4..p].try_into().unwrap()) as usize;
                if p + len > mem.bytes.len() {
                    return Err(format!("VM: str at {} with length {} runs past memory", p, len));
                }
                Ok(Value::Str(String::from_utf8_lossy(&mem.bytes[p..p + len]).into_owned()))
            }
            Type::I64 => {
                if addr + 8 > mem.bytes.len() {
                    return Err(format!("VM memory load out of bounds: address {}", addr));
                }
                let bytes: [u8; 8] = mem.bytes[addr..addr + 8].try_into().unwrap();
                let v = i64::from_le_bytes(bytes);
                Ok(Value::Int64(v))
            }
            Type::F32 => {
                if addr + 4 > mem.bytes.len() {
                    return Err(format!("VM memory load out of bounds: address {}", addr));
                }
                let bytes: [u8; 4] = mem.bytes[addr..addr + 4].try_into().unwrap();
                let v = f32::from_le_bytes(bytes);
                Ok(Value::Float(v as f64))
            }
            Type::F64 => {
                if addr + 8 > mem.bytes.len() {
                    return Err(format!("VM memory load out of bounds: address {}", addr));
                }
                let bytes: [u8; 8] = mem.bytes[addr..addr + 8].try_into().unwrap();
                let v = f64::from_le_bytes(bytes);
                Ok(Value::Float(v))
            }
            _ => Err(format!("Unsupported type for memory load: {:?}", ty)),
        }
    }

    fn store_val_at(&self, addr: usize, ty: &Type, val: Value) -> Result<(), String> {
        let val = match (ty, val) {
            (Type::Str, Value::Str(s)) => Value::Int(self.materialize_str("str store", &s)? as i64),
            (_, v) => v,
        };
        let mut mem = self.shared.lock().unwrap();
        match ty {
            Type::I32 | Type::Ptr(_) | Type::Array(_) | Type::Fn(_, _) | Type::Enum(_) | Type::Str => {
                if addr + 4 > mem.bytes.len() {
                    return Err(format!("VM memory store out of bounds: address {}", addr));
                }
                let v = match val {
                    Value::Int(i) => i as i32,
                    Value::Bool(b) => if b { 1 } else { 0 },
                    other => return Err(format!("Expected Int/Bool value for 32-bit store, got {:?}", other)),
                };
                mem.bytes[addr..addr + 4].copy_from_slice(&v.to_le_bytes());
                Ok(())
            }
            Type::Bool => {
                if addr + 4 > mem.bytes.len() {
                    return Err(format!("VM memory store out of bounds: address {}", addr));
                }
                let v: i32 = match val {
                    Value::Bool(b) => if b { 1 } else { 0 },
                    Value::Int(i) => if i != 0 { 1 } else { 0 },
                    other => return Err(format!("Expected Bool/Int value for bool store, got {:?}", other)),
                };
                mem.bytes[addr..addr + 4].copy_from_slice(&v.to_le_bytes());
                Ok(())
            }
            Type::I64 => {
                if addr + 8 > mem.bytes.len() {
                    return Err(format!("VM memory store out of bounds: address {}", addr));
                }
                let v = match val {
                    Value::Int64(i) => i,
                    Value::Int(i) => i,
                    other => return Err(format!("Expected Int64 value for 64-bit store, got {:?}", other)),
                };
                mem.bytes[addr..addr + 8].copy_from_slice(&v.to_le_bytes());
                Ok(())
            }
            Type::F32 => {
                if addr + 4 > mem.bytes.len() {
                    return Err(format!("VM memory store out of bounds: address {}", addr));
                }
                let v = match val {
                    Value::Float(f) => f as f32,
                    other => return Err(format!("Expected Float value for f32 store, got {:?}", other)),
                };
                mem.bytes[addr..addr + 4].copy_from_slice(&v.to_le_bytes());
                Ok(())
            }
            Type::F64 => {
                if addr + 8 > mem.bytes.len() {
                    return Err(format!("VM memory store out of bounds: address {}", addr));
                }
                let v = match val {
                    Value::Float(f) => f,
                    other => return Err(format!("Expected Float value for f64 store, got {:?}", other)),
                };
                mem.bytes[addr..addr + 8].copy_from_slice(&v.to_le_bytes());
                Ok(())
            }
            _ => Err(format!("Unsupported type for memory store: {:?}", ty)),
        }
    }

    fn eval_op(&mut self, op: &OpCode, args: &[Expr], scope: &mut HashMap<String, Value>) -> Result<Value, String> {
        match op {
            OpCode::Add => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int((x as i32).wrapping_add(y as i32) as i64)),
                    (Value::Int64(x), Value::Int64(y)) => Ok(Value::Int64(x.wrapping_add(y))),
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
                    (Value::Int64(x), Value::Int64(y)) => Ok(Value::Int64(x.wrapping_sub(y))),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x - y)),
                    _ => Err("Invalid types for -".to_string()),
                }
            }
            OpCode::BitXor => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(((x as i32) ^ (y as i32)) as i64)),
                    (Value::Int64(x), Value::Int64(y)) => Ok(Value::Int64(x ^ y)),
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
                    (Value::Int64(x), Value::Int64(y)) => {
                        let shift = (y as u32) & 63;
                        Ok(Value::Int64(x.wrapping_shl(shift)))
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
                    (Value::Int64(x), Value::Int64(y)) => {
                        let shift = (y as u32) & 63;
                        Ok(Value::Int64(x.wrapping_shr(shift)))
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
                    (Value::Int64(x), Value::Int64(y)) => {
                        let shift = (y as u32) & 63;
                        Ok(Value::Int64((x as u64).wrapping_shr(shift) as i64))
                    }
                    _ => Err("Invalid types for shru".to_string()),
                }
            }
            OpCode::BitAnd => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(((x as i32) & (y as i32)) as i64)),
                    (Value::Int64(x), Value::Int64(y)) => Ok(Value::Int64(x & y)),
                    _ => Err("Invalid types for bitand".to_string()),
                }
            }
            OpCode::BitOr => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int(((x as i32) | (y as i32)) as i64)),
                    (Value::Int64(x), Value::Int64(y)) => Ok(Value::Int64(x | y)),
                    _ => Err("Invalid types for bitor".to_string()),
                }
            }
            OpCode::MemLoad8 => {
                let i_val = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i,
                    _ => return Err("mem.load8 requires Int ptr".to_string()),
                };
                if i_val < 0 {
                    return Err(format!("Memory load out of bounds: ptr {}", i_val));
                }
                let ptr = i_val as usize;
                let mem = self.shared.lock().unwrap();
                if ptr >= mem.bytes.len() {
                    return Err(format!("Memory load out of bounds: ptr {}", ptr));
                }
                Ok(Value::Int(mem.bytes[ptr] as i64))
            }
            OpCode::MemStore8 => {
                let i_val = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i,
                    _ => return Err("mem.store8 requires Int ptr".to_string()),
                };
                if i_val < 0 {
                    return Err(format!("Memory store out of bounds: ptr {}", i_val));
                }
                let ptr = i_val as usize;
                self.check_write("mem.store8", ptr)?;
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
                let i_val = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i,
                    _ => return Err("mem.load32 requires Int ptr".to_string()),
                };
                if i_val < 0 {
                    return Err(format!("Memory load out of bounds: ptr {}", i_val));
                }
                let ptr = i_val as usize;
                let mem = self.shared.lock().unwrap();
                if ptr.checked_add(4).map_or(true, |end| end > mem.bytes.len()) {
                    return Err(format!("Memory load out of bounds: ptr {}", i_val));
                }
                let bytes: [u8; 4] = mem.bytes[ptr..ptr + 4].try_into().unwrap();
                Ok(Value::Int(i32::from_le_bytes(bytes) as i64))
            }
            OpCode::MemLoad64 => {
                let i_val = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i,
                    _ => return Err("mem.load64 requires Int ptr".to_string()),
                };
                if i_val < 0 {
                    return Err(format!("Memory load out of bounds: ptr {}", i_val));
                }
                let ptr = i_val as usize;
                let mem = self.shared.lock().unwrap();
                if ptr.checked_add(8).map_or(true, |end| end > mem.bytes.len()) {
                    return Err(format!("Memory load out of bounds: ptr {}", i_val));
                }
                let bytes: [u8; 8] = mem.bytes[ptr..ptr + 8].try_into().unwrap();
                Ok(Value::Int64(i64::from_le_bytes(bytes)))
            }
            OpCode::MemStore32 => {
                let i_val = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i,
                    _ => return Err("mem.store32 requires Int ptr".to_string()),
                };
                if i_val < 0 {
                    return Err(format!("Memory store out of bounds: ptr {}", i_val));
                }
                let ptr = i_val as usize;
                self.check_write("mem.store32", ptr)?;
                let val = match self.eval_expr(&args[1], scope)? {
                    Value::Int(i) => i as i32,
                    _ => return Err("mem.store32 requires Int val".to_string()),
                };
                let mut mem = self.shared.lock().unwrap();
                if ptr.checked_add(4).map_or(true, |end| end > mem.bytes.len()) {
                    return Err(format!("Memory store out of bounds: ptr {}", i_val));
                }
                mem.bytes[ptr..ptr + 4].copy_from_slice(&val.to_le_bytes());
                Ok(Value::Void)
            }
            OpCode::MemStore64 => {
                let i_val = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i,
                    _ => return Err("mem.store64 requires Int64 ptr".to_string()),
                };
                if i_val < 0 {
                    return Err(format!("Memory store out of bounds: ptr {}", i_val));
                }
                let ptr = i_val as usize;
                self.check_write("mem.store64", ptr)?;
                let val = match self.eval_expr(&args[1], scope)? {
                    Value::Int64(i) => i,
                    _ => return Err("mem.store64 requires Int64 val".to_string()),
                };
                let mut mem = self.shared.lock().unwrap();
                if ptr.checked_add(8).map_or(true, |end| end > mem.bytes.len()) {
                    return Err(format!("Memory store out of bounds: ptr {}", i_val));
                }
                mem.bytes[ptr..ptr + 8].copy_from_slice(&val.to_le_bytes());
                Ok(Value::Void)
            }
            OpCode::MemAlloc => {
                let size = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("mem.alloc requires Int size".to_string()),
                };
                // The cursor lives IN linear memory at address 0 (not in a Rust
                // field), so VM code, compiled wasm, and self-hosted AIPL all
                // share one allocator state.
                Ok(Value::Int(self.alloc_bytes(size) as i64))
            }
            OpCode::MemFree => Ok(Value::Void),
            OpCode::MemGrow => {
                let pages = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as i32,
                    _ => return Err("mem.grow requires Int pages".to_string()),
                };
                let mut mem = self.shared.lock().unwrap();
                let old_pages = mem.bytes.len() / PAGE_SIZE;
                if pages < 0 || old_pages + pages as usize > MAX_PAGES {
                    // wasm memory.grow reports failure as -1 rather than trapping.
                    return Ok(Value::Int(-1));
                }
                let new_len = (old_pages + pages as usize) * PAGE_SIZE;
                mem.bytes.resize(new_len, 0);
                Ok(Value::Int(old_pages as i64))
            }
            // Atomics: the whole read-modify-write happens while holding the
            // one lock on `shared`, so these are genuinely atomic across real
            // OS threads spawned by thread.spawn, not just single-threaded
            // bookkeeping.
            OpCode::AtomicAdd => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as usize,
                    _ => return Err("atomic.add requires Int ptr".to_string()),
                };
                self.check_write("atomic.add", ptr)?;
                check_atomic_alignment("atomic.add", ptr)?;
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
                self.check_write("atomic.cas", ptr)?;
                check_atomic_alignment("atomic.cas", ptr)?;
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
                self.check_write("atomic.lock", ptr)?;
                check_atomic_alignment("atomic.lock", ptr)?;
                loop {
                    {
                        let mut mem = self.shared.lock().unwrap();
                        if ptr + 4 > mem.bytes.len() {
                            return Err(format!("atomic.lock out of bounds: ptr {}", ptr));
                        }
                        let bytes: [u8; 4] = mem.bytes[ptr..ptr + 4].try_into().unwrap();
                        match i32::from_le_bytes(bytes) {
                            0 => {
                                mem.bytes[ptr..ptr + 4].copy_from_slice(&1i32.to_le_bytes());
                                return Ok(Value::Void);
                            }
                            1 => {} // held by someone else: spin
                            other => {
                                // A lock word is only ever 0 or 1. Anything else means
                                // this address is data, not a mutex (the classic case:
                                // address 0, the heap cursor). Spinning on it would
                                // hang forever, so fail loudly instead.
                                return Err(format!(
                                    "atomic.lock: word at ptr {} holds {}, which is not a lock state (0 = free, 1 = held); this address is data, not a mutex. Allocate a dedicated lock word with (mem.alloc 4)",
                                    ptr, other
                                ));
                            }
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
                self.check_write("atomic.unlock", ptr)?;
                check_atomic_alignment("atomic.unlock", ptr)?;
                let mut mem = self.shared.lock().unwrap();
                if ptr + 4 > mem.bytes.len() {
                    return Err(format!("atomic.unlock out of bounds: ptr {}", ptr));
                }
                let bytes: [u8; 4] = mem.bytes[ptr..ptr + 4].try_into().unwrap();
                let word = i32::from_le_bytes(bytes);
                if word != 1 {
                    return Err(format!(
                        "atomic.unlock: word at ptr {} holds {}, but a held lock holds 1; either this mutex was never locked or this address is data, not a mutex",
                        ptr, word
                    ));
                }
                mem.bytes[ptr..ptr + 4].copy_from_slice(&0i32.to_le_bytes());
                Ok(Value::Void)
            }
            OpCode::Mul => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Int((x as i32).wrapping_mul(y as i32) as i64)),
                    (Value::Int64(x), Value::Int64(y)) => Ok(Value::Int64(x.wrapping_mul(y))),
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
                    (Value::Int64(x), Value::Int64(y)) => {
                        if y == 0 {
                            Err("Division by zero".to_string())
                        } else if x == i64::MIN && y == -1 {
                            Err("Integer overflow".to_string())
                        } else {
                            Ok(Value::Int64(x / y))
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
                    (Value::Int64(x), Value::Int64(y)) => {
                        if y == 0 {
                            Err("Division by zero".to_string())
                        } else {
                            Ok(Value::Int64(((x as u64) / (y as u64)) as i64))
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
                    (Value::Int64(x), Value::Int64(y)) => {
                        if y == 0 {
                            Err("Division by zero".to_string())
                        } else if x == i64::MIN && y == -1 {
                            Ok(Value::Int64(0))
                        } else {
                            Ok(Value::Int64(x % y))
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
                    (Value::Int64(x), Value::Int64(y)) => {
                        if y == 0 {
                            Err("Division by zero".to_string())
                        } else {
                            Ok(Value::Int64(((x as u64) % (y as u64)) as i64))
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
                    (Value::Int64(x), Value::Int64(y)) => Ok(Value::Bool(x < y)),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Bool(x < y)),
                    _ => Err("Invalid types for <".to_string()),
                }
            }
            OpCode::Lte => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Bool((x as i32) <= (y as i32))),
                    (Value::Int64(x), Value::Int64(y)) => Ok(Value::Bool(x <= y)),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Bool(x <= y)),
                    _ => Err("Invalid types for <=".to_string()),
                }
            }
            OpCode::Gt => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Bool((x as i32) > (y as i32))),
                    (Value::Int64(x), Value::Int64(y)) => Ok(Value::Bool(x > y)),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Bool(x > y)),
                    _ => Err("Invalid types for >".to_string()),
                }
            }
            OpCode::Gte => {
                let a = self.eval_expr(&args[0], scope)?;
                let b = self.eval_expr(&args[1], scope)?;
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => Ok(Value::Bool((x as i32) >= (y as i32))),
                    (Value::Int64(x), Value::Int64(y)) => Ok(Value::Bool(x >= y)),
                    (Value::Float(x), Value::Float(y)) => Ok(Value::Bool(x >= y)),
                    _ => Err("Invalid types for >=".to_string()),
                }
            }
            // Short-circuit: the second operand is evaluated only when the
            // first does not decide the result.
            OpCode::And | OpCode::Or => {
                let a = match self.eval_expr(&args[0], scope)? {
                    Value::Bool(x) => x,
                    _ => return Err(format!("Invalid types for {:?}", op)),
                };
                if a == matches!(op, OpCode::Or) {
                    return Ok(Value::Bool(a));
                }
                match self.eval_expr(&args[1], scope)? {
                    Value::Bool(y) => Ok(Value::Bool(y)),
                    _ => Err(format!("Invalid types for {:?}", op)),
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
                if fd == 0 {
                    // stdin: the host-provided bytes (set_stdin), else the real stdin
                    let buf_ptr = match self.eval_expr(&args[1], scope)? {
                        Value::Int(i) => i as u32 as usize,
                        _ => return Err("fs.read requires Int buf_ptr".to_string()),
                    };
                    let max_len = match self.eval_expr(&args[2], scope)? {
                        Value::Int(i) => i.max(0) as usize,
                        _ => return Err("fs.read requires Int max_len".to_string()),
                    };
                    let mut buf = vec![0u8; max_len];
                    let n = match &self.stdin {
                        Some(src) => {
                            let mut src = src.lock().unwrap();
                            let n = max_len.min(src.len());
                            buf[..n].copy_from_slice(&src[..n]);
                            src.drain(..n);
                            Ok(n)
                        }
                        None => std::io::stdin().read(&mut buf),
                    };
                    return match n {
                        Ok(n) => {
                            self.write_bytes(buf_ptr, &buf[..n]);
                            Ok(Value::Int(n as i64))
                        }
                        Err(_) => Ok(Value::Int(-1)),
                    };
                }
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
                // fds 1 and 2 are the process stdout/stderr, as under WASI, so AIPL
                // code can print formatted bytes without a string value.
                if fd == 1 || fd == 2 {
                    let written = if fd == 1 { std::io::stdout().write(&data) } else { std::io::stderr().write(&data) };
                    return Ok(Value::Int(written.map(|n| n as i64).unwrap_or(-1)));
                }
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
            OpCode::ArgsSizes | OpCode::ArgsGet | OpCode::EnvSizes | OpCode::EnvGet => {
                let mut addr = [0usize; 2];
                for (i, a) in args.iter().enumerate().take(2) {
                    addr[i] = match self.eval_expr(a, scope)? {
                        Value::Int(v) => v as u32 as usize,
                        other => return Err(format!("{:?} requires Int addresses, got {:?}", op, other)),
                    };
                }
                // WASI hosts reject (trap on) misaligned out-parameters: the
                // count and size words, and the pointer table.
                let aligned = if matches!(op, OpCode::ArgsSizes | OpCode::EnvSizes) { [addr[0], addr[1]].to_vec() } else { vec![addr[0]] };
                if let Some(a) = aligned.iter().find(|a| *a % 4 != 0) {
                    return Err(format!("{:?}: address {} is not 4-aligned, which WASI requires", op, a));
                }
                let items = if matches!(op, OpCode::ArgsSizes | OpCode::ArgsGet) { Arc::clone(&self.args) } else { Arc::clone(&self.env) };
                let sizes = matches!(op, OpCode::ArgsSizes | OpCode::EnvSizes);
                Ok(self.wasi_strings(&items, sizes, addr[0], addr[1]))
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
            // Real OS thread spawn: the worker is a function reference (its
            // index into the shared function order), run on a real std::thread
            // with a fresh child VM that shares `self.shared` linear memory.
            // The handle is the address of a 16-byte thread record
            // [done:i32 result:i32 fn:i32 arg:i32], laid out and allocated
            // exactly as compiled code does (AIPL_SPEC.md 4.D), so heap
            // addresses and handles agree between the backends.
            OpCode::ThreadSpawn => {
                let fn_name = self.fn_ref_name(&args[0], scope, "thread.spawn")?;
                let arg = match self.eval_expr(&args[1], scope)? {
                    Value::Int(i) => i as i32,
                    _ => return Err("thread.spawn requires Int arg".to_string()),
                };
                let fn_index = self.fn_order.iter().position(|n| *n == fn_name).unwrap_or(0) as i32;
                let rec = self.alloc_bytes(16) as u32 as usize;
                let mut fields = Vec::with_capacity(16);
                for w in [0, 0, fn_index, arg] {
                    fields.extend_from_slice(&w.to_le_bytes());
                }
                self.write_bytes(rec, &fields);
                let mut child = self.spawn_child();
                let handle = std::thread::spawn(move || {
                    // a compiled thread allocates its runtime scratch block first
                    child.alloc_bytes(24);
                    let r = child.invoke(&fn_name, vec![Value::Int(arg as i64)]);
                    if let Ok(Value::Int(v)) = &r {
                        child.write_bytes(rec + 4, &(*v as i32).to_le_bytes());
                    }
                    child.write_bytes(rec, &1i32.to_le_bytes());
                    r
                });
                self.thread_handles.insert(rec as i32, handle);
                Ok(Value::Int(rec as i64))
            }
            OpCode::ThreadJoin => {
                let tid = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as i32,
                    _ => return Err("thread.join requires Int handle".to_string()),
                };
                let handle = match self.thread_handles.remove(&tid) {
                    Some(h) => h,
                    None => return Err(format!("thread.join: {} is not a handle from thread.spawn in this thread (or was already joined)", tid)),
                };
                match handle.join() {
                    Ok(Ok(Value::Int(i))) => Ok(Value::Int(i)),
                    Ok(Ok(_)) => Ok(Value::Int(0)),
                    Ok(Err(e)) => Err(format!("Spawned thread's function failed: {}", e)),
                    Err(_) => Err("Spawned thread panicked".to_string()),
                }
            }
            OpCode::I64ExtendS => match self.eval_expr(&args[0], scope)? {
                Value::Int(x) => Ok(Value::Int64((x as i32) as i64)),
                _ => Err("i64.extend_s requires Int".to_string()),
            },
            OpCode::I64ExtendU => match self.eval_expr(&args[0], scope)? {
                Value::Int(x) => Ok(Value::Int64((x as i32 as u32) as i64)),
                _ => Err("i64.extend_u requires Int".to_string()),
            },
            OpCode::I32Wrap => match self.eval_expr(&args[0], scope)? {
                Value::Int64(x) => Ok(Value::Int((x as i32) as i64)),
                _ => Err("i32.wrap requires Int64".to_string()),
            },
            // i64 -> f64 rounds to nearest, ties to even, as Rust's `as` and wasm's f64.convert_i64_s do.
            OpCode::F64ConvertI64S => match self.eval_expr(&args[0], scope)? {
                Value::Int64(x) => Ok(Value::Float(x as f64)),
                _ => Err("f64.convert_i64_s requires Int64".to_string()),
            },
            // Rust's sqrt is IEEE 754's correctly rounded square root, as wasm's f64.sqrt.
            OpCode::F64Sqrt => match self.eval_expr(&args[0], scope)? {
                Value::Float(x) => Ok(Value::Float(x.sqrt())),
                _ => Err("f64.sqrt requires Float".to_string()),
            },
            // Truncates toward zero; NaN or a result outside i64 is an error where wasm traps.
            OpCode::I64TruncF64S => match self.eval_expr(&args[0], scope)? {
                Value::Float(x) if x.is_nan() => Err("i64.trunc_f64_s: invalid conversion to integer (NaN)".to_string()),
                Value::Float(x) if x >= -9223372036854775808.0 && x < 9223372036854775808.0 => Ok(Value::Int64(x.trunc() as i64)),
                Value::Float(x) => Err(format!("i64.trunc_f64_s: integer overflow converting {}", x)),
                _ => Err("i64.trunc_f64_s requires Float".to_string()),
            },
            OpCode::F64ReinterpretI64 => match self.eval_expr(&args[0], scope)? {
                Value::Int64(x) => Ok(Value::Float(f64::from_bits(x as u64))),
                _ => Err("f64.reinterpret_i64 requires Int64".to_string()),
            },
            OpCode::I64ReinterpretF64 => match self.eval_expr(&args[0], scope)? {
                Value::Float(x) => Ok(Value::Int64(x.to_bits() as i64)),
                _ => Err("i64.reinterpret_f64 requires Float".to_string()),
            },
            OpCode::MemLoadF32 | OpCode::MemLoadF64 | OpCode::MemStoreF32 | OpCode::MemStoreF64 => {
                Err(format!("{:?} not supported in VM backend: floating point memory ops not implemented", op))
            }
            OpCode::SysTime => {
                let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?;
                Ok(Value::Int64(now.as_nanos() as i64))
            }
            OpCode::SysMonotonic => {
                static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
                Ok(Value::Int64(START.get_or_init(std::time::Instant::now).elapsed().as_nanos() as i64))
            }
            OpCode::SysRandom => {
                let ptr = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as u32 as usize,
                    _ => return Err("sys.random requires Int ptr".to_string()),
                };
                let len = match self.eval_expr(&args[1], scope)? {
                    Value::Int(i) => i as u32 as usize,
                    _ => return Err("sys.random requires Int len".to_string()),
                };
                // Like fs.read, the host writes without the store guard; an
                // address range outside memory is a failure, as in WASI.
                let mut buf = vec![0u8; len];
                let filled = File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).is_ok();
                let mem_len = self.shared.lock().unwrap().bytes.len();
                if !filled || ptr.checked_add(len).map_or(true, |end| end > mem_len) {
                    return Ok(Value::Int(-1));
                }
                self.write_bytes(ptr, &buf);
                Ok(Value::Int(0))
            }
            // The VM deliberately does not terminate the host process (it may be a
            // test runner or the agent server); the request surfaces as an error
            // carrying the code. Compiled wasm calls WASI proc_exit for real.
            OpCode::SysExit => {
                let code = match self.eval_expr(&args[0], scope)? {
                    Value::Int(i) => i as i32,
                    _ => return Err("sys.exit requires Int code".to_string()),
                };
                Err(format!("sys.exit({}) requested", code))
            }
            OpCode::StrLen => match self.eval_expr(&args[0], scope)? {
                Value::Str(s) => Ok(Value::Int(s.len() as i64)),
                other => Err(format!("str.len requires Str, got {:?}", other)),
            },
            // Materialise the string into the heap in the same [len][bytes] shape
            // the wasm data segment uses, and return the address of the bytes.
            // Each evaluation allocates afresh (bump allocator, never freed).
            OpCode::StrPtr => match self.eval_expr(&args[0], scope)? {
                Value::Str(s) => Ok(Value::Int(self.materialize_str("str.ptr", &s)? as i64)),
                other => Err(format!("str.ptr requires Str, got {:?}", other)),
            },
        }
    }
}

/// Runtime enforcement of the memory layout for writes (AIPL_SPEC.md, "Memory
/// layout"): bytes 0-3 are the heap cursor, bytes 64-1023 are reserved, and
/// bytes 1024..heap_start are the module's string literals, so no store or
/// atomic op may target them, however the address was computed. The wasm
/// backend emits the identical check (trapping with `unreachable`), so this is
/// a shared semantic, not a VM-only guard. Reads are not checked.
pub fn check_write_address(op: &str, ptr: usize, heap_start: usize) -> Result<(), String> {
    if ptr < 4 {
        return Err(format!(
            "{} at address {}: bytes 0-3 are the heap cursor owned by mem.alloc; take memory from (mem.alloc n) instead",
            op, ptr
        ));
    }
    if (64..1024).contains(&ptr) {
        return Err(format!(
            "{} at address {}: bytes 64-1023 are the reserved runtime block; take memory from (mem.alloc n) instead",
            op, ptr
        ));
    }
    if (1024..heap_start).contains(&ptr) {
        return Err(format!(
            "{} at address {}: bytes 1024-{} are the program's string literals, which are read-only; take memory from (mem.alloc n) instead",
            op, ptr, heap_start - 1
        ));
    }
    Ok(())
}

/// A contract failure as source text with the call's arguments, e.g.
/// `Pre-condition failed in 'f' at 1:37: (req (gt n 0)) with n = -1`.
fn contract_failure(
    kind: &str,
    form: &str,
    expr: &Expr,
    fn_name: &str,
    params: &[(String, Type)],
    scope: &HashMap<String, Value>,
    result: Option<&Value>,
) -> String {
    let (line, col) = expr.span();
    let mut bound: Vec<String> = params
        .iter()
        .filter_map(|(name, _)| scope.get(name).map(|v| format!("{} = {}", name, value_str(v))))
        .collect();
    if let Some(r) = result {
        bound.push(format!("res = {}", value_str(r)));
    }
    let with = if bound.is_empty() { String::new() } else { format!(" with {}", bound.join(", ")) };
    format!(
        "{} failed in '{}' at {}:{}: ({} {}){}",
        kind, fn_name, line, col, form, crate::printer::expr_str(expr), with
    )
}

/// A value as AIPL source would write it (pointers and arrays as their address).
fn value_str(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::Int64(i) => format!("{i}i64"),
        Value::Float(x) => format!("{x:?}"),
        Value::Bool(b) => b.to_string(),
        Value::Str(s) => format!("{s:?}"),
        Value::Ok(x) => format!("(ok {})", value_str(x)),
        Value::Err(x) => format!("(err {})", value_str(x)),
        Value::Void => "void".to_string(),
        other => format!("{other:?}"),
    }
}

/// Atomic instructions need a naturally aligned address; wasm traps on an
/// unaligned one, so the VM fails the same way. Blocks from mem.alloc are
/// always 8-aligned.
fn check_atomic_alignment(op: &str, ptr: usize) -> Result<(), String> {
    if ptr % 4 != 0 {
        return Err(format!("{} at address {}: atomic operations need a 4-aligned address", op, ptr));
    }
    Ok(())
}
