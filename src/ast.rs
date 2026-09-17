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
    Int(i64),
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
    BitXor,
    Shl,
    Shr,
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
    VecDot,
    MatMul,
    ArrGet,
    ArrSet,
    DomElem,
    DomMount,
    DomAppend,
    DomOnEvent,
    WebAlert,
    SysPrint,
    SysTime,
    SysExit,
    FsOpen,
    FsRead,
    FsWrite,
    FsClose,
    ThreadSpawn,
    ThreadJoin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Contract {
    Requires(Expr),
    Ensures(Expr),
    Invariant(Expr),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    Lit(Literal),
    Var(String),
    Let {
        name: String,
        ty: Type,
        val: Box<Expr>,
    },
    Set {
        name: String,
        val: Box<Expr>,
    },
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
    },
    Loop {
        var: String,
        start: Box<Expr>,
        end: Box<Expr>,
        step: Box<Expr>,
        body: Vec<Expr>,
    },
    While {
        cond: Box<Expr>,
        body: Vec<Expr>,
    },
    Call {
        func: String,
        args: Vec<Expr>,
    },
    Op {
        op: OpCode,
        args: Vec<Expr>,
    },
    Ok(Box<Expr>),
    Err(Box<Expr>),
    MatchResult {
        expr: Box<Expr>,
        ok_var: String,
        ok_body: Vec<Expr>,
        err_var: String,
        err_body: Vec<Expr>,
    },
    Block(Vec<Expr>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub contracts: Vec<Contract>,
    pub body: Vec<Expr>,
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
