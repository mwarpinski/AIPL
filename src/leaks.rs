//! `(no_leak)` (docs/design/MEMORY_PLAN.md M3): which functions may make
//! memory that is never freed, and the error for each function marked
//! `(no_leak)` that does. Mirrored by aipl_src/leaks.aipl, message for
//! message.
//!
//! A function may leak if its code uses `mem.alloc` or `format` (a str),
//! calls `alloc.forever` (the allocator whose memory is never freed; `new`
//! and `arr.new` are calls of alloc.create and alloc.array by now, with
//! the allocator they name), calls through a function reference, or calls
//! a function that may leak. (Results and unions are values, MEMORY_PLAN.md
//! M4, so nothing else allocates without saying so.) Inside
//! `unsafe` this counts for the function's callers but not against its own
//! `(no_leak)` promise: there it is the programmer's word, which is how an
//! allocator growing its pool with `mem.alloc` is marked `(no_leak)`. A
//! function marked `(no_leak)` does not leak for its callers.

use crate::ast::{Expr, FnDef, Module, OpCode};
use crate::compiler::wasm::walk_expr;
use std::collections::{HashMap, HashSet};

/// Why a function may leak: its first allocation that is never freed, or
/// its first call to a function that may.
#[derive(Clone, Debug)]
enum Why {
    Uses(String),
    Calls(String),
}

/// A call through a function reference whose signature does not promise
/// (no_leak): its target is not known, so it may keep memory.
const THROUGH: &str = "a call through a function type without (no_leak)";

/// What a site in a function does: an allocation that is never freed (what
/// it is), or a call.
#[derive(Clone, Debug)]
enum Site {
    Leak(String),
    Call(String),
}

/// A site: where it is, what it does, and whether it is inside unsafe.
type Sited = ((u32, u32), Site, bool);

/// The leaking sites and calls of f, in source order, with their positions
/// and whether each is inside unsafe.
fn sites(f: &FnDef) -> Vec<Sited> {
    let mut inside: HashSet<(u32, u32)> = HashSet::new();
    let mut mark_unsafe = |e: &Expr| {
        if let Expr::Unsafe(body, _) = e {
            for b in body {
                walk_expr(b, &mut |x: &Expr| {
                    inside.insert(span_of(x));
                });
            }
        }
    };
    for e in &f.body {
        walk_expr(e, &mut mark_unsafe);
    }
    let mut out = Vec::new();
    let mut visit = |e: &Expr| {
        let at = span_of(e);
        let site = match e {
            Expr::Op { op: OpCode::MemAlloc, .. } => Some(Site::Leak("mem.alloc".into())),
            Expr::Op { op: OpCode::FormatStr, .. } => Some(Site::Leak("format (a str)".into())),
            // a reference whose type promises (no_leak) keeps no memory
            // (MEMORY_PLAN.md decision 9): the checker let only such a
            // function into it
            Expr::CallRef { sig: crate::ast::Type::Fn(_, _, true), .. } => None,
            Expr::CallRef { .. } => Some(Site::Leak(THROUGH.into())),
            Expr::Call { func, .. } if func == "alloc.forever" => Some(Site::Leak("alloc.forever".into())),
            Expr::Call { func, .. } => Some(Site::Call(func.clone())),
            _ => None,
        };
        if let Some(s) = site {
            out.push((at, s, f.is_unsafe || inside.contains(&at)));
        }
    };
    for e in &f.body {
        walk_expr(e, &mut visit);
    }
    out.sort_by_key(|(at, _, _)| *at);
    out
}

/// The position an expression starts at.
fn span_of(e: &Expr) -> (u32, u32) {
    e.span()
}

/// Which functions may make memory that is never freed, and why: what
/// the checker needs to type `(ref f)` (decision 9) and `check` to give
/// its errors.
#[derive(Clone, Debug)]
pub struct Analysis {
    all: HashMap<String, Vec<Sited>>,
    why: HashMap<String, Why>,
}

impl Analysis {
    pub fn new(module: &Module) -> Analysis {
        let all: HashMap<String, Vec<Sited>> = module.functions.iter().map(|f| (f.name.clone(), sites(f))).collect();
        let marked: HashSet<&str> = module.functions.iter().filter(|f| f.is_no_leak).map(|f| f.name.as_str()).collect();
        // which unmarked functions may leak, and why (to a fixed point; each
        // reason names something found earlier, so the chains end)
        let mut why: HashMap<String, Why> = HashMap::new();
        loop {
            let mut more = false;
            for f in &module.functions {
                let name = f.name.as_str();
                if marked.contains(name) || why.contains_key(name) {
                    continue;
                }
                let reason = all[name].iter().find_map(|(_, s, _)| match s {
                    Site::Leak(what) => Some(Why::Uses(what.clone())),
                    Site::Call(g) if why.contains_key(g.as_str()) => Some(Why::Calls(g.clone())),
                    Site::Call(_) => None,
                });
                if let Some(r) = reason {
                    why.insert(name.to_string(), r);
                    more = true;
                }
            }
            if !more {
                break;
            }
        }
        Analysis { all, why }
    }

    /// Whether f may make memory that is never freed (a function marked
    /// `(no_leak)` does not, for its callers).
    pub fn may_leak(&self, f: &str) -> bool {
        self.why.contains_key(f)
    }

    /// "g calls h; h uses mem.alloc": the chain from start down to the
    /// allocation.
    pub fn chain(&self, start: &str) -> String {
        let mut parts = Vec::new();
        let mut at = start.to_string();
        loop {
            match self.why.get(at.as_str()) {
                Some(Why::Uses(what)) => {
                    parts.push(format!("{at} uses {what}"));
                    break;
                }
                Some(Why::Calls(g)) => {
                    parts.push(format!("{at} calls {g}"));
                    at = g.clone();
                }
                None => break,
            }
        }
        parts.join("; ")
    }
}

/// The errors for the functions marked `(no_leak)` that may make memory
/// that is never freed, one per function, in the module's order: each
/// `L:C: f is marked (no_leak) but ...`, at the first offending site.
pub fn check(module: &Module) -> Vec<(String, String)> {
    let a = Analysis::new(module);
    let mut errors = Vec::new();
    for f in module.functions.iter().filter(|f| f.is_no_leak) {
        let first = a.all[f.name.as_str()].iter().find(|(_, s, in_unsafe)| {
            !*in_unsafe
                && match s {
                    Site::Leak(_) => true,
                    Site::Call(g) => a.may_leak(g),
                }
        });
        if let Some(((l, c), s, _)) = first {
            let what = match s {
                Site::Leak(what) if what == THROUGH => format!("uses {what}, which may keep memory (a signature with (no_leak) takes only functions that keep none)"),
                Site::Leak(what) => format!("uses {what}, whose memory is never freed"),
                Site::Call(g) => format!("calls {g}, which may make memory that is never freed ({})", a.chain(g)),
            };
            errors.push((f.name.clone(), format!("{l}:{c}: {} is marked (no_leak) but {what}", f.name)));
        }
    }
    errors
}
