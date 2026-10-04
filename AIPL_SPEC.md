# AIPL Specification
## AI Programming Language: Formal Specification

> **Integer semantics: wasm semantics are the spec.** `i32` and `i64` are wrapping two's-complement; the VM must match wasmtime bit-for-bit, and any divergence is a VM bug. Enforced by `tests/test_differential.rs`, which runs every case in both backends.
> **Reference Implementation Decision:** WebAssembly semantics are the definitive specification for AIPL. The VM must match WebAssembly behavior in all edge cases, 32-bit wrapping arithmetic, shift masking, and control flow semantics.

AIPL (AI Programming Language) is an unambiguous, statically typed S-expression systems language with runtime-checked contracts, designed for AI agents to generate. It compiles to WebAssembly (with WASI for I/O), runs in a reference VM, and has a self-hosted compiler written in AIPL itself (section 6.4).

What AIPL optimises for is that a program has one obvious spelling and that mistakes are caught early with a `line:col` diagnostic, not brevity. With the standard library (section 12.6), a small I/O program is about twice the length of the Python equivalent (section 12.7), which is roughly the floor for a fully parenthesised, fully annotated syntax.

---

## 1. Syntax: One Canonical Text Form

AIPL source is `.aipl` text: a context-free, parenthesis-delimited S-expression syntax with no operator precedence, no indentation rules, and no statement separators. It is also the interchange format between agents. `src/printer.rs` prints any resolved program back as canonical flat text, which both compilers accept. (A MessagePack "binary AST" (`.baipl`) existed until P12; it was a dump of the compiler's internal Rust data structures with no stable format, nothing used it, and it was removed.)

---

## 2. Updated Formal Grammar (EBNF)

```ebnf
program        ::= "(" "module" identifier import* ( struct_def | fn_def )* ")" ;
import         ::= "(" "import" identifier [ "as" identifier ] ")" ;
struct_def     ::= "(" "struct" identifier "[" field* "]" ")" ;
field          ::= identifier ":" type ;

fn_def         ::= "(" "fn" identifier "[" param* "]" "->" type contract* expr* ")" ;
param          ::= identifier ":" type ;
contract       ::= "(" ("req" | "ens" | "inv") expr ")" ;

type           ::= "i32" | "i64" | "f32" | "f64" | "bool" | "str" | "void"
                 | "(" "result" type type ")"
                 | "(" "ptr" struct_name ")"
                 | "(" "arr" type ")"
                 | "(" "fn" "[" type* "]" "->" type ")" ;
struct_name    ::= identifier                       (* "m.S" for a struct from imported module m *)

expr           ::= literal
                 | identifier
                 | "(" "let" identifier ":" type expr ")"
                 | "(" "set!" identifier expr ")"
                 | "(" "if" expr expr expr ")"
                 | "(" "loop" identifier expr expr expr expr* ")"
                 | "(" "while" expr expr* ")"
                 | "(" "call" identifier expr* ")"
                 | "(" "block" expr* ")"
                 | "(" "return" [ expr ] ")" | "(" "break" ")" | "(" "continue" ")"
                 | "(" "cond" ( "(" expr expr+ ")" )+ "(" "else" expr+ ")" ")"
                 | "(" ("ok" | "err") [ ":" type ] expr ")"
                 | "(" "match_result" expr "(" "ok" identifier expr* ")" "(" "err" identifier expr* ")" ")"
                 | "(" "new" identifier ")"
                 | "(" "get" expr identifier "." identifier ")"
                 | "(" "put" expr identifier "." identifier expr ")"
                 | "(" "sizeof" identifier ")"
                 | "(" "arr.new" type expr ")"
                 | "(" "arr.get" type expr expr ")"
                 | "(" "arr.set" type expr expr expr ")"
                 | "(" "arr.len" expr ")"
                 | "(" "ptr.null" struct_name ")" | "(" "arr.null" type ")"
                 | "(" "ptr.cast" struct_name expr ")" | "(" "arr.cast" type expr ")"
                 | "(" "ptr.addr" expr ")" | "(" "arr.addr" expr ")"
                 | "(" "ref" identifier ")"
                 | "(" "call_ref" type expr expr* ")"
                 | "(" op expr* ")" ;

op             ::= arithmetic_op | bitwise_op | memory_op | atomic_op | comp_op
                 | conv_op | sys_op | fs_op | thread_op | str_op ;

arithmetic_op  ::= "+" | "-" | "*" | "/" | "%" | "divu" | "remu" ;
bitwise_op     ::= "^" | "shl" | "shr" | "shru" | "bitand" | "bitor" ;
memory_op      ::= "mem.load8" | "mem.load32" | "mem.load64" | "mem.load_f32" | "mem.load_f64"
                 | "mem.store8" | "mem.store32" | "mem.store64" | "mem.store_f32" | "mem.store_f64"
                 | "mem.alloc" | "mem.free" | "mem.grow" ;
atomic_op      ::= "atomic.add" | "atomic.cas" | "atomic.lock" | "atomic.unlock" ;
comp_op        ::= "eq" | "neq" | "lt" | "lte" | "gt" | "gte" | "and" | "or" | "not" ;
conv_op        ::= "i64.extend_s" | "i64.extend_u" | "i32.wrap"
                 | "f64.convert_i64_s" | "i64.trunc_f64_s" | "f64.reinterpret_i64" | "i64.reinterpret_f64" ;
sys_op         ::= "sys.print" | "sys.time" | "sys.monotonic" | "sys.random" | "sys.exit" ;
fs_op          ::= "fs.open" | "fs.read" | "fs.write" | "fs.close" | "fs.delete" ;
proc_op        ::= "args.sizes" | "args.get" | "env.sizes" | "env.get" ;
thread_op      ::= "thread.spawn" | "thread.join" ;
str_op         ::= "str.len" | "str.ptr" ;
```

Notes on the grammar as implemented by `src/parser.rs`:
- A module body is imports followed by struct and function definitions, in any order. `let` and `set!` are expressions that appear inside function bodies, not at module level.
- Comments start with `;;` and run to end of line.
- Integer literals are decimal, optionally negative (`-1` is one token) and are `i32`. An `i64` literal carries the suffix as part of the token: `42i64`, `-7i64`. Float literals must contain a `.` and at least one digit (`1.0`, not `1` or `inf`) and are `f64`. Strings are double-quoted and support the escapes `\n \t \r \0 \\ \"`; any other `\x` is an error. Booleans are `true` / `false`.
- `(call f ...)` takes a bare function name, never an expression. Imported functions are called as `(call modname.fn ...)`.
- `i64` is fully supported (section 8.1). `(fn [t1 t2] -> r)` is the type of a function reference (section 4.G).
- `(ptr i32)` is rejected (`ptr points to a struct; for a sequence of i32 use (arr i32)`), and so is the old `(arr T N)` form (`(arr T) takes no length`).
- In `(get p S.f)` / `(put p S.f v)` the field reference is one symbol, `StructName.fieldName`, split at its last `.`, so `(get p compiler.Node.next)` names field `next` of struct `compiler.Node`.

---

## 3. Type System & Contracts

AIPL is strongly and statically typed. Every parameter, return type, `let`, and struct field carries an explicit type annotation; the checker infers only the types of expressions.

### Scalar Primitives
- `i32`, `i64`: 32-bit and 64-bit signed integers.
- `f32`, `f64`: 32-bit and 64-bit IEEE-754 floating point numbers.
- `bool`: Logical `true` / `false`.
- `str`: Immutable UTF-8 string pointer + length.

### Compound Types
- `(result T_ok T_err)`: the type of `ok`/`err` values, consumed by `match_result` (section 4.F).
- `(ptr S)`: a pointer to a struct `S`. `(arr T)`: a heap array of `T` made by `arr.new`. Both are checked strictly, are never interchangeable with `i32` or with each other, and lower to `i32` in wasm (section 4.E).
- `(fn [t1 ...] -> r)`: a reference to a function with that signature, made by `(ref f)` and called with `call_ref` (section 4.G).
- `(Name t...)`: a generic struct instantiated with types `t...` (section 4.H), used behind `(ptr ...)` like any struct.

### Contracts
`(req e)` (precondition) and `(ens e)` (postcondition) are `bool` expressions placed before the body; inside `ens`, `res` is the return value. The checker type-checks them; nothing is proven statically. The VM evaluates every `req` before the body and every `ens` after it (including after an early `return`) and fails the call with `Pre-condition failed in 'f' at L:C: (req ...) with x = ...` (or `Post-condition`, which also shows `res`). Compiled wasm omits contracts. `(inv e)` is parsed and type-checked but never evaluated (audit B5).
```lisp
(fn db_read_slot [ptr:i32 offset:i32] -> i32
  (req (gt ptr 0))
  (req (gte offset 0))
  (ens (gte res 0))
  (mem.load32 (+ ptr offset)))
```

---

## 4. Systems Operations (Memory & Atomics)

### A. Raw WebAssembly Linear Memory Loads & Stores
- `(mem.load32 ptr)` -> Reads 4 bytes from linear memory offset `ptr` (`i32.load`).
- `(mem.store32 ptr val)` -> Writes 4 bytes to linear memory offset `ptr` (`i32.store`).
- `(mem.alloc size)` -> Bump allocation: returns the current heap cursor (the `i32` at address 0) and advances it by `size` rounded up to a multiple of 8, so every block is 8-aligned (the heap start is too): atomics, `i64`/`f64` values, and WASI out-parameters placed in any allocated block are aligned. The claim is one atomic add, so threads may allocate concurrently. If the new cursor is past the end of memory, memory grows by the pages needed to cover it (up to the 1024-page cap; beyond it nothing grows and the first access past the end fails). `new`, `arr.new`, and `ok`/`err` cells allocate the same way. Never frees. One cursor is shared by the VM, compiled wasm, and AIPL code.
- `(mem.grow pages)` -> Grows linear memory by `pages` × 64 KiB. Returns the previous size in pages, or `-1` if the 1024-page (64 MiB) maximum would be exceeded.
- `(mem.free ptr)` -> Accepted and type-checked, but a no-op today.

See "Memory layout" (section 7.9) for the reserved runtime block below address 1024.

### B. Strings

A `str` is a pointer to immutable UTF-8 bytes preceded by a 4-byte little-endian length. String literals are interned once per module, in first-use order, into the data area that starts at address 1024; the heap starts at the first 8-aligned address after them (section 7.9). Literals are read-only: a store into one traps like a store into the runtime block. A module may have up to 1 MiB of literals (64 KiB in the self-hosted compiler, section 6.4). `(str.len s)` returns the byte length as `i32`; `(str.ptr s)` returns the address of the bytes as `i32`, which is how a string becomes the `(ptr, len)` pair that `fs.*` and other pointer-taking ops expect (identity in both backends: the VM places the literals at the same addresses as wasm when it loads a module, and copies a string onto the heap only if it is not one of the first-loaded module's literals). Two literals with the same text share one address, so `(eq "a" "a")` is `true` and `(eq "a" "b")` is `false` in both backends. `(+ s t)` concatenation exists in the VM only. The VM represents a `str` as a Rust string rather than a pointer; the observable semantics above are the same.

### C. Host I/O (WASI)

Compiled modules import only the host functions they use from `wasi_snapshot_preview1`, so a module without I/O has no import section.

| Op | VM | wasm lowering | Result |
|---|---|---|---|
| `(sys.print s ...)` | prints each argument on its own line | `fd_write` to fd 1 with a two-entry iovec (bytes, then `"\n"`); `str` arguments only | `void` |
| `(fs.open ptr len flags)` | `std::fs` open; `0` = read-only, else create+truncate for writing | `path_open` on the preopened directory (fd 3): `oflags = CREAT|TRUNC` and rights `FD_READ|FD_WRITE` when `flags != 0`, else rights `FD_READ` | new fd, or `-1` |
| `(fs.read fd buf max)` | | `fd_read` with one iovec | bytes read, or `-1` |
| `(fs.write fd buf len)` | | `fd_write` with one iovec | bytes written, or `-1` |
| `(fs.close fd)` | | `fd_close` | `0`, or `-1` |
| `(fs.delete ptr len)` | | `path_unlink_file` on fd 3 | `0`, or `-1` |
| `(sys.exit code)` | returns the error `sys.exit(N) requested` rather than killing the host process | `proc_exit` | never returns |
| `(sys.time)` | the system clock | `clock_time_get` (realtime) | `i64` nanoseconds since 1970-01-01 UTC |
| `(sys.monotonic)` | a monotonic clock | `clock_time_get` (monotonic) | `i64` nanoseconds from an arbitrary fixed start; never decreases. Use for durations |
| `(sys.random ptr len)` | `/dev/urandom` | `random_get` | `0`, or `-1`; fills `len` bytes at `ptr` from the OS's secure generator |
| `(args.sizes count_ptr size_ptr)` | the host's argument list (`aipl eval FILE -- a b` gives `FILE a b`; empty unless set) | `args_sizes_get` | `0`, or `-1`; writes the argument count and the total bytes of the NUL-terminated arguments |
| `(args.get argv_ptr buf_ptr)` | | `args_get` | `0`, or `-1`; writes one u32 address per argument at `argv_ptr` and the NUL-terminated arguments at `buf_ptr` |
| `(env.sizes count_ptr size_ptr)` | the process environment (a host may replace it) | `environ_sizes_get` | as `args.sizes`, for `KEY=VALUE` entries |
| `(env.get env_ptr buf_ptr)` | | `environ_get` | as `args.get` |

Paths are `(ptr, len)` byte ranges in linear memory (`(str.ptr s)` / `(str.len s)` produce them from a string). In the VM they are ordinary paths. Under WASI a relative path resolves in the first preopened directory (fd 3, the working directory) and an absolute path (`/...`) in the second (fd 4), which a host grants as `/` when the program may use absolute paths: `wasmtime run --dir . --dir / prog.wasm`. Without that grant, opening an absolute path fails with `-1`. File descriptor 0 is stdin in both backends (`(fs.read 0 buf n)`; `io.read_stdin` reads all of it). A clock failure traps (it does not happen on supported hosts). File descriptors 1 and 2 are stdout and stderr in both backends, so `(fs.write 1 buf n)` prints raw bytes. The standard library builds printing of numbers and whole-file reads on exactly these primitives (section 12.6). Every WASI errno collapses to `-1`, matching the VM. Argument expressions are evaluated left to right in both backends. The `args.*`/`env.*` out-parameters `count_ptr`, `size_ptr`, and the address tables must be 4-aligned: WASI hosts trap on a misaligned one and the VM fails with `... is not 4-aligned, which WASI requires`. Like `fs.read`, the host writes through these addresses without the store guard. Under WASI the host decides what the program sees (wasmtime forwards an environment variable only with `--env`). Use `std/os` rather than these ops directly. `sys.time` remains VM-only.

### D. Atomics and threads

| Form | Type | Semantics |
|---|---|---|
| `(atomic.add p v)` | `i32` | atomically adds `v` to the word at `p`; returns the previous value. Every atomic address must be 4-aligned (any word in a `mem.alloc` block at a multiple-of-4 offset is); otherwise the VM fails and wasm traps |
| `(atomic.cas p expected new)` | `bool` | if the word at `p` is `expected`, atomically replaces it with `new` and returns `true`; otherwise `false` |
| `(atomic.lock p)` | `void` | acquires the lock word at `p` (0 free, 1 held), waiting while it is held. A word holding anything else is not a lock: the VM fails with an error and compiled code traps |
| `(atomic.unlock p)` | `void` | releases the lock at `p` and wakes waiters; the word must be 1 (held), otherwise an error / trap |
| `(thread.spawn worker arg)` | `i32` | runs `worker(arg)` on a new OS thread; `worker` is a `(fn [i32] -> i32)` reference. Returns a handle |
| `(thread.join h)` | `i32` | waits for the thread to finish and returns `worker`'s result |

All threads share linear memory and the allocator: `mem.alloc`, `new`, `arr.new`, and result cells claim their blocks with one atomic add on the heap cursor, so threads may allocate freely. Atomic addresses pass the store guard (section 7.9) like stores. A handle is the address of a 16-byte thread record `[done:i32 result:i32 fn:i32 arg:i32]` allocated by `thread.spawn`, identically in both backends; treat it as opaque. Each thread has its own file-descriptor table (stdin/stdout/stderr are shared). Output from concurrent threads may interleave: a `sys.print`'s text and its newline are two writes, so lines from two threads can join; use a lock when whole lines matter.

**Compiled threads.** A module that uses `thread.spawn` is a *threaded module* (section 6.2): it imports a shared memory (`"env" "memory"`) and the wasi-threads function `"wasi" "thread-spawn"`, and exports `wasi_thread_start`. Wasmtime removed wasi-threads in version 47, so these modules run under AIPL's own host (the runner from `aipl compile --exe`, and `tests/test_threads.rs`), or any host that still implements wasi-threads; they do not run under the stock `wasmtime` CLI or in the browser demo. Atomics alone need nothing special and work in every module. Wasmtime classes wasm threads as a tier 2 feature and requires shared memory to be enabled in its configuration (`Config::shared_memory`).

### E. Structs, Pointers, and Arrays

```lisp
(struct Point [x:i32 y:i32])
(struct Mixed [flag:bool val:i64 tag:i32])
(struct Node [val:i32 next:(ptr Node)])
```

Struct definitions are module-level. Layout rules, identical in the checker, VM, and wasm backend (`type_size_and_align`, `get_field_offset`, `get_struct_size` in `src/checker.rs`):

- `i32`, `f32`, `bool`, `str`, `(ptr S)`, `(arr T)`: 4 bytes, align 4. `i64`, `f64`: 8 bytes, align 8. A `bool` field is a full 4-byte word.
- Fields are laid out in declaration order; each offset is rounded up to the field's alignment.
- The total size is rounded up to the largest field alignment. `(sizeof Point)` is 8, `(sizeof Mixed)` is 24 (`flag` 0, padding, `val` 8, `tag` 16, padding).

**Pointer and array types are strict.** `(ptr S)` and `(arr T)` are distinct from `i32` and from each other: a `(ptr Node)` cannot be passed where a `(ptr Point)` is expected, an `i32` is never accepted as either, and `arr.get` on an `(arr i32)` must say `i32`. Pointers and arrays have no arithmetic and compare only with `eq`/`neq`. The only conversions are the explicit casts below, which are the unchecked points in a program and easy to find. A struct is never a value on its own, only behind `(ptr S)`. Arrays are their own type rather than a `(ptr T)` because `arr.get`, `arr.set`, and `arr.len` read the length header that only `arr.new` writes, so the type guarantees the header exists. None of this exists at run time: every pointer and array is an `i32` address in both backends, and the compiled bytes are the same as for untyped code.

| Form | Type | Semantics |
|---|---|---|
| `(new S)` | `(ptr S)` | `mem.alloc (sizeof S)`; the memory is not zeroed beyond what the heap already holds |
| `(get p S.f)` | type of `f` | `p` must be a `(ptr S)`; load at `p + offset(f)` with the instruction for `f`'s type |
| `(put p S.f v)` | `void` | `p` must be a `(ptr S)`; store at `p + offset(f)`; `v` must have `f`'s type |
| `(sizeof S)` | `i32` | compile-time constant |
| `(arr.new T n)` | `(arr T)` | evaluates `n` first; `n < 0` is a VM error and a wasm trap; then allocates `4 + n * sizeof(T)` bytes, writes `n` into the first 4, returns the address after them |
| `(arr.get T a i)` | `T` | `a` must be an `(arr T)`; load at `a + i * sizeof(T)` |
| `(arr.set T a i v)` | `void` | `a` must be an `(arr T)`; store at `a + i * sizeof(T)` |
| `(arr.len a)` | `i32` | the element count stored in the 4 bytes before `a` |
| `(ptr.null S)` / `(arr.null T)` | `(ptr S)` / `(arr T)` | address 0 |
| `(ptr.cast S x)` / `(arr.cast T x)` | `(ptr S)` / `(arr T)` | `x` must be `i32`; reinterprets the address (unchecked) |
| `(ptr.addr p)` / `(arr.addr a)` | `i32` | the address, e.g. for `mem.*` or arithmetic |

Field and element types are the scalars, `(ptr S)`, and `(arr T)`, so arrays of pointers (`(arr (ptr Point))`) and of arrays (`(arr (arr i32))`) work; structs do not nest by value. A `bool` read by `get`/`arr.get` is `true` iff its word is nonzero (wasm emits `i32.const 0; i32.ne` after the load), so a word written raw with `mem.store32` behaves the same in both backends. A `str` field or element holds the address of the string's bytes, as a `str` value does in wasm. When storing one, the VM copies the string into the heap the way `str.ptr` does, so heap addresses after a `str` store differ between the backends while lengths, contents, and `eq` agree. `put` and `arr.set` go through the same reserved-block write guard as `mem.store*` in both backends (section 7.9).

**Struct names are namespaced like functions** (section 11): inside the module that defines it, a struct is `Node`; an importer writes `compiler.Node` (or `c.Node` after `(import compiler as c)`), in `new`, `sizeof`, `(ptr ...)`, and field references such as `(get p compiler.Node.next)`. Two imported modules may each define a `Node`.

**Bounds checks are VM-only.** The VM checks `0 <= i < n` against the header at `a - 4` and fails with `Array index out of bounds: index I for array of length N`. The wasm backend does not check (it would need a second scratch local per function), so an out-of-range index reads or writes neighbouring heap memory. This is the second accepted VM-only check alongside contracts (section 10.4).

```lisp
(module points
  (struct Point [x:i32 y:i32])
  (fn main [] -> i32
    (let ps:(arr (ptr Point)) (arr.new (ptr Point) 3))
    (loop i 0 2 1
      (let p:(ptr Point) (new Point))
      (put p Point.x i)
      (put p Point.y (* i 10))
      (arr.set (ptr Point) ps i p))
    (let sum:i32 0)
    (loop i 0 (- (arr.len ps) 1) 1
      (let p:(ptr Point) (arr.get (ptr Point) ps i))
      (set! sum (+ sum (+ (get p Point.x) (get p Point.y)))))
    sum))                                 ;; => Int(33) in both backends
```

### F. Results

`(ok v)` and `(err e)` build a value of type `(result T_ok T_err)`; `match_result` consumes one. Both backends allocate an 8-byte heap cell `[tag:i32 payload:i32]` (tag 0 = ok, 1 = err) from the heap cursor **before** evaluating the payload, so a result and any allocation inside its payload land at the same addresses in both. In compiled wasm a result is the `i32` address of that cell; the VM keeps a tagged value but writes the same cell. Payloads must be 32-bit (`i32`, `bool`, `str`): the wasm backend rejects others with `Wasm Codegen: result payloads must be 32-bit (i32, bool, str), got I64`. The VM accepts them, so keep result payloads 32-bit in code meant to compile.


### G. Function References

```lisp
(fn add [a:i32 b:i32] -> i32 (+ a b))
(fn twice [f:(fn [i32 i32] -> i32) x:i32] -> i32
  (call_ref (fn [i32 i32] -> i32) f x x))
(fn main [] -> i32 (call twice (ref add) 20))      ;; => 40
```

| Form | Type | Semantics |
|---|---|---|
| `(ref f)` | `(fn [params of f] -> return of f)` | a reference to function `f` |
| `(call_ref (fn [t...] -> r) g args...)` | `r` | calls the function `g` refers to; the written signature must equal `g`'s type, and the arguments must match it |

Function types are strict like pointer types: a reference is never an `i32`, a reference with one signature is never accepted for another, there is no arithmetic on references, and they compare only with `eq`/`neq`. There is no cast to a function type, so every reference names a real function. `call_ref` repeats the signature, as `arr.get` repeats the element type, so the call site states what it calls. References can be parameters, results, `let`s, struct fields, and array elements.

At run time a reference is the function's position among the program's functions (after import resolution). The wasm backend emits a funcref table holding every function (min = max = function count) and an active element segment filling it from offset 0, and lowers `call_ref` to `call_indirect` (arguments first, then the reference) with a type index from one extra type per distinct signature, appended after the function types in source order. Table, element section, and extra types are emitted only when the module uses `ref` or `call_ref`, so other programs compile to the same bytes as before. The resolver qualifies `(ref f)` in an imported module like a call, so references work across imports.

### H. Generics

A generic struct or function takes type parameters in its header, and every use names its type arguments explicitly; there is no type inference.

```lisp
(struct (Box T) [value:T next:(ptr (Box T))])

(fn (make T) [v:T] -> (ptr (Box T))
  (let b:(ptr (Box T)) (new (Box T)))
  (put b (Box T) value v)
  b)

(fn main [] -> i32
  (let b:(ptr (Box i64)) (call (make i64) 5i64))
  (i32.wrap (get b (Box i64) value)))
```

| Form | Meaning |
|---|---|
| `(struct (Name T...) [fields])` | a generic struct; parameters are names starting with an uppercase letter |
| `(fn (name T...) [params] -> ret body...)` | a generic function |
| `(Name t...)` | the struct instantiated with types `t...`, wherever a struct name goes: `(ptr (Name i32))`, `(new (Name i32))`, `(sizeof (Name i32))`, `(ptr.null (Name i32))`, `(ptr.cast (Name i32) x)` |
| `(get p (Name t...) field)`, `(put p (Name t...) field v)` | field access on a generic struct (a non-generic struct keeps `(get p S.field)`) |
| `(call (name t...) args...)`, `(ref (name t...))` | calling, or referencing, the function instantiated with `t...` |

Generics are expanded before type checking (`src/generics.rs`, `aipl_src/generics.aipl`): each distinct instantiation becomes an ordinary struct or function named `Name<t,...>` (for example `Box<i64>`, `vec.Vec<ptr<Point>>`, `fn<i32,i32->i32>` for a function type argument), made by substituting the arguments for the parameters. The checker, the VM, both code generators, and any future backend only ever see concrete code, and instance names appear as such in diagnostics and wasm exports. Rules:

- A template is checked only through its instances: an error inside a generic body is reported (at the template's own line) when some use instantiates it. A template nobody uses is not checked.
- Type arguments may be any type, including other instances and function types. Arity is checked: `generic 'Box' takes 1 type argument(s) (T), got 2`.
- A generic may not be named like a built-in form or type (`get`, `put`, `new`, `i32`, ...), since `(get p (Name T) f)` would be ambiguous.
- A generic whose instances keep growing (`(fn (f T) ... (call (f (ptr T)) ...))`) is an error once an instance name passes 1024 characters.
- Imports qualify generics like everything else: `vec.Vec` from another module, `(vec.Vec i32)`, `(call (vec.push i32) v 5)`; aliases work (`(import vec as v)`, `(v.Vec i32)`).

---

## 5. WebAssembly Binary Execution Semantics

AIPL code maps 1-to-1 to WebAssembly binary opcodes:
- `(mem.load32 ptr)` -> `i32.load (MemArg { offset: 0, align: 2 })`
- `(mem.store32 ptr val)` -> `i32.store (MemArg { offset: 0, align: 2 })`
- `(shl a b)` -> `i32.shl`
- `(shr a b)` -> `i32.shr_s`
- `(bitand a b)` -> `i32.and`
- `(bitor a b)` -> `i32.or`

---

## 6. Toolchain Pipeline & Compiler Expectations

Every AIPL entry point runs the same four stages in order. A failure at any stage aborts with a single diagnostic string and a non-zero exit code; nothing downstream runs.

```
source.aipl
   │
   ▼
[1] Resolver   (src/resolver.rs)   reads the file, parses it, follows every (import ...),
   │                               renames imported fns to `mod.fn`, returns ONE flat Module
   ▼
[2] Parser     (src/parser.rs)     S-expression text -> AST (Module { name, imports, functions })
   │                               every node carries a (line, col) span
   ▼
[3] Checker    (src/checker.rs)    static types + contract typing; no inference across fns
   │
   ├──────────────────────────────┐
   ▼                              ▼
[4a] VM        (src/vm.rs)       [4b] Wasm backend (src/compiler/wasm.rs)
     tree-walking interpreter         emits a core wasm module via wasm-encoder
     runs contracts at call time      contracts are NOT emitted (checked statically only)
```

### 6.1 CLI surface (`src/main.rs`)

| Command | Stages run | Success output |
|---|---|---|
| `aipl verify FILE` | 1, 2, 3 | `[AIPL Verifier] OK: module 'NAME' type-checks. Contracts are type-checked, not proven; ...` (nothing is proved: `req`/`ens` run in the VM, section 3) |
| `aipl eval FILE [--func NAME] [-- ARGS...]` | 1, 2, 3, 4a | `[AIPL Result]: Int(42)` (Rust `Debug` of the returned `Value`; default `--func main`). The program's argv is `FILE ARGS...` (`std/os`) |
| `aipl compile FILE [-o out.wasm]` | 1, 2, 3, 4b | `[AIPL Compiler] Successfully compiled 'FILE' -> 'out.wasm' (N bytes)` |
| `aipl compile --exe [--sandbox] FILE -o prog` | 1, 2, 3, 4b | a standalone executable `prog` (section 6.5) |
| `aipl run [--sandbox] prog.wasm [-- ARGS...]` | the `aipl-run` launcher | runs a compiled module natively (section 6.5); exits with its status |
| `aipl compile --self FILE [-o out.wasm]` | 1, 2, 3, 4b, then `resolver.resolve_file` and `codegen.compile_module` in the VM | compiles with the Rust toolchain and with the self-hosted one (AIPL resolver and AIPL codegen) and fails unless the bytes are identical (section 6.4) |
| `aipl test FILE [--func run_all]` | 1, 2, 3, 4a | `[AIPL Test] All groups passed.` and exit 0; otherwise `N group(s) failed.` and exit 1 |
| `aipl serve [--addr 127.0.0.1:8080]` | on request | agent RPC server |

`eval` and `test` only invoke zero-argument functions. To exercise a function that takes parameters, wrap it in a zero-arg driver or write a Rust test (section 10.2).

### 6.2 What a compiled `.wasm` module looks like

`WasmCompiler::compile` produces a WebAssembly module with these sections, in this order: **type, import (only if the module does I/O), function, memory, export, code, data**, plus a table and element section when it uses function references. Allocation uses the threads proposal's `i32.atomic.rmw.add`, which wasmtime and every major browser accept on ordinary memory.

A **threaded module** (one that uses `thread.spawn`) differs: it imports its memory as shared (`"env" "memory"`, min 16, max 1024 pages) instead of defining it, adds a global (the address of this thread's runtime scratch cells, 64 in the main thread), a start function, a data-count section, and two compiler-generated functions after the user's: the start function, which copies the heap cursor and the string literals into memory once (guarded by an atomic flag at address 88, since every thread instantiates the module again), and the exported `wasi_thread_start(tid, record)`, which allocates the thread's 24-byte runtime scratch block, calls the worker through the function table, stores its result, and wakes `thread.join`. Its data segments are passive.

| Item | Value |
|---|---|
| Imports | from `wasi_snapshot_preview1`, only those used, in this order: `fd_write`, `fd_read`, `path_open`, `fd_close`, `proc_exit`, `path_unlink_file`. Their types come first in the type section, and every user function index is offset by the import count. A module with no `sys.print`/`sys.exit`/`fs.*` has no import section and instantiates with no imports. |
| Memory | one linear memory, min 16 pages (1 MiB, same as the VM), max 1024 pages (64 MiB), exported as `"memory"` |
| Data segments | one writing the heap start at address 0 (1024, or the first 8-aligned address after the string literals); if the module has string literals, a second at address 1024 holding every distinct literal as `[len u32 LE][bytes]` |
| String literal | `i32.const <address of its bytes>`; `str` values are pointers (section 4.B) |
| I/O scratch | functions that do I/O, `arr.new`, `atomic.cas/lock/unlock`, or `thread.spawn` get two extra `i32` locals; the WASI lowerings use runtime cells 64-87 for iovecs and out-parameters (section 7.9), or in a threaded module the same offsets in the current thread's scratch block |
| Heap cursor | the `i32` at address 0; `mem.alloc` compiles to `i32.const 0; <size>; i32.atomic.rmw.add` (the size is evaluated first, then one atomic add claims the block), followed (as for `new`, `arr.new`, and result cells) by a check that grows memory with `memory.grow` when the cursor passes `memory.size` (no locals used) |
| Loops | `block { loop { ... } }`; a `loop` (counted) also wraps its body in a block so `continue` falls into the step (section 7.10) |
| Store guard | every `mem.store*`, `put`, and `arr.set` is preceded by a 12-instruction check that traps (`unreachable`) if the address is in bytes 0-3 or between 64 and the heap start (the runtime block and the string literals); each function gets one extra `i32` scratch local for it, which `ok`/`err`, `match_result`, and `arr.new` also use |
| Function refs | only when the module uses `ref`/`call_ref`: one extra type per distinct `call_ref` signature after the function types, a funcref table (section id 4) of every function, and an element section (id 9) filling it; `call_ref` is `call_indirect` (section 4.G) |
| Results and arrays | `ok`/`err` allocate an 8-byte `[tag][payload]` cell; `match_result` tests the tag with `i32.eqz`; `arr.new` writes the count header and returns the address after it (sections 4.E, 4.F) |
| Exports | **every** function in the flat module, exported under its AIPL name (`add`, `compiler.tokenize`, ...) |
| Function types | params map `i32/bool/str/void -> i32`, `f32 -> f32`, `f64 -> f64`; a `void` return is an empty result list |
| Locals | every `let` anywhere in the body (including nested in `if`/`while`/`loop`/`block`) plus every `loop` induction variable becomes one wasm local, allocated after the params |

Bytes 0..8 are always `00 61 73 6D 01 00 00 00` (`\0asm`, version 1). A module that compiles must also pass `wasmparser::Validator::validate_all`; the test suite enforces this.

### 6.3 Backend support matrix (as of 2026-10-01)

"Yes" means the op runs. "Err" means the backend returns an explicit error naming the op; apart from `mem.free` (documented as a no-op) there are no silent defaults or no-ops in any backend. The self-hosted column is `aipl_src/codegen.aipl` (section 6.4).

| Ops | Checker | VM | Rust wasm backend | Self-hosted |
|---|---|---|---|---|
| `+ - * / % divu remu ^ shl shr shru bitand bitor` on `i32` | Yes | Yes, wrapping | Yes | Yes |
| the same on `i64` | Yes | Yes, wrapping at 64 bits | Yes (`i64.*`) | Yes |
| `+ - * /`, comparisons on `f32` / `f64` | Yes | Yes | Yes (`f32.*` / `f64.*`); `%`, `divu`, `remu`, shifts, bitwise are Err | Yes (same rejections, compile error 99) |
| `eq neq lt lte gt gte` on `i32` / `bool` / `str`; `and or not` | Yes | Yes | Yes | Yes |
| `i64` literals | Yes | Yes | Yes | Yes |
| `f64` literals | Yes | Yes | Yes | Yes when the digits form an integer ≤ 2^53 with ≤ 22 after the point (section 6.4); otherwise compile error 973 |
| `i64.extend_s i64.extend_u i32.wrap` | Yes | Yes | Yes | Yes |
| `f64.convert_i64_s i64.trunc_f64_s f64.reinterpret_i64 i64.reinterpret_f64` | Yes | Yes (`trunc` errors on NaN / out of range) | Yes (`trunc` traps) | Yes |
| `mem.load8/32/64`, `mem.store8/32/64` | Yes | Yes | Yes | Yes |
| `mem.load_f32/f64`, `mem.store_f32/f64` | Yes | Err | Err | compile error 987 |
| `mem.alloc`, `mem.grow` | Yes | Yes | Yes | Yes |
| `mem.free` | Yes | no-op (argument not evaluated) | no-op (argument not evaluated) | compile error 987 |
| `atomic.add/cas/lock/unlock` | Yes | Yes, real across OS threads | Yes (wasm atomics; `lock` waits with `memory.atomic.wait32`) | Yes |
| `struct`, `new`, `get`, `put`, `sizeof` | Yes | Yes | Yes | Yes |
| `arr.new`, `arr.get`, `arr.set` | Yes | Yes, bounds-checked | Yes, **not** bounds-checked | Yes |
| `ok`, `err`, `match_result` | Yes | Yes | Yes (8-byte heap cell; 32-bit payloads only) | Yes |
| `sys.print` | Yes | Yes (`println!`, any value) | Yes via WASI `fd_write`; `str` arguments only | Yes |
| `sys.exit` | Yes | returns the error `sys.exit(N) requested` | Yes via WASI `proc_exit` | Yes |
| `sys.time`, `sys.monotonic`, `sys.random` | Yes | Yes | Yes via WASI `clock_time_get` / `random_get` | Yes |
| `fs.open/read/write/close/delete` | Yes | Yes, real `std::fs` | Yes via WASI | Yes |
| `args.sizes/get`, `env.sizes/get` | Yes | Yes (host-set args, process environment) | Yes via WASI `args_*` / `environ_*` | Yes |
| `ref`, `call_ref`, `(fn [..] -> r)` types | Yes | Yes | Yes (funcref table, `call_indirect`) | Yes |
| `return`, `break`, `continue`, `cond` | Yes | Yes | Yes (`return`, `br`; `cond` is nested `if`) | Yes |
| `thread.spawn / thread.join` | Yes | Yes, real `std::thread`; the worker is a `(fn [i32] -> i32)` reference | Yes, as a threaded module (section 4.D): needs a host that provides `wasi.thread-spawn` | Yes |
| `str` literals, `str.len`, `str.ptr` | Yes | Yes | Yes (interned data segment, pointer identity) | Yes |
| `(+ str str)` | Yes | Yes | Err (no string concatenation in wasm) | compile error 99 |
| `(import ...)` | resolved before checking | | | **no**: `compile_module` takes one import-free module |

Rule of thumb for code generators: string concatenation is **VM-only** today. Integer/boolean/float code, memory, structs, arrays, results, string literals, printing, and file I/O run in both; compiled I/O needs a WASI host with a preopened directory (section 10.5). Array bounds checks and contracts exist only in the VM.

### 6.4 The self-hosted backend (`aipl_src/codegen.aipl`)

`codegen.compile_module [src_ptr:i32 src_len:i32] -> i32` tokenizes and parses AIPL source (via `compiler.tokenize` / `compiler.parse_ast`) and emits a complete wasm module. It stores the output pointer in cell 60 and returns the byte length, or `-1` with a nonzero compile error code in cell 4 (the first error encountered; later ones are usually consequences). For everything it accepts, the output is required to be **byte-identical** to `WasmCompiler::compile`. `tests/test_selfhost.rs` enforces this on 30 programs including `memory.aipl`, `compiler.aipl`, and codegen.aipl itself (`compiler.aipl` merged in by hand, since `compile_module` does not resolve imports). `aipl compile --self` checks the same thing for any file and, on a mismatch, reports the first differing byte, the section (and code-section function) it falls in, and a hex window of each side.

It infers each expression's static type the way `expr_type` in `src/compiler/wasm.rs` does (`node_type` / `group_type`) and selects `i32.*` / `i64.*` / `f32.*` / `f64.*` instructions, `if` and `match_result` block types, struct field and array element load/store widths, and alignment from it. It runs in the VM today; compiled to wasm it also compiles itself (section 10.6). Limits that differ from the Rust backend:

- **One module, no imports.** `compile_module` itself takes a single import-free source. Imports are flattened first by `aipl_src/resolver.aipl` (section 11), which `aipl compile --self` runs in the VM; `aipl_src/driver.aipl` chains the two (`driver.compile_file`), and its `main`/`_start` make it a command: compiled to wasm, `wasmtime run --dir . --env AIPL_PATH aiplc.wasm IN.aipl OUT.wasm` compiles a multi-file program with no Rust involved (standard library at `$AIPL_STD`, else `aipl_src/std/`); exit status 0, 1 on a resolve/compile/write error, 2 on bad usage. In the VM: `aipl eval aipl_src/driver.aipl -- IN.aipl OUT.wasm`. The byte-parity tests in `tests/test_selfhost.rs` still flatten with the Rust resolver and `src/printer.rs` (`tests/test_printer.rs` checks that round trip), which keeps them independent of the AIPL resolver. A qualified call such as `(call util.f)` resolves only if a function with that exact name is defined in the source given.
- **Float literals must be exact by construction.** `compile_module` computes an `f64` literal as `m / 10^k`, where `m` is the integer formed by all its digits and `k` is the number of digits after the point, using `f64.convert_i64_s` and one division. That equals Rust's correctly rounded `parse::<f64>` whenever `m ≤ 2^53` and `k ≤ 22`. Anything else (for example `9007199254740993.0`) is compile error 973 rather than a possibly different rounding. Exponent notation (`1.5e3`) and a leading `+` are not float literals in the self-hosted tokenizer (the Rust tokenizer accepts them), so they fail as unknown symbols (971).
- Ops listed as compile error 987 in section 6.3 are not in its keyword table.

Buffers are sized from the input: tokens `12 * (src_len + 1)` bytes, AST `16 * (tokens + 2)`, and output, section scratch, and function scratch `4 * src_len + 64 KiB` each. Memory is grown with `mem.grow` as needed, so a compile works within the 1024-page limit shared by both backends. Compiling the whole self-hosted toolchain (driver, resolver, codegen, compiler, and the standard library it uses: about 216 KB) fits.

| Compile error (cell 4) | Meaning |
|---|---|
| 90 | `mem.grow` refused: the compile needs more than 1024 pages |
| 91 | output or a function body exceeded `4 * src_len + 64 KiB` |
| 92 | more than 2048 functions, or a function with more than 16 parameters |
| 93 | more than 1024 locals in one function |
| 94 | more than 255 structs, a struct with more than 15 fields, or more than 31 distinct `call_ref` signatures |
| 95 | struct field or array element type is not a scalar (`i32 i64 f32 f64 bool str`) |
| 96 | unknown struct or field in `new`/`get`/`put`/`sizeof` |
| 97 | an `ok`/`err` payload that is not 32-bit |
| 98 | a type the backend cannot lower (anything but the scalars and `(result T E)`) |
| 99 | an operator applied to a type with no wasm instruction for it (e.g. `%` on `f64`, `+` on `str`) |
| 768 | string literals exceed the self-hosted buffers (64 KiB of literal data or 1364 distinct literals), or the 1 MiB limit both backends share |
| 971 | a symbol that is not a local or parameter |
| 973 | a float literal outside the exact range above |
| 987 | a form whose head is not a recognised keyword |
| 999 | an empty expression where one is required |
| 1452 | call to an undefined function; cells 44/48 hold the callee name's source offset and length |

### 6.5 Standalone executables and the `aipl-run` launcher

A compiled module runs natively under `aipl-run` (`src/bin/aipl_run.rs`), the toolchain's only native component: wasm cannot start itself, so a small Rust program embeds wasmtime, loads the module (compiled by Cranelift at start-up, about 10 ms for a small program), and connects it to the operating system. It contains no compiler logic. (A native backend written in AIPL, which will write Linux x86-64 executables needing no launcher, is in progress: `docs/NATIVE_BACKEND_PLAN.md`. It is not yet reachable from the command line.)

- **Entry point.** A module with a zero-argument `main` and no `_start` of its own gets an exported `_start` that calls `main` and discards its result, so the module is a WASI command. The exit status is 0 when `_start` returns, or the value given to `sys.exit`, which must be 0-125: as under wasmtime, any other value (including a negative one) is an error reported with status 134, since shells reserve 126 and up. A trap prints the error and exits with 134. (`main`'s return value is not the exit status: many programs return data.)
- **What the program sees.** The real stdin, stdout, and stderr; the command line (argv[0] is the program); the environment; the working directory as preopened fd 3 and `/` as fd 4, so relative and absolute paths both work (section 4.C); and, for a threaded module, the wasi-threads `thread-spawn` import (section 4.D). `--sandbox` grants only the working directory.
- **`aipl compile --exe FILE -o prog`** writes the launcher followed by the module and a 16-byte trailer: flags (`u32` LE, bit 0 = sandbox), the module's length (`u32` LE), and the magic `AIPLEXE1`. The launcher finds a module appended to its own file and runs it; without one it runs a `.wasm` named on its command line (`aipl-run [--sandbox] prog.wasm ARGS...`). The result is one file with no other dependency, about 18 MB in a release build (the size is wasmtime), built for the host's OS and CPU. The `.wasm` remains the portable form.
- The launcher is found as `aipl-run` next to the `aipl` binary, or at `$AIPL_RUNNER`. Writing the bundle is done by the Rust CLI only because WASI cannot set a file's executable bit; the format is plain bytes.

---

## 7. Typing and Evaluation Rules by Example

The checker is `src/checker.rs`; the VM is `src/vm.rs`. Both agree on these rules.

### 7.1 Typing and Scoping Rules (P7 Specification)

The checker (`src/checker.rs`) enforces these six rules; the VM (`src/vm.rs`), the Rust wasm backend (`src/compiler/wasm.rs`), and the self-hosted compiler (`aipl_src/codegen.aipl`, which assumes checked input and emits the same bytes as the Rust backend) implement the same semantics:

1. `set!` has type void.
2. `if` whose two branches are both void is void; otherwise both branches must have the same non-void type — an if mixing void and non-void is a type error with a message suggesting `(block ... value)`.
3. `let` has type void (it declares, it does not yield); a function body's last expression must therefore be a value expression when the return type is non-void.
4. `let` is block-scoped: a `let` inside if/while/loop/block/match arms is visible only within that construct; shadowing an outer name is a type error.
5. `set!` on an undeclared name is a type error and a VM runtime error (delete the globals fallback at vm.rs Expr::Set).
6. `match_result` arms bind ok_var/err_var to the actual Ok/Err payload types from the matched expression's ResultType; ok/err take an explicit result type via (ok:T v) or infer from an enclosing let/return type.

| Form | Type | Example |
|---|---|---|
| integer literal | `i32` | `42`, `-7` |
| suffixed integer literal | `i64` | `42i64`, `-7i64`, `4294967296i64` |
| float literal | `f64` (never `f32`) | `3.5` |
| `true` / `false` | `bool` | |
| `"text"` | `str` | |
| `(let x:T v)` | `void` (declares `x` as `T`; `v` must be `T`) | `(let n:i32 (+ 1 2))` |
| `(set! x v)` | `void` (`v` must match `x`; `x` must be declared) | `(set! n 5)` |
| `(if c a b)` | `void` if both `a`,`b` are `void`; else `T` where `typeof(a)==typeof(b)==T` | |
| `(loop i s e st body*)` | `void`; `s e st` must be `i32`; `i` is block-scoped to the body | |
| `(while c body*)` | `void` | |
| `(block e1 ... en)` | type of `en` (or `void` if empty); introduces a block scope | |
| `(call f args)` | declared return type of `f`; arity and arg types must match | |
| `(ok v)` / `(ok:T_err v)` | `ResultType<typeof v, T_err>` (default `T_err` = `i32`) | `(ok 42)` |
| `(err e)` / `(err:T_ok e)` | `ResultType<T_ok, typeof e>` (default `T_ok` = `i32`) | `(err -1)` |
| `(match_result r (ok v body*) (err e body*))` | type of the last expr of the bodies (which must agree); `v` bound as `T_ok`, `e` bound as `T_err` | |
| `(new S)` / `(arr.new T n)` | `(ptr S)` / `(arr T)` | |
| `(sizeof S)`, `(arr.len a)`, `(ptr.addr p)`, `(arr.addr a)` | `i32` | |
| `(get p S.f)` / `(arr.get T a i)` | the field's type / `T` | `p` must be `(ptr S)`, `a` must be `(arr T)`, `i` must be `i32` |
| `(put p S.f v)` / `(arr.set T a i v)` | `void` | as above; `v` must match the field / `T` |
| binary arithmetic / bitwise | type of the operands, which must be equal | `(+ 1 2)` is `i32` |
| comparisons | `bool`; operands must have equal type | |

A function body is a sequence of expressions. The **last** expression's type must equal the declared return type unless the return type is `void`, in which case the last value is discarded.

### 7.2 Valid: value-producing `if` as the final expression

```lisp
(module ex_if
  (fn sign [n:i32] -> i32
    (if (lt n 0) -1
        (if (eq n 0) 0 1))))
```
`aipl eval` on a driver calling `(call sign -5)` prints `[AIPL Result]: Int(-1)`. Compiles to `if (result i32) ... else ... end` in wasm.

### 7.3 Invalid: mixed branch types

```lisp
(fn bad [n:i32] -> i32
  (if (lt n 0)
      (set! n 0)      ;; type void
      1)              ;; type i32
  n)
```
Checker output: `3:5: If branch type mismatch: then is Void, else is I32. If mixing void and non-void, consider wrapping in (block ... value)`.

### 7.4 Void statements and void `if`

Since `set!` and `let` have type `void`, an `if` whose branches perform side effects is typed as `void`:

```lisp
(fn clamp_to_100 [n:i32] -> i32
  (let out:i32 n)
  (if (gt n 100)
      (set! out 100)
      (set! out n))
  out)
```
Void `if` statements compile cleanly to `if` (empty block type) in wasm.

### 7.5 Loops

`loop` is **inclusive** of its end bound and always steps by the third argument:

```lisp
(fn sum_to_ten [] -> i32
  (let acc:i32 0)
  (loop i 1 10 1
    (set! acc (+ acc i)))
  acc)          ;; => 55, because the bound is inclusive: 1+2+...+10
```
`(loop i 0 9 1 ...)` runs 10 times. `(loop i 0 0 1 ...)` runs once. `start`, `end`, and `step` are each evaluated **once**, in that order, before the first pass (like Python's `range`), so `(loop i 0 (- (call count r) 1) 1 ...)` calls `count` once. Changing a variable used in the bound inside the body does not change the bound. The body may still `set!` the loop variable itself. (Until 2026-10-02 the end and step were re-evaluated on every pass; a call there ran every time.) Leave a loop early with `break`, or skip to the next iteration with `continue` (section 7.10).

```lisp
(fn count_down [start:i32] -> i32
  (req (gte start 0))
  (let n:i32 start)
  (let steps:i32 0)
  (while (gt n 0)
    (set! n (- n 1))
    (set! steps (+ steps 1)))
  steps)
```

### 7.6 Contracts: what runs, where, and what happens on failure

- `req` expressions are type-checked with the parameters in scope and **evaluated by the VM before the body**. Every `req` must be `bool`.
- `ens` expressions are type-checked with parameters plus `res` (bound to the return type) and **evaluated by the VM after the body** with `res` bound to the actual result.
- `inv` is type-checked like `req` but is not evaluated at runtime by any backend today.
- The wasm backend does **not** emit contracts. A compiled module has no runtime checks; the contracts were only verified to be well-typed, not proven.

```lisp
(module contracts_demo
  (fn safe_div [num:i32 den:i32] -> i32
    (req (neq den 0))
    (ens (or (eq num 0) (neq res 0)))   ;; weak, but bool
    (/ num den))

  (fn main [] -> i32
    (call safe_div 10 0)))
```
`aipl eval contracts_demo.aipl` fails with `Pre-condition failed in 'safe_div' at 3:10: (req (neq den 0)) with num = 10, den = 0`: the contract as source, its position, and the arguments (plus `res` for a failed `ens`). A `req` or `ens` that is not `bool` is rejected at check time: `L:C: Contract expression in 'safe_div' must evaluate to Bool, got I32`.

### 7.7 Results

`ok`/`err` build a result value; `match_result` consumes one and binds each arm's variable to the payload type from the result type (rule 6). Representation and the 32-bit payload rule are in section 4.F.

```lisp
(fn parse_digit [c:i32] -> (result i32 i32)
  (if (and (gte c 48) (lte c 57)) (ok (- c 48)) (err -1)))

(fn digit_or_zero [c:i32] -> i32
  (match_result (call parse_digit c)
    (ok v v)
    (err e 0)))
```
`(ok x)` defaults its error type to `i32` and `(err e)` defaults its ok type to `i32`, so both branches of the `if` are `(result i32 i32)`. Write `(ok:T v)` / `(err:T e)` to name the other side explicitly, e.g. `(ok:bool 1)` is `(result i32 bool)`. Both arms of `match_result` must end in the same type. This compiles and runs identically in both backends.

### 7.8 Memory

Linear memory is byte-addressed. Both backends start with 16 pages (1 MiB) and grow up to 1024 pages (64 MiB): automatically when an allocation needs it (section 4.A), or explicitly with `mem.grow`. Loads and stores are little-endian, unaligned access is allowed, and out-of-bounds access is a VM runtime error (`Memory store out of bounds: ptr N`) and a wasm trap. Get memory from `mem.alloc`; never pick an address yourself (section 7.9).

```lisp
(fn pack_two [] -> i32
  (let p:i32 (mem.alloc 8))        ;; p == 1024 on a fresh VM or module
  (mem.store32 p 7)
  (mem.store32 (+ p 4) 35)
  (+ (mem.load32 p) (mem.load32 (+ p 4))))   ;; 42
```

### 7.9 Memory layout

There is one layout and one allocator, shared by the VM, compiled wasm, and AIPL code such as `memory.aipl` and `codegen.aipl`. Bytes `0..1024` are the **runtime block**; the heap begins at `1024` and grows upward through `mem.alloc`.

| Address | Width | Owner | Meaning |
|---|---|---|---|
| 0 | i32 | allocator | heap cursor: the next address `mem.alloc` will return. Initialised to `1024` by `VM::new` and by the wasm data segment. |
| 4 | i32 | codegen | compile-error flag (`0` = none). `set_compile_error` writes `1`; `has_compile_error` reads it. |
| 8 | i32 | codegen | pointer to the struct table (255 entries x 256 bytes, after a 16-byte header) |
| 12 | i32 | codegen | pointer to the string-interning state block (blob length, count, newline address, blob pointer, entries) |
| 16 | i32 | codegen | pointer to the keyword table (256 bytes, `mem.alloc`'d once by `codegen_init`) |
| 20 | i32 | codegen | pointer to the function signature table (2048 entries × 88 bytes) |
| 24 | i32 | codegen | pointer to the default locals table (1024 entries × 12 bytes) |
| 28 | i32 | codegen | running locals count for the function being compiled |
| 32 | i32 | codegen | WASI import count of the module being compiled |
| 36..56 | | unused | (codegen's WASI import indices moved to an allocated table) |
| 60 | i32 | codegen | pointer to the module `compile_module` emitted |
| 64, 68 | i32, i32 | WASI runtime | iovec 0: buffer pointer, length (used by `sys.print`, `fs.read`, `fs.write`) |
| 72, 76 | i32, i32 | WASI runtime | iovec 1: the interned `"\n"` and length 1 (`sys.print`) |
| 80 | i32 | WASI runtime | `nwritten` / `nread` out-parameter |
| 84 | i32 | WASI runtime | `path_open`'s opened-fd out-parameter |
| 88 | i32 | threaded modules | 1 once the shared memory has been initialised (section 6.2) |
| 88..1024 | | reserved | not handed out by `mem.alloc`; do not use |
| 1024..heap start | | string literals | interned string literals, `[len u32 LE][bytes]` each, in first-use order; a `str` value points at the bytes. Empty for a module without literals. Read-only |
| heap start.. | | heap | `mem.alloc` region. The heap start is the first 8-aligned address after the literals (1024 without literals); it is the initial value of the cursor at address 0 |

The runtime's own writes into 64-87 are emitted directly and bypass the store guard described below; user code still cannot store anywhere from 64 up to the heap start.

Rules that follow from this:

- **Never write to a literal address.** Take memory from `mem.alloc` and pass pointers around. Every module in `aipl_src/` and every test does this; a `grep` for four-digit literals in `codegen.aipl` finds only allocation sizes, the page size, an error code, and the 1024 in a comment.
- **The checker enforces the block for literal addresses.** Any `mem.*` or `atomic.*` op whose address is a literal is rejected at check time if it stores to or locks bytes 0-3 (`... bytes 0-3 are the heap cursor owned by mem.alloc ...`), touches bytes 64-1023 (`... bytes 64-1023 are the reserved runtime block ...`), or names a misaligned cell in 4-63. Reading the cursor with `(mem.load32 0)` and using the aligned cells 4-60 is allowed; that is what `memory.aipl` and `codegen.aipl` do. Literal heap addresses (1024 and up) are allowed but discouraged.
- **Both backends enforce the block for writes at runtime.** Every `mem.store*`, `put`, `arr.set`, and `atomic.*` op checks its address before writing: bytes 0-3, 64-1023, and the string literals (1024 up to the heap start) are refused however the address was computed. A store into a literal fails in the VM with `... bytes 1024-N are the program's string literals, which are read-only ...`. The VM fails with `mem.store32 at address 512: bytes 64-1023 are the reserved runtime block; take memory from (mem.alloc n) instead`; compiled wasm traps with `unreachable` on exactly the same addresses (the backend emits a 12-instruction check before each store, using one extra `i32` local per function). This is a shared semantic, not a VM-only guard, so the differential test treats it as agreement. The self-hosted compiler (`codegen.aipl`, `emit_store_guard`) emits the identical bytes, and `tests/test_selfhost.rs` checks that equality directly. Reads are not checked: the block is zero and reading it is harmless.
- **The VM additionally enforces lock validity.** A lock word is only ever `0` (free) or `1` (held). `atomic.lock` on a word holding anything else fails immediately with `atomic.lock: word at ptr N holds V, which is not a lock state ...` instead of spinning forever, and `atomic.unlock` on a word that is not `1` fails with `... a held lock holds 1 ...`. This is what turns "I locked the heap cursor by accident" from a silent hang into an error, whichever way the address was produced.
- **Fresh instances agree.** A fresh VM and a fresh wasm instance both return the heap start from the first `mem.alloc` (1024 for a module without string literals), then that plus `size` rounded up to a multiple of 8, and so on: the VM lays out the first loaded module's literals exactly as the wasm backend does (`wasm::string_layout`). This is why memory-heavy programs can be compared across backends (section 10.4).
- **Threads share the block.** OS threads spawned by `thread.spawn` share the same linear memory and allocator; allocation is atomic, so workers may allocate. In a threaded module, spawned threads use their own copy of cells 64-87 (section 6.2).
- **Codegen state is per instance.** `codegen_init` is idempotent: it allocates its tables only when cell 16 is zero and always clears cells 4 and 28.


### 7.10 Control flow: `return`, `break`, `continue`, `cond`

| Form | Meaning |
|---|---|
| `(return v)` / `(return)` | leaves the function with `v` (which must have the function's return type), or with nothing from a `void` function; `ens` contracts still run on the returned value |
| `(break)` | leaves the innermost `while` or `loop` |
| `(continue)` | starts the next iteration of the innermost loop: a `while` re-tests its condition; a `loop` still applies its step first |
| `(and a b)` / `(or a b)` | short-circuit: `b` runs only if `a` is `true` (for `and`) or `false` (for `or`). Exactly two `bool` operands; nest for more: `(and a (and b c))`. So `(and (lt i n) (eq (arr.get i32 a i) x))` is a safe guard |
| `(cond (c1 e...) (c2 e...) ... (else e...))` | the first clause whose test is true runs its body; `else` is required, like `if`'s else branch. The parser rewrites it to `(if c1 (block e...) (if c2 (block e...) ... (block e...)))`, so the `if` typing rules apply |

`return`, `break`, and `continue` are statements of type `void`. They go where a statement goes: in a body, a `block`, a `cond` clause, or a void `if`. An early exit is therefore written `(if (lt i 0) (return -1) (block))`, not `(if (lt i 0) (return -1) i)`, which mixes `void` and `i32`. A function body may end in `(return v)` instead of a bare `v`. `break` and `continue` outside a loop body (including a `while` condition) and `return` inside a contract are checker errors.

```lisp
(fn index_of [a:(arr i32) target:i32] -> i32
  (loop i 0 (- (arr.len a) 1) 1
    (if (eq (arr.get i32 a i) target) (return i) (block)))
  -1)

(fn classify [n:i32] -> str
  (cond
    ((lt n 0) "negative")
    ((eq n 0) "zero")
    (else "positive")))
```

Lowering (wasm and the self-hosted compiler): `return` is the `return` instruction. Loops are `block { loop { ... } }`; `break` branches to the outer block. A `while` body's `continue` branches to the loop header. A `loop` wraps its body in one more block, and `continue` branches to that block's end, which falls into the step. `(and a b)` is `a; if (result i32) b else i32.const 0 end` and `(or a b)` is `a; if (result i32) i32.const 1 else b end`. Branch depths count the enclosing `if`s (including those of `and`/`or`) and `match_result` arms.

A jump inside an operand (`(+ x (block (if c (break) (block)) 1))`, legal because a `block` may end in a value) leaves the whole expression at once in both backends: operands after it are not evaluated.

---

## 8. Integer Semantics

`i32` is a wrapping two's-complement 32-bit integer in **both** backends; the wasm instruction set defines the behaviour and the VM must match it bit for bit. Internally the VM stores integers in an `i64` but truncates every operand to `i32` before each operation, so no value wider than 32 bits is observable.

| Expression | Result | Why |
|---|---|---|
| `(+ 2147483647 1)` | `-2147483648` | wraps |
| `(* 65536 65536)` | `0` | wraps |
| `(- 0 -2147483648)` | `-2147483648` | wraps |
| `(/ -7 2)` | `-3` | truncates toward zero (`i32.div_s`) |
| `(% -7 2)` | `-1` | sign follows the dividend (`i32.rem_s`) |
| `(/ 7 0)` | runtime error | VM: `Division by zero`; wasm: trap |
| `(/ -2147483648 -1)` | runtime error | VM: `Integer overflow`; wasm: trap |
| `(% -2147483648 -1)` | `0` | defined, not a trap (`i32.rem_s`) |
| `(divu -1 2)` | `2147483647` | operands reinterpreted as unsigned |
| `(remu -1 2)` | `1` | |
| `(shl 1 33)` | `2` | shift count masked to 5 bits (`33 & 31 == 1`) |
| `(shr -8 1)` | `-4` | arithmetic shift, sign-extending |
| `(shru -8 1)` | `2147483644` | logical shift, zero-filling |

### 8.1 `i64`

`i64` is a first-class declarable type with the same wrapping two's-complement semantics at 64 bits. There is **no implicit widening**: an `i32` and an `i64` never meet in one operation, and the checker rejects `(+ 1 2i64)` with `Type mismatch in binary op: I32 vs I64`. Conversions are explicit ops that map one-to-one onto wasm instructions.

| Form | Meaning | wasm |
|---|---|---|
| `42i64`, `-7i64`, `4294967296i64` | 64-bit literal; the suffix is part of the token | `i64.const` |
| `(let x:i64 0i64)`, `[a:i64]`, `-> i64` | declarations | local/param/result `i64` |
| `+ - * / % divu remu ^ bitand bitor shl shr shru` on two `i64` | 64-bit wrapping; shift counts masked to 6 bits | `i64.add` ... `i64.shr_u` |
| `eq neq lt lte gt gte` on two `i64` | signed comparison, yields `bool` | `i64.eq` ... `i64.ge_s` |
| `(i64.extend_s x)` | `i32 -> i64`, sign-extending | `i64.extend_i32_s` |
| `(i64.extend_u x)` | `i32 -> i64`, zero-extending | `i64.extend_i32_u` |
| `(i32.wrap x)` | `i64 -> i32`, low 32 bits | `i32.wrap_i64` |
| `(f64.convert_i64_s x)` | `i64 -> f64`, rounded to nearest (ties to even) | `f64.convert_i64_s` |
| `(i64.trunc_f64_s x)` | `f64 -> i64`, toward zero; NaN or out of range is a VM error / wasm trap | `i64.trunc_f64_s` |
| `(f64.reinterpret_i64 x)` / `(i64.reinterpret_f64 x)` | same 64 bits, other type | `f64.reinterpret_i64` / `i64.reinterpret_f64` |
| `(mem.load64 p)` | reads 8 little-endian bytes as `i64` | `i64.load` |
| `(mem.store64 p v)` | `v` must be `i64` | `i64.store` |

| Expression | Result |
|---|---|
| `(+ 2147483647i64 1i64)` | `2147483648` (no i32 wrap) |
| `(+ 9223372036854775807i64 1i64)` | `-9223372036854775808` |
| `(* 4294967296i64 2147483648i64)` | `-9223372036854775808` |
| `(divu -1i64 2i64)` | `9223372036854775807` |
| `(shl 1i64 65i64)` | `2` (count masked: `65 & 63 == 1`) |
| `(i64.extend_s -1)` | `-1` |
| `(i64.extend_u -1)` | `4294967295` |
| `(i32.wrap 4294967301i64)` | `5` |
| `(i32.wrap (+ (i64.extend_s 2147483647) 1i64))` | `-2147483648` |

Loop bounds, memory addresses, `mem.alloc` sizes, file descriptors, and thread handles remain `i32`. To index memory with an `i64` computation, narrow it first with `i32.wrap`. The VM prints an `i64` result as `Int64(n)`, an `i32` as `Int(n)`.

### 8.2 Type-directed code generation

The wasm backend selects instructions from the static operand type (`i32` / `i64` / `f32` / `f64`), and an `if` whose branches are `i64` or `f64` gets a matching block result type. `f64` arithmetic (`+ - * /`) and comparisons therefore compile and validate. `%`, `divu`, `remu`, shifts, and bitwise ops on floats are rejected at compile time with `Wasm Codegen: <op> is not supported for operands of type F64`.

**Status:** the wrapping rules above for both widths are implemented in `src/vm.rs`, covered by `tests/test_i64.rs` (VM result plus wasm validation), and proven equal across backends by `tests/test_differential.rs`, which executes every case in this section in both the VM and wasmtime and asserts identical results. When the two disagree, wasm is right and the VM is fixed (section 10.4).

---

## 9. Diagnostics

Every parser and checker error is a single line of the form

```
<line>:<col>: <message>
```

with 1-based line and column of the offending token or the opening `(` of the offending form. The resolver prefixes the file path: `aipl_src/memory.aipl: 12:5: Undefined variable 'foo'`. Contract failures carry the contract's position (section 7). Other VM runtime errors (division by zero, out-of-bounds memory, unknown thread handle) currently have **no** position.

Representative messages, exactly as produced:

| Situation | Message |
|---|---|
| missing `)` at end of file | `2:27: Expected RParen, got EOF` |
| extra `)` after the module | `3:1: unexpected tokens after module end — check for an extra ')'` |
| unknown operator | `3:5: Unknown op/keyword: badop` |
| unterminated string | `4:12: Unterminated string literal` |
| `(ptr i32)` in a type position | `1:20: Unknown compound type: ptr` |
| wrong literal type in `let` | `3:5: Type mismatch in 'let': expected I32, got Bool` |
| undefined name | `3:5: Undefined variable 'x'` |
| `set!` before `let` | `3:5: Undefined variable 'x' in set!` |
| `if` branches disagree | `3:5: If branch type mismatch: then is Void, else is I32. If mixing void and non-void, consider wrapping in (block ... value)` |
| `let` of a name already in scope | `3:12: Cannot shadow existing variable 'x'` |
| function body ends in `let` | `2:3: Function 'f' expects return type I32, but body returned Void` |
| mixed `i32` / `i64` operands | `3:5: Type mismatch in binary op: I32 vs I64` |
| unknown struct | `3:5: Unknown struct 'P'` |
| unknown field | `3:5: Struct 'P' has no field 'z'` |
| wrong `put` value type | `3:5: Type mismatch writing to field 'P.x': expected I32, got Bool` |
| struct defined twice in one module | `1:30: Duplicate struct definition 'P'` |
| `get`/`put` through the wrong pointer | `4:5: get Point.x needs a (ptr Point), got Ptr(Struct("Node"))` |
| an `i32` where a pointer is expected | `3:5: get Point.x needs a (ptr Point), got I32` |
| arithmetic on a pointer | `3:5: Add on Ptr(Struct("Point")): pointers, arrays, and function refs have no arithmetic; use get/put or arr.get/arr.set, or convert with ptr.addr/arr.addr and ptr.cast/arr.cast` |
| `arr.get` with the wrong element type | `4:5: arr.get I64 needs an (arr I64), got Array(I32)` |
| `(ptr i32)` | `1:20: (ptr i32) is not a type: ptr points to a struct; for a sequence of i32 use (arr i32)` |
| `call_ref` signature differs from the reference's type | `3:5: call_ref signature Fn([I32, I32], I64) does not match the function's type Fn([I32, I32], I32)` |
| wrong `thread.spawn` worker | `1:51: thread.spawn needs a worker of type (fn [i32] -> i32), got Fn([I64], I32)` |
| array index out of range (runtime, VM) | `Array index out of bounds: index 3 for array of length 3` |
| 64-bit result payload in the wasm backend | `Wasm Codegen: result payloads must be 32-bit (i32, bool, str), got I64` |
| non-bool `if` condition | `3:5: If condition must be Bool, got I32` |
| `break` outside a loop | `3:5: break is only allowed inside a while or loop body` |
| `return` with the wrong type | `3:5: return value has type Bool, but the function returns I32` |
| `cond` without `else` | `1:32: cond needs a final (else ...) clause, as if needs an else branch` |
| wrong arity | `4:5: Function 'add' expects 2 arguments, got 1` |
| wrong argument type | `4:5: Arg 1 of 'add' expects I32, got Bool` |
| body/return mismatch | `2:3: Function 'f' expects return type I32, but body returned Bool` |
| unsupported op in a backend | `Wasm Codegen: SysTime is not supported in the wasm backend` or `SysTime not supported in VM backend: system ops not implemented` |
| store or lock at literal address 0 | `3:5: AtomicLock at address 0: bytes 0-3 are the heap cursor owned by mem.alloc; locking it hangs and storing to it corrupts the allocator. Take memory from (mem.alloc n) instead` |
| literal address in 64-1023 | `3:5: MemStore32 at literal address 512: bytes 64-1023 are the reserved runtime block. Take memory from (mem.alloc n) instead` |
| locking a word that is not 0/1 (runtime, VM) | `atomic.lock: word at ptr 1024 holds 1024, which is not a lock state (0 = free, 1 = held); this address is data, not a mutex. Allocate a dedicated lock word with (mem.alloc 4)` |

Type names in messages are the Rust `Debug` spelling: `I32`, `F64`, `Bool`, `Str`, `Void`, `ResultType(I32, I32)`.

---

## 10. Testing Conventions

There are three layers of tests. Each one must fail loudly on fabricated work; a test that only asserts "returned a number" or "produced more than N bytes" is not acceptable in this repository (see `AIPL_Structural_Audit.md`, item U6).

### 10.1 AIPL-native self-tests (`aipl test`)

Each real module in `aipl_src/` exposes a zero-arg `run_<module>_tests` function that returns an `i32` count of passing sub-tests (or `1` for a single pass / `0` for fail). The suite entry point `aipl_src/test_suite.aipl` imports every verified module, calls each runner, compares against the expected count, prints one `[PASS]`/`[FAIL]` line per group with `sys.print`, and returns the number of failing groups. The Rust `test` subcommand turns that into the exit code.

```lisp
;; aipl_src/test_suite.aipl (abridged)
(module test_suite
  (import compiler)
  (import memory)

  (fn report [label:str result:i32 expected:i32] -> i32
    (if (eq result expected)
        (block (sys.print (+ "[PASS] " label)) 0)
        (block (sys.print (+ "[FAIL] " label)) 1)))

  (fn run_all [] -> i32
    (let failures:i32 0)
    (set! failures (+ failures (call report "compiler: tokenizer (2 tests)" (call compiler.run_tokenizer_tests) 2)))
    (set! failures (+ failures (call report "memory: allocator + arena"     (call memory.run_memory_tests)     1)))
    failures))
```

Pattern for a module's own runner: perform real work, then assert on the **content** of the result, not its shape.

```lisp
;; aipl_src/memory.aipl
(fn run_memory_tests [] -> i32
  (let p1:i32 (call aipl_heap_alloc 64))
  (let p2:i32 (call aipl_heap_alloc 128))
  (let arena:i32 (call arena_create 1024))
  (let a1:i32 (call arena_alloc arena 32))
  (let a2:i32 (call arena_alloc arena 64))
  (let _r:i32 (call arena_reset arena))
  (let a3:i32 (call arena_alloc arena 32))
  (if (and (gt p2 p1) (and (gt a2 a1) (eq a3 a1))) 1 0))
```

Expected terminal output:
```
[AIPL Test] Running 'run_all' from 'aipl_src/test_suite.aipl'...

[PASS] compiler: tokenizer (2 tests)
[PASS] compiler: parser (2 tests)
[PASS] codegen: signatures + 3 real wasm modules (4 tests)
[PASS] memory: allocator + arena
[PASS] file_io: real disk round-trip

[AIPL Test] All groups passed.
```

Rules: import a module into `test_suite.aipl` only once its runner is verified to do real work. `thread_sync.aipl` is imported like every other module: `thread.spawn` takes a function reference, which survives the resolver's renaming (P10).

### 10.2 Rust integration tests (`cargo test`, `tests/*.rs`)

Rust tests drive the library crate `aipl_core` directly and are the only way to call AIPL functions with arguments. The canonical shape exercises **every** stage and asserts concrete values:

```rust
use aipl_core::checker::TypeChecker;
use aipl_core::compiler::wasm::WasmCompiler;
use aipl_core::parser::Parser;
use aipl_core::vm::{Value, VM};
use wasmparser::Validator;

#[test]
fn fib_runs_and_compiles() {
    let src = r#"
    (module compute
      (fn fib [n:i32] -> i32
        (if (lte n 1) n
            (+ (call fib (- n 1)) (call fib (- n 2))))))
    "#;
    let module = Parser::parse(src).expect("parse");
    TypeChecker::new().check_module(&module).expect("check");

    let mut vm = VM::new();
    vm.load_module(module.clone());
    assert_eq!(vm.invoke("fib", vec![Value::Int(7)]).unwrap(), Value::Int(13));

    let wasm = WasmCompiler::compile(&module).expect("compile");
    assert!(wasm.starts_with(&[0x00, 0x61, 0x73, 0x6D]));
    Validator::new().validate_all(&wasm).expect("wasm must validate");
}
```

To assert a diagnostic, match on the exact `L:C:` prefix so a regression in position tracking fails the test:

```rust
let err = Parser::parse("(module m\n  (fn f [] -> i32 42)").unwrap_err();
assert!(err.starts_with("2:21:"), "got {err}");
```

To assert that a backend **rejects** something rather than faking it:

```rust
let module = Parser::parse("(module m (fn f [] -> void (sys.print \"x\")))").unwrap();
let err = WasmCompiler::compile(&module).unwrap_err();
assert!(err.contains("sys.print not supported in wasm backend"));
```

Files today (over 200 tests): `tests/test_all.rs` (pipeline smoke), `tests/test_v2.rs` (memory, atomics across real threads, real file I/O, results, imports), `tests/test_diagnostics.rs` (exact `L:C:` prefixes), `tests/test_i64.rs` (64-bit type, VM plus wasm validation), `tests/test_memory_layout.rs` (reserved-block enforcement in both backends), `tests/test_opcode_conformance.rs` (10.3), `tests/test_differential.rs` (10.4), `tests/test_wasi.rs` (10.5), `tests/test_selfhost.rs` (10.6), `tests/test_doc_examples.rs` (10.7), `tests/test_pointers.rs` (strict pointer/array typing, VM/wasm agreement, struct namespacing across imports), `tests/test_std.rs` (every eligible standard-library function in both backends under WASI, plus exact printed output), `tests/test_printer.rs` (source round trip of every repository program), `tests/test_refs.rs` (function references in both backends, signature checks, refs across imports), `tests/test_control_flow.rs` (return/break/continue/cond, short-circuit `and`/`or`, loop bounds evaluated once, in both backends; checker rejections), `tests/test_threads.rs` (threaded modules under a wasi-threads host against the VM), `tests/test_generics.rs` (template expansion in both backends, Rust/AIPL parity, errors), `tests/test_resolver_aipl.rs` (the AIPL resolver against the Rust one, subdirectory imports), `tests/test_runner.rs` (the `aipl-run` launcher and `aipl compile --exe` executables), and the native backend's `tests/test_native_reader.rs` (the wasm reader against wasmparser), `tests/test_native_x64.rs` (the encoder against GNU as), and `tests/test_native_elf.rs` (runs the first native executable). `tests/test_differential.rs` also checks that allocation grows memory to the same page count in both backends.

### 10.3 Opcode conformance contract

`tests/test_opcode_conformance.rs` holds a minimal program for every `OpCode` variant. The `match` over `OpCode` has no wildcard arm, so adding a variant to `src/ast.rs` without a program is a compile error. For each variant the test asserts exactly one of:

- **(a)** the VM returns `Ok`, the wasm backend returns bytes, and `wasmparser` validates them; or
- **(b)** the checker, the VM, or the wasm backend returns `Err`.

An op that "succeeds" by returning a default value in one backend and a no-op in the other satisfies neither and fails the suite. New opcodes must be added to this file in the same commit that adds them to the AST.

### 10.4 Differential testing (`tests/test_differential.rs`)

This is what makes "wasm semantics are the spec" enforceable. `wasmtime` is a dev-dependency; the harness compiles a module with `WasmCompiler`, instantiates the bytes in wasmtime (no imports are needed), and calls the export. `differential(module, wasm, fn_name, args)` runs the same call in a fresh `VM` and a fresh wasmtime instance and asserts one of:

- both return the same value (`bool` is normalised to `Int(0|1)`, its wasm shape), or
- both fail (VM `Err` and wasmtime trap, e.g. division by zero).

Two VM-only checks are **not** divergences when paired with a wasmtime success: a `req`/`ens` failure (the wasm backend emits no contracts) and `Array index out of bounds` (the wasm backend does not bounds-check, section 4.E). Any other mismatch panics with `DIVERGENCE ... (fix src/vm.rs)`. The wasm side is never changed to match the interpreter.

Coverage:

- Every edge case in section 8 and 8.1 as a single-expression program.
- Control flow: inclusive `loop` bound, stepped and negative-start loops, `while` with `set!`, nested `if`/`block` values, recursion, memory round trips through the bump allocator.
- Typing and scoping (P7): one case per rule.
- Structs and arrays (P8): `i32`, `bool`, `i64`, `f32`, `f64` fields at their aligned offsets; `sizeof` with padding; `i32` and `i64` arrays including the length header and cursor advance; a size expression that itself allocates; the heap addresses left by `ok`/`err` cells and by an allocation inside a result payload; negative `arr.new` sizes and `put`/`arr.set` into the reserved block failing in both; out-of-range indexes failing in the VM only; and the section 4.E example.
- Every `examples/*.aipl`: resolved, checked, compiled; every function returning `i32`/`i64`/`bool` with all-`i32` params is called over nine fixed argument tuples in both backends. Files the wasm backend rejects are skipped with a printed reason, and files that do I/O (`word_count`, `word_freq`) are compared with captured output in `tests/test_wasi.rs` instead. Every example must parse (the stale-file list is empty). The test asserts at least 4 files and 40 calls were compared so it cannot silently go vacuous; today it compares `accounts`, `math_core`, `matrix_mult`, and `quicksort` over 116 calls. Every example is also compiled by the self-hosted compiler at byte parity (`tests/test_selfhost.rs`).
- `aipl_src/codegen.aipl`'s `test_signatures_and_locals` is compared under WASI (the module uses `fs.*` and so imports WASI).

Sample argument values are deliberately small. Example functions use parameters as loop bounds, and a tree-walking VM asked to iterate `i32::MAX` times is not a test, it is a hang. Wrap-around is covered by the explicit expression cases instead.

### 10.5 I/O under WASI (`tests/test_wasi.rs`)

Compiled modules that print or touch files import from `wasi_snapshot_preview1`, so they need a WASI host. The tests build one with `wasmtime-wasi`: a `WasiCtxBuilder` with stdout captured into a `MemoryOutputPipe`, stderr inherited, and a fresh temporary directory preopened as `.` with read-write permission, linked through `wasmtime_wasi::p1::add_to_linker_sync`. The VM side of each comparison runs with the process cwd switched to its own temporary directory (under a lock, since cwd is process-global) so both backends see an empty directory.

Covered: `aipl_src/file_io.aipl`'s `run_file_io_tests` returns 1 in both backends and leaves no file behind; `sys.print` output is exactly one line per argument; interned string literals report their length and compare by identity; 600 bytes of literals compile and run in both backends, and literals past the 1 MiB limit fail to compile with a message; opening a missing file returns -1 in both; a write-then-read round trip agrees byte for byte and the wasm side's file is visible on the host in the preopened directory; `sys.exit 7` surfaces as `I32Exit(7)` from wasmtime and as `sys.exit(7) requested` from the VM; and a module with no I/O has no import section and still instantiates with no imports.

To run compiled I/O outside the tests: `wasmtime run --dir=. module.wasm --invoke main`.

### 10.6 Self-hosted byte parity (`tests/test_selfhost.rs`)

`self_host(src)` runs `codegen.compile_module` in a fresh VM, validates the output with `wasmparser`, and returns the bytes or the compile error code. `assert_self_hosted_matches_rust` asserts the whole module equals `WasmCompiler::compile` for the same source and prints the first differing function and byte if not. Coverage: minimal, `add`, `i64` (literals at both extremes, unsigned ops, an `i64` struct field after a `bool`, `i64` arrays, `mem.load64`/`store64`), `f32`/`f64` parameters, arithmetic and struct fields, 200 pseudo-random float literals plus out-of-range ones (973), `(ok:T v)` with compound `T`, an escaped quote inside a string, `compute` (loop + call), structs with `bool`/`str` fields, arrays, `sys.print`, results (including `match_result` as a statement), file I/O, repeated string literals with a `str` let plus `mem.alloc`/`mem.grow`/`sys.exit`, a mixed program, void `if` with block-scoped `let`s and an empty `(block)` else, a store, arrays plus results, `aipl_src/memory.aipl`, `aipl_src/compiler.aipl`, and codegen.aipl compiling itself (about 80 s in a debug build). Behavioural checks run the self-hosted output in wasmtime: the store guard traps on bytes 0-3 and 64-1023 and nowhere else in a module without literals, and also on the literals in a module with 800 bytes of them, whose heap then starts at 1832 in both the VM and wasm; arrays and results compute the same values as the VM and a negative `arr.new` traps; a file write plus `sys.print` under WASI produces the file and the exact stdout. Compile errors 95, 96, and 768 are asserted for non-scalar fields/elements, unknown structs, and a 70 000-byte literal (over the self-hosted 64 KiB buffer).

**Bootstrap fixpoint.** `self_hosted_compiler_reproduces_itself_under_wasmtime` compiles the self-hosted compiler with the Rust backend (stage 1), runs that wasm module under wasmtime on its own source, and requires the output (stage 2) to be byte-identical to stage 1. The second compile involves neither the VM nor any Rust compiler code, and takes about 20 ms. Since P9, codegen.aipl itself uses `i64` and `f64` arithmetic (literal parsing, LEB128, float literal bits), so the fixpoint also covers those paths of the self-hosted compiler.

### 10.7 Documentation examples (`tests/test_doc_examples.rs`)

Every ```` ```lisp ```` block in `PROMPT_GUIDE_FOR_AIS.md` and `README.md` must parse, check, and compile. If it defines `main`, `main` must return the value listed in the test's `EXPECTED` table in both the VM and wasmtime (with WASI and a scratch directory). Adding an example with a `main` means adding its expected value there. This test found that a `match_result` used as a statement compiled to invalid wasm.

---

## 11. Imports and Multi-Module Programs

```lisp
;; util.aipl
(module util
  (fn double [x:i32] -> i32 (* x 2)))

;; main.aipl
(module app
  (import util)
  (fn main [] -> i32 (call util.double 21)))      ;; => Int(42)
```

- `(import name)` finds `name.aipl` by searching, in order: the importing file's directory, the entry file's directory, the standard library (`aipl_src/std/`), then each directory listed in the colon-separated `AIPL_PATH` environment variable. The first match wins, so a local `io.aipl` shadows the standard one. The resolver parses it and merges its functions into the entry module renamed as `name.fn`. `(import name as u)` lets you write `(call u.double ...)` locally; it is rewritten to `util.double` before checking.
- An import may name a file in a subdirectory: `(import native/wasm_reader)` finds `native/wasm_reader.aipl` in the same places, relative to each. The module is named by the last segment (`wasm_reader.decode`), so a sibling inside `native/` that writes `(import wasm_reader)` reaches the same module. Two different files with the same name in one program are an error (`two different modules are named 'x'`). An import path is names separated by `/`: no leading `/`, no `.` or `..`.
- Resolution is implemented twice: `src/resolver.rs` (used by every `aipl` command) and `aipl_src/resolver.aipl` (used by `aipl compile --self` and by the self-hosted toolchain compiled to wasm). The AIPL resolver produces flat source text: every struct, then every function, imported modules first in resolution order, comments dropped. The host passes the standard library directory; the resolver reads `AIPL_PATH` itself with `std/os`. `tests/test_resolver_aipl.rs` requires both to produce the same compiled bytes for every program with imports in the repository, plus aliases, diamonds, cycles, and missing modules (`circular import: NAME`, `cannot find module: NAME`).
- Import depth is flattened to one level: a function from a module imported by an import is still `directimport.fn`, not `a.b.fn`. Diamond imports produce one copy. Cycles are an error naming the file.
- The entry module's own functions keep bare names. In a compiled `.wasm`, exports are `main` and `util.double`.
- Structs follow the same rule: `(struct Node ...)` in `util` is `util.Node` everywhere outside `util` (`(ptr util.Node)`, `(new util.Node)`, `(get p util.Node.val)`), aliases included, so two imports may each define `Node`. An importer's bare `Node` never reaches into an import.
- `(ref f)` is rewritten like `(call f ...)`, so a function reference inside an imported module points at the qualified function, including the worker of a `thread.spawn`.

---

## 12. Worked Examples

Each example is complete and runs with the command shown. Expected output is what the current toolchain prints.

### 12.1 Pure computation, both backends

```lisp
;; gcd.aipl
(module gcd_demo
  (fn gcd [a:i32 b:i32] -> i32
    (req (and (gt a 0) (gt b 0)))
    (ens (gt res 0))
    (let x:i32 a)
    (let y:i32 b)
    (while (neq y 0)
      (let t:i32 y)
      (set! y (% x y))
      (set! x t))
    x)

  (fn main [] -> i32 (call gcd 1071 462)))
```
```
$ aipl eval gcd.aipl
[AIPL VM] Executing function 'main' from 'gcd.aipl'...
[AIPL Result]: Int(21)
$ aipl compile gcd.aipl -o gcd.wasm
[AIPL Compiler] Successfully compiled 'gcd.aipl' -> 'gcd.wasm' (N bytes)
```
The `.wasm` exports `gcd`, `main`, and `memory`. Note the `let t` inside the `while` body: it becomes a wasm local of the function and is re-assigned each iteration, which is the intended semantics.

### 12.2 Memory as a data structure, both backends

```lisp
(module stack_demo
  ;; stack layout: [count:i32][slot0:i32][slot1:i32]...
  (fn stack_new [cap:i32] -> i32
    (let s:i32 (mem.alloc (+ 4 (* cap 4))))
    (mem.store32 s 0)
    s)
  (fn stack_push [s:i32 v:i32] -> void
    (let n:i32 (mem.load32 s))
    (mem.store32 (+ s (+ 4 (* n 4))) v)
    (mem.store32 s (+ n 1)))
  (fn stack_pop [s:i32] -> i32
    (req (gt (mem.load32 s) 0))
    (let n:i32 (- (mem.load32 s) 1))
    (mem.store32 s n)
    (mem.load32 (+ s (+ 4 (* n 4)))))
  (fn main [] -> i32
    (let s:i32 (call stack_new 4))
    (call stack_push s 10)
    (call stack_push s 32)
    (+ (call stack_pop s) (call stack_pop s))))     ;; => Int(42)
```

### 12.3 Printing and strings, both backends

```lisp
(module hello
  (fn main [] -> i32
    (sys.print "hello, aipl")
    (str.len "hello, aipl")))
```
`aipl eval hello.aipl` prints `hello, aipl` then `[AIPL Result]: Int(11)`. `aipl compile hello.aipl` produces a module importing `wasi_snapshot_preview1::fd_write`; run it with any WASI host (for example `wasmtime run hello.wasm --invoke main`) and it prints the same line. String concatenation `(+ "a" "b")` is the one string feature still VM-only: the wasm backend rejects it.

### 12.4 Threads and atomics, both backends

The worker is a function reference of type `(fn [i32] -> i32)`; the single `i32` argument is the natural way to hand it a pointer.

```lisp
(module counter_demo
  (fn worker [counter:i32] -> i32
    (loop i 1 1000 1
      (atomic.add counter 1))
    0)

  (fn main [] -> i32
    (let counter:i32 (mem.alloc 4))
    (mem.store32 counter 0)
    (let t1:i32 (thread.spawn (ref worker) counter))
    (let t2:i32 (thread.spawn (ref worker) counter))
    (let _a:i32 (thread.join t1))
    (let _b:i32 (thread.join t2))
    (mem.load32 counter)))            ;; => Int(2000), deterministically
```
`atomic.add` returns the previous value; the call above is in statement position so the value is dropped. Threads share linear memory (and therefore the allocator) and the function table, but not locals. Compiled, this is a threaded module (section 4.D): it runs under AIPL's runner, not the stock `wasmtime` CLI.

### 12.5 File round-trip, both backends

Paths are `(ptr, len)` pairs into linear memory, matching the WASI convention. `fs.open` flags: `0` read-only, non-zero write/create/truncate. All `fs.*` return `-1` on failure rather than raising. In the VM the path is relative to the process cwd; under WASI it is relative to the preopened directory (fd 3), so a host must preopen one, e.g. `wasmtime run --dir=. file_demo.wasm --invoke main`.

```lisp
(module file_demo
  (fn main [] -> i32
    (let path:i32 (mem.alloc 8))
    (let out:i32 (mem.alloc 4))
    (let in:i32 (mem.alloc 4))
    ;; "t.bin"
    (mem.store8 (+ path 0) 116) (mem.store8 (+ path 1) 46) (mem.store8 (+ path 2) 98)
    (mem.store8 (+ path 3) 105) (mem.store8 (+ path 4) 110)
    (mem.store32 out 3735928559)              ;; 0xDEADBEEF, wraps to -559038737
    (let w:i32 (fs.open path 5 1))
    (let _n:i32 (fs.write w out 4))
    (let _c:i32 (fs.close w))
    (let r:i32 (fs.open path 5 0))
    (let _m:i32 (fs.read r in 4))
    (let _d:i32 (fs.close r))
    (let _x:i32 (fs.delete path 5))
    (if (eq (mem.load32 in) (mem.load32 out)) 1 0)))   ;; => Int(1)
```

---

### 12.6 The standard library (`aipl_src/std/`)

Written in AIPL over `fs.*`, `mem.*`, `str.len`, and `str.ptr` (no Rust opcodes), so each function behaves identically in the VM and compiled under WASI. Import modules by name: `(import io)`, `(import vec)`, and so on.

| Module | Contents |
|---|---|
| `str` | `(struct Bytes [addr:i32 len:i32])`, a byte slice (`len` -1 marks a failed read). `bytes [addr len] -> (ptr Bytes)`, `from_str [s:str] -> (ptr Bytes)`, `byte_at`, `is_space [c] -> bool` (space and `\t \n \v \f \r`), `bytes_eq [a b] -> bool`, `find_byte [b c] -> i32` (first index or -1), `count_byte`, `count_lines` (newlines plus an unterminated last line), `count_words` (runs of non-space bytes), `parse_int [b] -> (result i32 i32)` (`(ok n)`, or `(err i)` with the index of the first bad byte; optional leading `-`) |
| `fmt` | `uint_to_bytes [n out] -> i32` (n read as unsigned), `int_to_bytes` (leading `-`), `hex_to_bytes` (lowercase, no prefix): each writes ASCII at `out` and returns the count (at most 10, 11, and 8 bytes) |
| `io` | `read_stdin [] -> (ptr str.Bytes)` (all of stdin), `write_str [fd s]`, `println [s]`, `eprintln [s]` (stderr), `print_int [n]`, `println_int [label n]` (prints `label`, then `n`, then a newline), `read_file [path:str] -> (ptr str.Bytes)` (whole file; `len` -1 on failure), `write_file [path:str b:(ptr str.Bytes)] -> i32` (bytes written or -1), and `read_path` / `write_path`, the same for a path held as `(ptr str.Bytes)` |
| `vec` | generic growable list `(vec.Vec T)`: `(call (vec.make T) capacity)`, `push`, `pop`, `at [v i]`, `set [v i x]`, `len`, `clear`, `index_of`, `sort_by [v cmp:(fn [T T] -> i32)]` (stable merge sort; `cmp` negative puts the first argument first), each called as `(call (vec.push T) v x)`; plus `sort_i32 [v:(ptr (vec.Vec i32))]` and `cmp_i32` |
| `map` | generic hash map `(map.Map V)` from `i32` keys: `(call (map.make V) capacity)`, `set [m k v]`, `get_or [m k default]`, `has`, `remove -> bool`, `count`; iterate with `(loop i 0 (- (call (map.capacity V) m) 1) 1 (if (call (map.slot_used V) m i) ... (block)))` reading `slot_key` / `slot_val` |
| `strmap` | generic hash map `(strmap.StrMap V)` from byte strings (symbol tables, word counts): the same API as `map` with keys of type `(ptr str.Bytes)`; the map keeps the key pointer, so a key's bytes must not change while it is stored |
| `os` | the command line and environment: `arg_count [] -> i32` (argv[0], the program, included), `arg [i] -> (ptr str.Bytes)` (`len` -1 past the end), `env [name:str] -> (ptr str.Bytes)` (`len` -1 if unset), each fetching a fresh copy; `random_i32 [] -> i32` from the OS generator |
| `buf` | string builder: `make [capacity] -> (ptr buf.Buf)`, `push_byte`, `push_str [b s:str]`, `push_bytes [b (ptr str.Bytes)]`, `push_int`, `len`, `clear`, `bytes [b] -> (ptr str.Bytes)` (a view of the contents; take it after building) |

Containers are generic (section 4.H): a list of points is `(ptr (vec.Vec (ptr Point)))`, filled with `(call (vec.push (ptr Point)) v p)` and read with `(call (vec.at (ptr Point)) v i)`, with no casts.

From outside, the slice type is `str.Bytes`: `(ptr str.Bytes)`, `(get b str.Bytes.len)`. Every module ends in a `run_<module>_tests` runner wired into `aipl_src/test_suite.aipl`. Allocation grows memory as needed (section 4.A), but nothing is freed (`mem.free` is a no-op): `print_int` allocates 11 bytes per call, `read_file` a buffer per file, and growing a `vec`, `map`, or `buf` abandons the old storage. Long-running programs should reuse containers (`clear`) rather than make new ones.

### 12.7 A complete I/O program with the standard library, both backends

[examples/word_count.aipl](examples/word_count.aipl) reads `input.txt`, prints its line, word, and byte counts, writes an error to stderr if the file cannot be read, and returns the line count. Twelve code lines, against 49 before the library existed:

```lisp
(module word_count
  (import io)
  (import str)
  (fn main [] -> i32
    (let b:(ptr str.Bytes) (call io.read_file "input.txt"))
    (if (lt (get b str.Bytes.len) 0)
        (block (call io.eprintln "word_count: cannot open input.txt") -1)
        (block
          (call io.println_int "lines: " (call str.count_lines b))
          (call io.println_int "words: " (call str.count_words b))
          (call io.println_int "bytes: " (get b str.Bytes.len))
          (call str.count_lines b)))))
```
```
$ cd examples && ../target/debug/aipl eval word_count.aipl
lines: 4
words: 15
bytes: 81
[AIPL Result]: Int(4)
$ aipl compile examples/word_count.aipl -o word_count.wasm
$ cd examples && wasmtime run --dir=. ../word_count.wasm --invoke main
lines: 4
words: 15
bytes: 81
```
`tests/test_wasi.rs` runs this program in both backends against a generated input and against the shipped `examples/input.txt`, and asserts the exact stdout and return value; `aipl compile --self` reports byte parity for it and for each `std` module.

## 13. Pitfalls for Code Generators

Each of these is a real failure mode observed when LLMs write AIPL. The fix is in the second column.

| Mistake | Correct form |
|---|---|
| `(f x)` to call a user function | `(call f x)`; bare `(name ...)` is only for built-in ops |
| `(if c (set! x 1))` with no else | `if` always takes three arguments; for a statement write `(if c (set! x 1) (block))`; both branches must be void or both the same value type |
| `(let x 5)` | `(let x:i32 5)`; the type annotation is mandatory |
| `(let x:i32 5)` inside a block when `x` is already in scope | shadowing is a type error (`Cannot shadow existing variable`); `let` is block-scoped, so reuse a name only in sibling blocks |
| `(set! y 1)` without a prior `let y` | declare first; there are no implicit globals |
| `(+ n 1.0)` or `(eq n 0.0)` on an `i32` | all operands to one op share one type; write `1` or convert explicitly |
| returning `void` from an `-> i32` function (body ends in `while`/`loop`/`set!` to a `void`) | end the body with a value expression, e.g. the accumulator name |
| `(and a b c)` with three operands | `and`/`or` take exactly two: `(and a (and b c))`. They short-circuit, so `(and (lt i n) (eq (arr.get i32 a i) x))` is a safe guard |
| `(fn (i32) -> i32)` or `(fn [i32] i32)` | the function type is `(fn [i32] -> i32)`: brackets around the parameters, then `->` |
| `(call_ref f x)` or `(call f x)` where `f` is a reference | `(call_ref (fn [i32] -> i32) f x)`: the signature is part of the call |
| `(thread.spawn name len arg)` with a name in memory | `(thread.spawn (ref worker) arg)`, `worker` of type `(fn [i32] -> i32)` |
| `(let p:i32 (new Point))`, or an `i32` parameter that holds a pointer | use `(ptr Point)`; arrays are `(arr T)`. `new` and `arr.new` never return `i32` |
| `(ptr i32)` for a sequence of numbers | `(arr i32)`, made by `(arr.new i32 n)` |
| `(eq p 0)` to test for null | `(eq p (ptr.null Point))` |
| `(+ p 4)` or `(mem.load32 p)` on a pointer | read fields with `get`; for raw memory, `(ptr.addr p)` first |
| a struct from an imported module written bare | qualify it: `(ptr geo.Point)`, `(get p geo.Point.x)` |
| `(mem.store32 512 x)`, `(atomic.lock 0)`, any literal address below 1024 | rejected by the checker: address 0 is the heap cursor and 64-1023 is reserved. Take memory from `(mem.alloc n)` and pass the pointer. A computed address that lands on a non-lock word fails at runtime in the VM instead of hanging |
| `(let x:i64 5)` or `(+ n 1i64)` where `n` is `i32` | no implicit widening: write `5i64`, or convert with `(i64.extend_s n)`; narrow back with `(i32.wrap x)` |
| `(loop i 0 n 1 ...)` expecting `n` iterations | `loop` is inclusive: this runs `n + 1` times; use `(- n 1)` |
| `(% a b)` with negative `a` expecting a positive result | `%` is `rem_s`; add `b` and take `%` again for a modulo |
| `(+ str str)` in code meant for `aipl compile` | VM-only; build strings with `std/buf`. Threads and atomics compile; a program using `thread.spawn` needs AIPL's runner (or another wasi-threads host) to run |
| `(get p x)` or `(get p Point x)` | the field is one symbol: `(get p Point.x)`; arrays name the element type every time: `(arr.get i32 a i)` |
| relying on `arr.get` to catch a bad index in compiled code | only the VM bounds-checks; check `(lt i (arr.len a))` yourself where it matters |
| `(ok 1i64)` or an `f64` payload in code meant for `aipl compile` | result payloads must be 32-bit in wasm; return an `i32` pointer to a struct instead |
| ending a function in `(let ...)` | `let` is void; end with the value, e.g. the variable name |
| `(sys.print n)` with an `i32` in code meant for `aipl compile` | the wasm backend prints `str` only; use `(call io.print_int n)` or `(call io.println_int "label " n)` from the standard library |
| hand-writing digit formatting, file-reading loops, byte counting, growable arrays, hash tables, or string building | the standard library (section 12.6): `io`, `str`, `fmt`, `vec`, `map`, `strmap`, `buf`, `os` |
| `(call vec.push v x)` on a generic container | name the element type: `(call (vec.push i32) v x)`; the container's type is `(ptr (vec.Vec i32))` (section 4.H) |
| `(get b Box.value)` on a generic struct | name the instance: `(get b (Box i32) value)` |
| calling `mem.grow` before allocating | not needed: allocation grows memory itself (up to 1024 pages) |
| passing a `str` literal where a `(ptr, len)` path or buffer is expected, e.g. `(fs.open "t.bin" 5 0)` | type error: `fs.*` take `i32` pointers. Write `(fs.open (str.ptr "t.bin") (str.len "t.bin") 0)` |
| `(if (lt i 0) (return -1) i)` | `return` is a statement (void): `(if (lt i 0) (return -1) (block))`, then the value |
| `(cond ((lt n 0) -1) ((eq n 0) 0))` without `else` | `cond` needs a final `(else ...)` clause; use `(else (block))` when the clauses are statements |
| `else if`, `elif`, `switch`, `case` | do not exist; use `cond` |
| `(break)` in a `while` condition or outside any loop | only inside a `while` or `loop` body |
| a `done`/`found` flag variable to stop a loop | `(break)`, or `(return v)` from the function |
| an extra `)` after the closing `(module` paren | reported as `L:C: unexpected tokens after module end — check for an extra ')'` |
| `inf`, `nan`, `1e9` as literals | not literals; `1e9` is a symbol and will be reported as an undefined variable |
| `(req n > 0)` | contracts are S-expressions: `(req (gt n 0))` |
| `(ens (gt result 0))` | the result is named `res`, always |
| declaring a test "done" because it returned a number | assert the value; see section 10 |
