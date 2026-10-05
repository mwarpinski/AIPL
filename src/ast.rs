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
    /// `(ptr S)`: pointer to a struct; the inner type is always `Struct`.
    Ptr(Box<Type>),
    /// A struct named in `(ptr S)`. Never a value type on its own.
    Struct(String),
    ResultType(Box<Type>, Box<Type>),
    /// `(arr T)`: an `arr.new` array of `T`, whose length sits in the 4 bytes before it.
    Array(Box<Type>),
    Fn(Vec<Type>, Box<Type>),
    /// A value of the enum named (`(enum Name [members])`, AIPL_SPEC.md 4.I):
    /// an `i32` at run time, a distinct type to the checker.
    Enum(String),
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
    /// returns the previous size in pages, or -1 if the maximum (1024 pages)
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
    /// `(checked.add a b)`, `(checked.sub a b)`, `(checked.mul a b)`: i32/i64
    /// arithmetic that traps on signed overflow instead of wrapping.
    CheckedAdd,
    CheckedSub,
    CheckedMul,
    /// `(ltu a b)`, `(lteu a b)`, `(gtu a b)`, `(gteu a b)`: i32/i64 compared as
    /// unsigned (wasm `i32.lt_u` ...).
    LtU,
    LteU,
    GtU,
    GteU,
    Gt,
    Gte,
    And,
    Or,
    Not,
    SysPrint,
    /// `(sys.time)`: i64 nanoseconds since the Unix epoch (WASI clock_time_get, realtime).
    SysTime,
    /// `(sys.monotonic)`: i64 nanoseconds from an arbitrary fixed start, for
    /// measuring durations (WASI clock_time_get, monotonic).
    SysMonotonic,
    /// `(sys.random ptr len)`: fills len bytes at ptr with OS randomness
    /// (WASI random_get); 0, or -1 on failure.
    SysRandom,
    SysExit,
    FsOpen,
    FsRead,
    FsWrite,
    FsClose,
    FsDelete,
    /// `(args.sizes count_ptr size_ptr)`: WASI `args_sizes_get`. Writes the
    /// argument count and the total bytes of the NUL-terminated arguments;
    /// returns 0, or -1 on failure. std/os wraps the four args/env ops.
    ArgsSizes,
    /// `(args.get argv_ptr buf_ptr)`: WASI `args_get`. Writes one u32 pointer
    /// per argument at argv_ptr and the NUL-terminated arguments at buf_ptr.
    ArgsGet,
    /// `(env.sizes count_ptr size_ptr)`: WASI `environ_sizes_get`, as args.sizes
    /// for the `KEY=VALUE` environment entries.
    EnvSizes,
    /// `(env.get env_ptr buf_ptr)`: WASI `environ_get`, as args.get.
    EnvGet,
    ThreadSpawn,
    ThreadJoin,
    /// `(i64.extend_s x)`: i32 -> i64, sign-extending (wasm `i64.extend_i32_s`).
    I64ExtendS,
    /// `(i64.extend_u x)`: i32 -> i64, zero-extending (wasm `i64.extend_i32_u`).
    I64ExtendU,
    /// `(i32.wrap x)`: i64 -> i32, keeping the low 32 bits (wasm `i32.wrap_i64`).
    I32Wrap,
    F64ConvertI64S,
    /// `(f64.sqrt x)`: the square root, correctly rounded (wasm `f64.sqrt`,
    /// IEEE 754); NaN for a negative x, and sqrt(-0.0) is -0.0.
    F64Sqrt,
    I64TruncF64S,
    F64ReinterpretI64,
    I64ReinterpretF64,
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
    Ok(Box<Expr>, Option<Type>, (u32, u32)),
    Err(Box<Expr>, Option<Type>, (u32, u32)),
    MatchResult {
        expr: Box<Expr>,
        ok_var: String,
        ok_body: Vec<Expr>,
        err_var: String,
        err_body: Vec<Expr>,
        span: (u32, u32),
    },
    Block(Vec<Expr>, (u32, u32)),
    NewStruct {
        struct_name: String,
        span: (u32, u32),
    },
    GetField {
        struct_name: String,
        field_name: String,
        ptr: Box<Expr>,
        span: (u32, u32),
    },
    PutField {
        struct_name: String,
        field_name: String,
        ptr: Box<Expr>,
        val: Box<Expr>,
        span: (u32, u32),
    },
    Sizeof {
        struct_name: String,
        span: (u32, u32),
    },
    ArrNew {
        elem_ty: Type,
        size: Box<Expr>,
        span: (u32, u32),
    },
    ArrGet {
        elem_ty: Type,
        ptr: Box<Expr>,
        index: Box<Expr>,
        span: (u32, u32),
    },
    ArrSet {
        elem_ty: Type,
        ptr: Box<Expr>,
        index: Box<Expr>,
        val: Box<Expr>,
        span: (u32, u32),
    },
    /// `(arr.len a)`: the element count stored before the array.
    ArrLen {
        arr: Box<Expr>,
        span: (u32, u32),
    },
    /// `(ptr.null S)` / `(arr.null T)`: `ty` is the resulting `(ptr S)` / `(arr T)`.
    Null {
        ty: Type,
        span: (u32, u32),
    },
    /// `(ptr.cast S addr)` / `(arr.cast T addr)`: an `i32` address as `ty`.
    Cast {
        ty: Type,
        addr: Box<Expr>,
        span: (u32, u32),
    },
    /// `(return v)` / `(return)`: leaves the function. Type void.
    Return {
        val: Option<Box<Expr>>,
        span: (u32, u32),
    },
    /// `(break)`: leaves the innermost while/loop. Type void.
    Break((u32, u32)),
    /// `(continue)`: next iteration of the innermost while/loop (a `loop`
    /// still applies its step). Type void.
    Continue((u32, u32)),
    /// `(ref f)`: a reference to function `f`, of type `(fn [params] -> ret)`.
    Ref {
        name: String,
        span: (u32, u32),
    },
    /// `(call_ref (fn [params] -> ret) f args...)`: an indirect call; `sig`
    /// must equal `f`'s type.
    CallRef {
        sig: Type,
        func: Box<Expr>,
        args: Vec<Expr>,
        span: (u32, u32),
    },
    /// `(ptr.addr p)` / `(arr.addr a)`: the `i32` address of a pointer or
    /// array; `(enum.ord e)`: the `i32` value of an enum. `kind` records which
    /// spelling was used, so the checker can require the matching operand.
    Addr {
        val: Box<Expr>,
        kind: AddrKind,
        span: (u32, u32),
    },
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
            Expr::Ok(_, _, span) => *span,
            Expr::Err(_, _, span) => *span,
            Expr::MatchResult { span, .. } => *span,
            Expr::Block(_, span) => *span,
            Expr::NewStruct { span, .. } => *span,
            Expr::GetField { span, .. } => *span,
            Expr::PutField { span, .. } => *span,
            Expr::Sizeof { span, .. } => *span,
            Expr::ArrNew { span, .. } => *span,
            Expr::ArrGet { span, .. } => *span,
            Expr::ArrSet { span, .. } => *span,
            Expr::ArrLen { span, .. } => *span,
            Expr::Null { span, .. } => *span,
            Expr::Cast { span, .. } => *span,
            Expr::Addr { span, .. } => *span,
            Expr::Ref { span, .. } => *span,
            Expr::Return { span, .. } => *span,
            Expr::Break(span) | Expr::Continue(span) => *span,
            Expr::CallRef { span, .. } => *span,
        }
    }
}

/// Which `i32` view an `Expr::Addr` takes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AddrKind {
    /// `(ptr.addr p)`
    Ptr,
    /// `(arr.addr a)`
    Arr,
    /// `(enum.ord e)`
    Enum,
}

/// `(enum Name [a b (c 10) ...])`: named `i32` values. A member without a
/// value is one more than the member before it (the first is 0).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumDef {
    pub name: String,
    pub members: Vec<(String, i32)>,
    pub span: (u32, u32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructField {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<StructField>,
    pub span: (u32, u32),
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
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
    pub functions: Vec<FnDef>,
}
