# Memory for programs that run for days

**Status: approved 2026-10-19, to do it properly even where that is
more work.** Written when the networking plan (NET_PLAN.md) met AIPL's
memory model. Decisions (section 5): checked value views; the
`str`-returning conveniences kept for programs that run and exit, with
`(no_leak)` refusing them; the name `(no_leak)`; networking after M1 and
M3; and every allocation names its allocator, never-freed memory being
one more allocator, `alloc.forever` (decision 5). First step: the seed
refresh, which M1 needs (the seed of 2026-10-16 11:56 predates storage
types such as `(arr u8)`, added at 15:29 that day in ab54038).

## 1. What is wrong today

Two things, one a safety hole and one a leak.

**Views are not checked.** A `str.Bytes` is an address and a
length, with no generation. A view of a buffer that has been freed reads
whatever is there now, silently, in code with no `unsafe`:

```lisp
(let v:str.Bytes (call buf.bytes b))
(call buf.free b)
(let c:(ptr buf.Buf) (call buf.make a 32))
(call buf.push_str c "something else!")
(println "view after free: {v}")          ;; prints "something else!"
```

Checked 2026-10-19 in the VM and natively: no trap. AIPL_SPEC.md says
`buf.free` "also ends any view of it", which is not true, and the
generation checks' promise ("any pointer to a destroyed object fails
with Use after free") does not cover views. This blocks launch on its
own, servers or not.

**Some allocations can never be freed, and nothing shows where.** In
code with no `unsafe`, these take memory for the rest of the run:

| What | Where it happens | How often |
|---|---|---|
| a view | `str.from_str`, `str.slice`, `buf.bytes`, `io.read_file`: each makes a `str.Bytes` with `new` | 1,700 places hold the type (377 in std, 1,308 in the compiler) |
| a run-time `str` | `str.concat`, `buf.to_str`, `str.from_bytes`, `(format ...)`, `date.to_str` | about 50 calls in std |
| `new`, `arr.new`, `mem.alloc` | written in the program, or inside std (`io.print_int` takes 11 bytes a call) | 65 in std, 252 in the compiler |
| a result cell | `ok`/`err` when a side is a `str` or needs more than 8 slots (AIPL_SPEC.md 4.F) | wherever such results are made |
| a union held by address | `make` of a union needing more than 8 slots (4.J) | wherever such unions are made |

For a program that runs and exits (a compiler, a data job, a tool, the
CGI-style handler in NET_PLAN.md) this is the right trade: no frees, no
leaks that matter. For a program that runs for days it is fatal, and the
program cannot tell which calls are safe to make in a loop.

## 2. The rule the fix establishes

In code with no `unsafe`:

1. **Every allocation that can be freed is checked.** A pointer, an
   array, and now a view carry the generation of what they point into,
   so a use after free, through any of them, stops the program.
2. **Every allocation that cannot be freed is visible to the checker.**
   A function marked `(no_leak)` may not make one, directly or through
   anything it calls, and the checker says which call would.

A long-running program then puts its work in a `(no_leak)` function,
gives it an allocator (an arena per request, say), and resets the arena
after: anything kept by mistake across requests traps the next time it
is used, and nothing can pile up.

## 3. The pieces

### M1. Checked views (the safety hole, and the view leak)

`str.Bytes` becomes a value struct holding a checked byte array:

```lisp
(struct Bytes [(read data:(arr u8)) (read start:i32) (read len:i32)] (value) (pub))
```

- **Nothing is allocated** for a view: it is three slots, copied like an
  `i32`. `str.slice` makes a new value from an old one.
- **Every read is checked** through `data`: its bounds, and its
  generation, so a view of a freed buffer traps with "Use after free".
- **A literal is viewed in place.** A `str` is already a 4-byte length
  and then its bytes (AIPL_SPEC.md 4.B), which is an `(arr u8)`'s layout,
  so `str.from_str` views a literal with no copy; literals are never
  freed, so their arrays need no generation.
