use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Type {
    I32,
    I64,
    F32,
    F64,
    Bool,
    Str,
    Void,
    Ptr(Box<Type>),
    ResultType(Box<Type>, Box<Type>),
    Array(Box<Type>, usize),
    Vector(Box<Type>, usize),
    Fn(Vec<Type>, Box<Type>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    /// A 32-bit integer literal (`42`, `-7`). Stored as i64 for convenience but
    /// always truncated to i32 by every backend.
    Int(i64),
    /// A 64-bit integer literal written with an explicit suffix: `42i64`.
    Int64(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OpCode {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    DivU,
    RemU,
    BitXor,
    Shl,
    Shr,
    ShrU,
    DivU,
    RemU,
    BitAnd,
    BitOr,
    MemLoad8,
    MemLoad32,
    MemLoad64,
    MemLoadF32,
    MemLoadF64,
    MemStore8,
    MemStore32,
    MemStore64,
    MemStoreF32,
    MemStoreF64,
    MemAlloc,
    MemFree,
    /// `(mem.grow pages)`: grows linear memory by `pages` 64 KiB pages and
    /// returns the previous size in pages, or -1 if the maximum (100 pages)
    /// would be exceeded (wasm `memory.grow`).
    MemGrow,
    AtomicAdd,
    AtomicCas,
    AtomicLock,
    AtomicUnlock,
    Eq,
    Neq,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
    Not,
    ArrGet,
    ArrSet,
    SysPrint,
    SysTime,
    SysExit,
    FsOpen,
    FsRead,
    FsWrite,
    FsClose,
    FsDelete,
    ThreadSpawn,
    ThreadJoin,
    /// `(i64.extend_s x)`: i32 -> i64, sign-extending (wasm `i64.extend_i32_s`).
    I64ExtendS,
    /// `(i64.extend_u x)`: i32 -> i64, zero-extending (wasm `i64.extend_i32_u`).
    I64ExtendU,
    /// `(i32.wrap x)`: i64 -> i32, keeping the low 32 bits (wasm `i32.wrap_i64`).
    I32Wrap,
    /// `(str.len s)`: byte length of a string. In wasm a `str` is a pointer to
    /// interned bytes preceded by a 4-byte little-endian length, so this is
    /// `i32.load (s - 4)`; the VM reads the Rust string's length.
    StrLen,
    /// `(str.ptr s)`: the address of a string's bytes as an `i32`, for passing
    /// to `fs.*` and other pointer-taking ops. Identity in wasm (a `str` already
    /// is that pointer); the VM copies the string into the heap and returns
    /// the copy's address. Pair with `(str.len s)` for the length.
    StrPtr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Contract {
    Requires(Expr),
    Ensures(Expr),
    Invariant(Expr),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Lit(Literal, (u32, u32)),
    Var(String, (u32, u32)),
    Let {
        name: String,
        ty: Type,
        val: Box<Expr>,
        span: (u32, u32),
    },
    Set {
        name: String,
        val: Box<Expr>,
        span: (u32, u32),
    },
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
        span: (u32, u32),
    },
    Loop {
        var: String,
        start: Box<Expr>,
        end: Box<Expr>,
        step: Box<Expr>,
        body: Vec<Expr>,
        span: (u32, u32),
    },
    While {
        cond: Box<Expr>,
        body: Vec<Expr>,
        span: (u32, u32),
    },
    Call {
        func: String,
        args: Vec<Expr>,
        span: (u32, u32),
    },
    Op {
        op: OpCode,
        args: Vec<Expr>,
        span: (u32, u32),
    },
    Ok(Box<Expr>, (u32, u32)),
    Err(Box<Expr>, (u32, u32)),
    MatchResult {
        expr: Box<Expr>,
        ok_var: String,
        ok_body: Vec<Expr>,
        err_var: String,
        err_body: Vec<Expr>,
        span: (u32, u32),
    },
    Block(Vec<Expr>, (u32, u32)),
}

impl Expr {
    pub fn span(&self) -> (u32, u32) {
        match self {
            Expr::Lit(_, span) => *span,
            Expr::Var(_, span) => *span,
            Expr::Let { span, .. } => *span,
            Expr::Set { span, .. } => *span,
            Expr::If { span, .. } => *span,
            Expr::Loop { span, .. } => *span,
            Expr::While { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::Op { span, .. } => *span,
            Expr::Ok(_, span) => *span,
            Expr::Err(_, span) => *span,
            Expr::MatchResult { span, .. } => *span,
            Expr::Block(_, span) => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub contracts: Vec<Contract>,
    pub body: Vec<Expr>,
    pub span: (u32, u32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Import {
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Module {
    pub name: String,
    pub imports: Vec<Import>,
    pub functions: Vec<FnDef>,
}
