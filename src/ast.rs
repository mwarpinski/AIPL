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
    /// A struct: named in `(ptr S)` for a struct behind a pointer, or on its
    /// own for a value struct (`(struct S [...] (value))`, AIPL_SPEC.md 4.E).
    Struct(String),
    ResultType(Box<Type>, Box<Type>),
    /// `(arr T)`: an `arr.new` array of `T`, whose length sits in the 4 bytes before it.
    Array(Box<Type>),
    /// `(fn [params] -> ret)`, and with `(no_leak)` (true) a reference to
    /// a function the checker proves keeps no memory (MEMORY_PLAN.md decision 9).
    Fn(Vec<Type>, Box<Type>, bool),
    /// A value of the enum named (`(enum Name [members])`, AIPL_SPEC.md 4.I):
    /// an `i32` at run time, a distinct type to the checker.
    Enum(String),
    /// A value of the union named (`(union Name [(variant field:T ...) ...])`,
    /// AIPL_SPEC.md 4.J): a tagged heap cell, never null; an `i32` address
    /// at run time.
    Union(String),
    /// `u8`, `i8`, `u16`, `i16`: a storage type, only a struct field's or
    /// an array element's (AIPL_SPEC.md 4.A): 1 or 2 bytes in memory, read
    /// as an `i32` (zero- or sign-extended), written from one (its low bits).
    Narrow(Narrow),
}

/// The storage types: their width and whether a read sign-extends.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Narrow {
    U8,
    I8,
    U16,
    I16,
}