- **A buffer's storage becomes an `(arr u8)`** from its allocator, so
  growing it (a new array, the old one freed) ends the old views, as
  the spec has always said it does.

The change: `str.Bytes` becomes `str.Bytes` in about 1,700 places
(a script does most of it), `(get b str.Bytes.len)` keeps working,
`(ptr.null str.Bytes)` and `str.repoint` (which changes a view in place)
go, and the 27 places in std that read through a view's raw address
use the array instead. The compiler's source is most of the 1,700, and
it is compiler source, so this comes after the seed refresh, or is
written for the seed's language (value structs are in it). Checks on
every byte read cost something: measured on the benchmarks and on the
compiler compiling itself before it lands; loop versioning already
removes most such checks from loops.

### M2. No hidden immortal allocations in std

The standard library stops allocating behind a caller's back:
`io.print_int` and its kind print through a scratch area, or through
std/stream's writer; every std function that builds text takes an
allocator and returns a view or a buffer, with the `str`-returning
forms kept for programs that run and exit and marked as such (M3).

### M3. `(no_leak)`: a checked promise

```lisp
(fn handle [a:alloc.Allocator req:str.Bytes out:(ptr buf.Buf)] -> void (no_leak)
  ...)
```

The checker refuses a `(no_leak)` function that makes an allocation that
cannot be freed (`new`, `arr.new`, `mem.alloc`, a run-time `str`, a
result cell, a union held by address) or calls a function that may,
naming the call and where the allocation is: "handle may make memory
that is never freed: it calls str.concat (std/str.aipl:289), which
makes a str". The analysis is the call-graph pass loop versioning
already has (`may_free`), over a different question, in both checkers.

The rule, exactly (settled 2026-10-19):

- A function not marked `(no_leak)` **may leak** if its code, `unsafe`
  parts included, uses `new`, `arr.new`, `mem.alloc`, or `format` (a
  `str`), calls `alloc.forever`, calls through a function reference
  (`call_ref`, whose target is not known; allowed once function types can
  carry the promise), or calls a function that may leak. A function
  marked `(no_leak)` does not leak for its callers. The hidden cells (a
  result with a `str` side or more than 8 slots, a union held by address)
  are not counted: only the code generators know which values take one,
  and M4 removes them, so `(no_leak)` does not ship before M4 does.
- A function marked `(no_leak)` must not, and the checker says which use
  or call does, with the chain down to the allocation: "handle may make
  memory that is never freed: it calls str.concat (std/str.aipl:297),
  which calls str.room, which uses mem.alloc". Inside `unsafe` the
  promise is the programmer's word, as every unsafe promise is: that is
  how an allocator that grows its own pool with `mem.alloc` (heap.raw,
  arena.raw, alloc.raw) is marked `(no_leak)` and trusted, and where the
  checking of its callers stops.
- An `Allocator` a function is handed is its caller's choice: `(no_leak)`
  says what the function's own code does, so a server passes its
  handlers a request arena, never `alloc.forever`.
- Both checkers compute it over the whole program, after generics; a
  function not marked is not checked, only described (may leak or not),
  so marking a request path is how a program asks for the guarantee.

### M4. Results and unions without hidden cells

A result with a `str` side or more than 8 slots, and a union that is
recursive, has a `str` variant, or needs more than 8 slots, got a heap
cell the program never sees and never frees.

