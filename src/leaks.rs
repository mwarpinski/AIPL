//! `(no_leak)` (docs/design/MEMORY_PLAN.md M3): which functions may make
//! memory that is never freed, and the error for each function marked
//! `(no_leak)` that does. Mirrored by aipl_src/leaks.aipl, message for
//! message.
//!
//! A function may leak if its code uses `new`, `arr.new`, `mem.alloc`, or
//! `format` (a str), calls `alloc.forever` (the allocator whose memory is
//! never freed), calls through a function reference, or calls a
//! function that may leak. (Results and unions are values, MEMORY_PLAN.md
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
#[derive(Clone)]
enum Why {
    Uses(String),
    Calls(String),
}

/// What a site in a function does: an allocation that is never freed (what
/// it is), or a call.
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
            Expr::NewStruct { .. } => Some(Site::Leak("new".into())),
            Expr::ArrNew { .. } => Some(Site::Leak("arr.new".into())),
            Expr::Op { op: OpCode::MemAlloc, .. } => Some(Site::Leak("mem.alloc".into())),
            Expr::Op { op: OpCode::FormatStr, .. } => Some(Site::Leak("format (a str)".into())),
            Expr::CallRef { .. } => Some(Site::Leak("a call through a function reference (its target is not known)".into())),
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

/// The errors for the functions marked `(no_leak)` that may make memory
/// that is never freed, one per function, in the module's order: each
/// `L:C: f is marked (no_leak) but ...`, at the first offending site.
pub fn check(module: &Module) -> Vec<(String, String)> {
    let all: HashMap<&str, Vec<Sited>> = module.functions.iter().map(|f| (f.name.as_str(), sites(f))).collect();
    let marked: HashSet<&str> = module.functions.iter().filter(|f| f.is_no_leak).map(|f| f.name.as_str()).collect();
    // which unmarked functions may leak, and why (to a fixed point; each
    // reason names something found earlier, so the chains end)
    let mut why: HashMap<&str, Why> = HashMap::new();
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
                why.insert(name, r);
                more = true;
            }
        }
        if !more {
            break;
        }
    }
    let chain = |start: &str| -> String {
        let mut parts = Vec::new();
        let mut at = start.to_string();
        loop {
            match why.get(at.as_str()) {
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
    };
    let mut errors = Vec::new();
    for f in module.functions.iter().filter(|f| f.is_no_leak) {
        let first = all[f.name.as_str()].iter().find(|(_, s, in_unsafe)| {
            !*in_unsafe
                && match s {
                    Site::Leak(_) => true,
                    Site::Call(g) => why.contains_key(g.as_str()),
                }
        });
        if let Some(((l, c), s, _)) = first {
            let what = match s {
                Site::Leak(what) => format!("uses {what}, whose memory is never freed"),
                Site::Call(g) => format!("calls {g}, which may make memory that is never freed ({})", chain(g)),
            };
            errors.push((f.name.clone(), format!("{l}:{c}: {} is marked (no_leak) but {what}", f.name)));
        }
    }
    errors
}