impl Narrow {
    pub fn from_name(s: &str) -> Option<Narrow> {
        match s {
            "u8" => Some(Narrow::U8),
            "i8" => Some(Narrow::I8),
            "u16" => Some(Narrow::U16),
            "i16" => Some(Narrow::I16),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Narrow::U8 => "u8",
            Narrow::I8 => "i8",
            Narrow::U16 => "u16",
            Narrow::I16 => "i16",
        }
    }
    /// Bytes in memory (also the alignment).
    pub fn size(self) -> usize {
        match self {
            Narrow::U8 | Narrow::I8 => 1,
            Narrow::U16 | Narrow::I16 => 2,
        }
    }
    pub fn signed(self) -> bool {
        matches!(self, Narrow::I8 | Narrow::I16)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    /// A 32-bit integer literal (`42`, `-7`). Stored as i64 for convenience but
    /// always truncated to i32 by every backend.
    Int(i64),
    /// A 64-bit integer literal written with an explicit suffix: `42i64`.
    Int64(i64),
    Float(f64),
    /// `1.5f32`: the value (the decimal rounded to an f64, then to an f32,
    /// in every toolchain) and the literal as written.
    Float32(f32, String),
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
    MemStore8,
    MemStore32,
    MemStore64,
    /// `(mem.load8_s a)`, `(mem.load16_u a)`, `(mem.load16_s a)`: one or two
    /// bytes, zero- or sign-extended to an i32; `(mem.store16 a v)`: v's low
    /// 16 bits (AIPL_SPEC.md 4.A).
    MemLoad8S,
    MemLoad16U,
    MemLoad16S,
    MemStore16,
    MemAlloc,
    /// `(mem.grow pages)`: grows linear memory by `pages` 64 KiB pages and
    /// returns the previous size in pages, or -1 if the maximum (32768 pages, 2 GiB)
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
    /// `(fs.opendir ptr len)`: a directory's descriptor, or -1.
    FsOpendir,
    /// `(fs.readdir fd buf len cookie)`: WASI directory entries from `cookie` on.
    FsReaddir,
    /// `(fs.stat ptr len out)`: a file's WASI filestat (64 bytes at `out`).
    FsStat,
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
    /// The f32 conversions (AIPL_SPEC.md 4.A): f64 -> f32 (round to
    /// nearest), f32 -> f64 (exact), integer -> float (round to nearest),
    /// float -> integer (toward zero; NaN or out of range traps), and the
    /// bit reinterpretations.
    F32DemoteF64,
    F64PromoteF32,
    F32ConvertI32S,
    F32ConvertI64S,
    F64ConvertI32S,
    I32TruncF32S,
    I32TruncF64S,
    I64TruncF32S,
    F32ReinterpretI32,
    I32ReinterpretF32,
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
    /// The formatting forms after the resolver splits their strings
    /// (AIPL_SPEC.md 4.K): `(format "x = {x}")` is `(format.str "x = "
    /// (format.piece x ""))`, each argument a string literal or a piece, a
    /// value and its format as written after the colon. `format.to` takes the
    /// buffer first. The resolver lowers them to `std/show` calls once the
    /// program checks (formats.rs), so no backend sees them.
    FormatStr,
    FormatTo,
    FormatPrint,
    FormatPrintln,
    FormatEprintln,
    FormatPiece,
    /// Early returns (AIPL_SPEC.md 4.F): `(try r)` is a result's ok value,
    /// or returns its error from the function; `(try_opt o)` an Option's
    /// value, or returns `none`; `(try_or o e)` an Option's value, or returns
    /// `(err e)`. Lowered by the resolver to match_result / match
    /// (formats::lower), like the formatting ops.
    Try,
    TryOpt,
    TryOr,
    /// `(defer e ...)`, a statement of a function's body: `e ...` run when the
    /// function returns, however it returns, the last defer first. Lowered
    /// by the resolver (formats::lower), like `try`.
    Defer,
    /// `(format.dbg "label" e)`, from `(dbg e)`: prints the label and e's
    /// value to stderr and gives the value (formats.rs).
    FormatDbg,
    /// `(format.expect "label" c)`, from a test's `(expect c)`: a failure
    /// counted and reported when c is false (formats.rs lower_expect).
    FormatExpect,
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
    /// `(unsafe e...)`: a block in which the unchecked operations (raw memory,
    /// casts, atomics, threads, raw-address I/O) and calls to `(unsafe)`
    /// functions are allowed (AIPL_SPEC.md 3).
    Unsafe(Vec<Expr>, (u32, u32)),
    GetField {
        struct_name: String,
        field_name: String,
        ptr: Box<Expr>,
        span: (u32, u32),
    },
    /// `(put p S.f V.g ... v)`: stores `v` in field `f` of the struct `p`
    /// points at, or, with a path, in field `g` of the value struct held
    /// inline in `f` (and so on): `path` is the fields after the first.
    PutField {
        struct_name: String,
        field_name: String,
        ptr: Box<Expr>,
        val: Box<Expr>,
        #[serde(default)]
        path: Vec<(String, String)>,
        span: (u32, u32),
    },
    /// `(sizeof S)` for a struct, or `(sizeof T)` for any type that can be
    /// stored in memory (`Type::Struct` for a struct name).
    Sizeof {
        ty: Type,
        span: (u32, u32),
    },
    ArrGet {
        elem_ty: Type,
        ptr: Box<Expr>,
        index: Box<Expr>,
        span: (u32, u32),
    },
    /// `(arr.set T a i v)`, or with a path `(arr.set V a i V.f ... v)`: one
    /// field of the value struct element (`path` is the fields, in order).
    ArrSet {
        elem_ty: Type,
        ptr: Box<Expr>,
        index: Box<Expr>,
        val: Box<Expr>,
        #[serde(default)]
        path: Vec<(String, String)>,
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
    /// `(ptr.cast S addr)` / `(arr.cast T addr)`: an `i32` address as `ty`,
    /// unchecked for generations. With `gen`, `(ptr.make S addr gen)` /
    /// `(arr.make T addr gen)`: a pointer carrying that generation, which
    /// every access compares with the word 8 bytes before the object
    /// (docs/design/GENERATIONS_PLAN.md).
    Cast {
        ty: Type,
        addr: Box<Expr>,
        gen: Option<Box<Expr>>,
        span: (u32, u32),
    },
    /// `(make V args...)`: a value of value struct `V`, one argument per
    /// field in order.
    Record {
        struct_name: String,
        args: Vec<Expr>,
        span: (u32, u32),
    },
    /// `(with v V.f x)`: a copy of value struct `v` with field `f` replaced.
    With {
        struct_name: String,
        field_name: String,
        value: Box<Expr>,
        new_val: Box<Expr>,
        span: (u32, u32),
    },
    /// `(make Name.variant args...)`: a new union value of that variant.
    Make {
        union_name: String,
        variant: String,
        args: Vec<Expr>,
        span: (u32, u32),
    },
    /// `(match v arms... [(else body...)])` over a union or an enum value.
    Match {
        value: Box<Expr>,
        arms: Vec<MatchArm>,
        else_body: Option<Vec<Expr>>,
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
            Expr::Block(_, span) | Expr::Unsafe(_, span) => *span,
            Expr::GetField { span, .. } => *span,
            Expr::PutField { span, .. } => *span,
            Expr::Sizeof { span, .. } => *span,
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
            Expr::Make { span, .. } | Expr::Match { span, .. } => *span,
            Expr::Record { span, .. } | Expr::With { span, .. } => *span,
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
    /// `(ptr.gen p)`: the generation a pointer carries
    PtrGen,
    /// `(arr.gen a)`
    ArrGen,
}

/// `(union Name [(variant field:T ...) ...])`: a value is one of the
/// variants, carrying that variant's fields. Variants are numbered in order
/// (the tag stored at offset 0 of the cell).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnionDef {
    pub name: String,
    pub variants: Vec<Variant>,
    pub span: (u32, u32),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<StructField>,
}

/// One arm of a `match`: `(Name.member [binders] body...)` for a union
/// (binders: the variant's fields, in order), `(Name.member body...)` for an
/// enum (binders: None).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchArm {
    pub member: String,
    pub binders: Option<Vec<String>>,
    pub body: Vec<Expr>,
    pub span: (u32, u32),
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
    /// `(value)`: a value struct, held inline and never behind a pointer.
    #[serde(default)]
    pub is_value: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub contracts: Vec<Contract>,
    pub body: Vec<Expr>,
    pub span: (u32, u32),
    /// The function has the `(unsafe)` clause: calling it takes an unsafe
    /// context, and its body is one.
    #[serde(default)]
    pub is_unsafe: bool,
    /// The `(no_leak)` clause: the function promises to make no memory that
    /// is never freed (docs/design/MEMORY_PLAN.md M3), which the checker
    /// checks outside `unsafe` (leaks::check).
    #[serde(default)]
    pub is_no_leak: bool,
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
    pub unions: Vec<UnionDef>,
    pub functions: Vec<FnDef>,
    /// Where each item was written, keyed `"struct NAME"`, `"enum NAME"`,
    /// `"union NAME"`, or `"fn NAME"`; filled by the resolver so type
    /// errors can name the file, and empty for a module parsed from one text.
    #[serde(skip)]
    pub sources: std::collections::HashMap<String, Source>,
    /// Each source file's name in compiled code's line table (the resolver's
    /// FlatProgram::files); empty for a module parsed from one text.
    #[serde(skip)]
    pub file_names: std::collections::HashMap<std::path::PathBuf, String>,
}

/// The file an item was written in, and the files of type arguments
/// substituted into a generic instance (by token position).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Source {
    pub file: std::path::PathBuf,
    pub foreign: std::collections::HashMap<(u32, u32), std::path::PathBuf>,
}

impl Source {
    /// The file the token at `pos` was written in.
    pub fn file_at(&self, pos: (u32, u32)) -> &std::path::Path {
        self.foreign.get(&pos).unwrap_or(&self.file)
    }

    /// Prefixes an error that starts with `L:C:` with the file that position is in.
    pub fn name_file(&self, e: String) -> String {
        let pos = e.split(':').take(2).map(|n| n.trim().parse::<u32>()).collect::<Result<Vec<_>, _>>();
        let file = match pos.as_deref() {
            Ok([l, c]) => self.foreign.get(&(*l, *c)).unwrap_or(&self.file),
            _ => &self.file,
        };
        format!("{}: {}", file.display(), e)
    }
}

/// A type as AIPL source writes it (`(ptr Point)`, `(arr i64)`), for
/// diagnostics; `{:?}` stays Rust's own form.
impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // a placeholder for a type parameter shows as the parameter
        f.write_str(&crate::checker::shown(&crate::printer::type_str(self)))
    }
}

/// An op as AIPL source writes it (`+`, `mem.load32`), for diagnostics.
impl std::fmt::Display for OpCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(crate::printer::op_name(self))
    }
}