**(a) Results (done 2026-10-19).** A result is never stored in memory
(a struct field or array element cannot be one), which is why a `str`
side once needed a cell, so it is always held by value: a `str` side is
one i32-sized slot, and a result needing more than 8 slots is refused,
where its type is written and where one is made, by both checkers word
for word ("keep the larger side's data in a struct that is not (value)
and make that side a (ptr S)"). The result-cell code is gone from the
VM, wasm.rs, and codegen.aipl, and with it the checker's payload-type
pass the VM ran on every load. checker.aipl now decides which unions are
values (checker.rs union_layouts), which (b) needs too. No test program
in the tree used a result wider than 8 slots but results_by_value.aipl
and unions_by_value.aipl, which tested the cell.

**(b) Unions held by address (decision 6: every union a value).** Unions that are
recursive (ast.aipl's `Type` and `Expr`, the tests' `List` and `Chain`),
have a `str` variant, or exceed 8 slots are an unchecked i32 address of
a cell from `make`. A `str` variant needs no cell, for the reason (a)
gave: once a `str` is one slot of a value, and the VM writes a `str`
into memory as it does for a struct field. What remains is recursion and
width, and decision 6 settles both: every union is a value, at most 8
slots, as a result is; recursion and width go through a `(ptr S)` the
program allocates from an allocator it names, and anything else is
refused with the way to write it. Steps: a `str` variant is one slot
(done 2026-10-19: the union is a value; all three deciders, checker.rs,
checker.aipl, and codegen.aipl, agree); the refusal lands unused, its
message naming the widest variant and saying to move its fields into a
struct held by a `(ptr S)`; ast.aipl's `Type` and `Expr` and the tests'
recursive unions move behind cells (done 2026-10-19:
tools/migrate_ast_cells.py moved the 22 variants of `Expr` that hold
expressions, `Expr.make` for width, and `Type`'s 4 that hold types
behind read-only structs built by constructors in ast.aipl; `Expr` is 7
slots, `Type` 4; the compiler's output is unchanged and compiling itself
takes the same time, 1.18 s against 1.16 s; no seed refresh was needed,
since the new code uses only structs and pointers); the refusal is on in
both checkers (done 2026-10-19, tests/test_values.rs), with the spec,
the prompt guide's union example, and every test that held a union by
address rewritten; then the held-by-address code leaves all three code
generators. A loss to recover: formatting prints a pointer field as its
address, so a tree no longer prints whole; the generated printers could
follow pointers, with a guard against cycles.

### M5. Long-lived servers

With M1 to M4, the socket half of the original networking plan is safe:
a server that accepts connections itself, keeps state in memory, and
runs each request's `(no_leak)` handler with an arena reset after it.
NET_PLAN.md's fresh instance per request stays as the simple choice
(and the one where a crash costs one request); both use std/http.

## 4. Order, and what each step proves

| Step | What | Tests that show it |
|---|---|---|
| 0 | Correct the spec now: views are not checked, and `buf.free` does not end them (done 2026-10-19) | the docs check |
| S | Refresh the stage-0 seed, so the compiler's source can use storage types, value views, and the forms added since 2026-10-16 (done 2026-10-19) | test_bootstrap: the new seed is the Rust build of its source, and builds itself to the same bytes |
| M1 | checked views (done 2026-10-19: the three ways a view outlived its memory each stop the program in every backend, tests/test_memory.rs; every other test and parity check unchanged; the compiler compiling itself natively 1.73 s to 1.78 s; benchmarks the same but knucleotide, 0.80 s to 0.59 s, since views no longer allocate and bytes_eq reads raw after one check) | the program above traps with "Use after free" in all three backends; every existing test and parity check unchanged; benchmark and self-compile times reported |
| M3 | `(no_leak)` (done 2026-10-19: both checkers, word for word; std's allocators marked after a second seed refresh; tests/test_memory.rs) | one test per kind of immortal allocation, refused with its path; std's request-path functions marked and checked |
| F | `alloc.forever` (decision 5): a variant of `alloc.Allocator` whose memory is never reused (a free retires the generation, so stale pointers still trap); then `new` and `arr.new` leave the language, the compiler's 209 uses and every other moving to `alloc.create` / `alloc.array` on `alloc.forever` | the same bytes from both toolchains; the compiler compiling itself, timed before and after |
| M4 (a) | results always by value (done 2026-10-19: a `str` side is one slot; over 8 slots refused by both checkers; result cells removed from all three code generators; tests/test_values.rs) | a result with a `str` side allocates nothing in any backend; the 9-slot refusal is the same from both toolchains |
| M2, M4 (b) | std and the compiler stop making hidden cells | `(no_leak)` holds for std/http, std/url, std/json, std/text on their request paths |
| M5 | servers | a server serves a million requests with memory flat, in all three backends; a handler that keeps a pointer across requests traps on its next use |

M1 is the largest; the rest are each about the size of one stdlib
batch. The seed refresh (launch checklist section 6) comes first, so the
compiler's source can be migrated in the current language.

## 5. Decisions for the owner

1. **Views as checked values (M1, recommended)**, or views kept as
   pointers with a generation added (a 16-byte cell still allocated per
   view, so the leak stays; not recommended).
2. **Keep the `str`-returning conveniences** (`str.concat`, `format`,
   `buf.to_str`) for programs that run and exit, with `(no_leak)`
   refusing them (recommended); or remove run-time `str` altogether, so
   `str` is only literals and constants (stricter, and every program
   that builds a message changes).
3. **The name** `(no_leak)`, or argon's `(no_bump)` (REVISIONS.md), or
   another. `no_leak` says what it promises; the allocations it refuses
   are not all bump allocations.
4. **Networking** waits for M1 and M3 (recommended: servers built on the
   fixed model), or the fresh-instance server ships first, since it is
   safe today.
5. **Every allocation names its allocator (adopted).** `new`, `arr.new`,
   and a run-time `str` are memory that is never freed, made without
   saying so. Never-freed memory becomes an allocator like the others,
   `alloc.forever`: `(call (alloc.create Config) alloc.forever)` lives
   until exit and says so; `(call (alloc.create Node) arena)` goes with
   the arena. Then `(no_leak)` is one rule ("never uses alloc.forever",
   directly or through a call), there is one way to allocate, and which
   memory lasts is visible in the source, as in Zig. `new` and `arr.new`
   leave the language; the compiler's 209 uses and the tests' move to
   `alloc.create` / `alloc.array` (by script). Run-time `str` values come
   only from `alloc.forever` (they have no generation, so nothing else
   could free them safely); text built per request is a view of a
   buffer from the request's allocator.
6. **Unions held by address (M4 b; decided 2026-10-19: (i)).** Either (i) every union is a
   value, and recursion or width goes through a pointer the program
   allocates: `(union List [(nil) (cons head:i32 tail:(ptr Cell))])`
   with `(struct Cell [l:List])`, so each cell is an ordinary checked
   `(ptr S)` from a named allocator and a stale one traps (recommended:
   no new form, one rule shared with results, and nothing unchecked
   left; but ast.aipl's `Type` and `Expr` move behind such cells, the
   largest migration in the plan); or (ii) `make` of a union held by
   address names its allocator, `(make List.cons a h t)`, and the union
   becomes a checked pointer (address and generation, an i64 slot)
   instead of an unchecked i32 address (less rewriting, a second way to
   allocate, and every backend's union representation changes). Either
   needs a seed refresh before the compiler's source uses it.
7. **The 8-slot cap stays (decided 2026-10-19).** Measured before
   deciding: no std union exceeds it; the tests' three that do exist to
   test the old cell; in the compiler only ast.aipl's `Expr` does, since
   a `str.Bytes` is 3 slots and its variants hold two names. Variants
   share slots by kind, so a union is its tag and its largest variant's
   slots of each kind, not the sum of its variants (two 7-slot unions of
   i32s in one union are 8 slots; one of i32s with one of f64s, 14).
   Hitting the cap almost always means a variant carries a whole record,
   which belongs in a struct the variant points to (as Rust's
   `ExprKind::Call(Box<CallExpr>)`); text cut from one input can be a
   span (start, len: 2 slots), and text from a fixed set an enum (1).
   A cap can be raised later without breaking a program, never lowered:
   raise it only if the showcase programs keep reaching it.
