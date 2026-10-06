# AIPL Specification
## AI Programming Language: Formal Specification

> **WebAssembly semantics are the specification.** Where this document is silent, a program means what its compiled wasm does. `i32` and `i64` are wrapping two's-complement, shifts mask their count, and traps are wasm's traps. The VM and the native backend must match wasmtime exactly, and any divergence is a bug in them: `tests/test_differential.rs` checks the VM, `tests/test_native.rs` the native executables.

AIPL (AI Programming Language) is an unambiguous, statically typed S-expression systems language with runtime-checked contracts, designed for AI agents to generate. It compiles to WebAssembly (with WASI for I/O), and on Linux x86-64 to native executables translated from that wasm (section 6.6). It also runs in a reference VM, and has a self-hosted compiler written in AIPL itself (section 6.4).

What AIPL optimises for is that a program has one obvious spelling and that mistakes are caught early with a `line:col` diagnostic, not brevity. With the standard library (section 12.6), a small I/O program is about twice the length of the Python equivalent (section 12.7), which is roughly the floor for a fully parenthesised, fully annotated syntax.

---

## 1. Syntax: One Canonical Text Form

AIPL source is `.aipl` text: a context-free, parenthesis-delimited S-expression syntax with no operator precedence, no indentation rules, and no statement separators. It is also the interchange format between agents. `src/printer.rs` prints any resolved program back as canonical flat text, which both compilers accept. (A MessagePack "binary AST" (`.baipl`) existed until P12; it was a dump of the compiler's internal Rust data structures with no stable format, nothing used it, and it was removed.)

---

## 2. Updated Formal Grammar (EBNF)

```ebnf
program        ::= "(" "module" identifier import* ( struct_def | enum_def | union_def | const_def | fn_def )* ")" ;
import         ::= "(" "import" import_path [ "as" identifier ] ")" ;
import_path    ::= identifier ( "/" identifier )* ;
struct_def     ::= "(" "struct" ( identifier | generic_head ) "[" field* "]" ")" ;
field          ::= identifier ":" type ;
generic_head   ::= "(" identifier type_param+ ")" ;
type_param     ::= identifier ;                     (* starts with an uppercase letter *)

enum_def       ::= "(" "enum" identifier "[" ( identifier | "(" identifier integer ")" )+ "]" ")" ;
const_def      ::= "(" "const" identifier ":" type literal ")" ;   (* NAME in capitals; i32 i64 f64 bool str *)
union_def      ::= "(" "union" identifier "[" ( "(" identifier field* ")" )+ "]" ")" ;

fn_def         ::= "(" "fn" ( identifier | generic_head ) "[" param* "]" "->" type contract* expr* ")" ;
param          ::= identifier ":" type ;
contract       ::= "(" ("req" | "ens" | "inv") expr ")" ;

type           ::= "i32" | "i64" | "f32" | "f64" | "bool" | "str" | "void"
                 | "(" "result" type type ")"
                 | "(" "ptr" struct_name ")"
                 | "(" "arr" type ")"
                 | "(" "fn" "[" type* "]" "->" type ")"
                 | "(" struct_name type+ ")"          (* a generic struct instance, used behind ptr *)
                 | type_param                         (* inside a generic definition *)
                 | identifier ;                       (* an enum or a union, e.g. Color, Shape *)
struct_name    ::= identifier                       (* "m.S" for a struct from imported module m *)
struct_ref     ::= struct_name | "(" struct_name type+ ")" ;
fn_name        ::= identifier | "(" identifier type+ ")" ;   (* the second form instantiates a generic *)

expr           ::= literal
                 | identifier
                 | "(" "let" identifier ":" type expr ")"
                 | "(" "set!" identifier expr ")"
                 | "(" "if" expr expr expr ")"
                 | "(" "loop" identifier expr expr expr expr* ")"
                 | "(" "while" expr expr* ")"
                 | "(" "call" fn_name expr* ")"
                 | "(" "block" expr* ")"
                 | "(" "return" [ expr ] ")" | "(" "break" ")" | "(" "continue" ")"
                 | "(" "cond" ( "(" expr expr+ ")" )+ "(" "else" expr+ ")" ")"
                 | "(" ("ok" | "err") [ ":" type ] expr ")"
                 | "(" "match_result" expr "(" "ok" identifier expr* ")" "(" "err" identifier expr* ")" ")"
                 | "(" "new" struct_ref ")"
                 | "(" "get" expr struct_name "." identifier ")"
                 | "(" "get" expr "(" struct_name type+ ")" identifier ")"
                 | "(" "put" expr struct_name "." identifier expr ")"
                 | "(" "put" expr "(" struct_name type+ ")" identifier expr ")"
                 | "(" "sizeof" struct_ref ")"
                 | "(" "arr.new" type expr ")"
                 | "(" "arr.get" type expr expr ")"
                 | "(" "arr.set" type expr expr expr ")"
                 | "(" "arr.len" expr ")"
                 | "(" "ptr.null" struct_ref ")" | "(" "arr.null" type ")"
                 | "(" "ptr.cast" struct_ref expr ")" | "(" "arr.cast" type expr ")"
                 | "(" "ptr.addr" expr ")" | "(" "arr.addr" expr ")"
                 | "(" "ref" fn_name ")"
                 | "(" "enum.cast" identifier expr ")" | "(" "enum.ord" expr ")"
                 | "(" "call_ref" type expr expr* ")"
                 | "(" "make" identifier expr* ")"     (* Union.variant *)
                 | "(" "match" expr match_arm+ ")"
                 | "(" op expr* ")" ;

match_arm      ::= "(" identifier [ "[" identifier* "]" ] expr* ")"   (* Name.member; binders for a union variant *)
                 | "(" "else" expr* ")" ;                             (* last *)

op             ::= arithmetic_op | bitwise_op | memory_op | atomic_op | comp_op
                 | conv_op | sys_op | fs_op | proc_op | thread_op | str_op ;

arithmetic_op  ::= "+" | "-" | "*" | "/" | "%" | "divu" | "remu" | "checked.add" | "checked.sub" | "checked.mul" ;
bitwise_op     ::= "^" | "shl" | "shr" | "shru" | "bitand" | "bitor" ;
memory_op      ::= "mem.load8" | "mem.load32" | "mem.load64"
                 | "mem.store8" | "mem.store32" | "mem.store64"
                 | "mem.alloc" | "mem.grow" ;
atomic_op      ::= "atomic.add" | "atomic.cas" | "atomic.lock" | "atomic.unlock" ;
comp_op        ::= "eq" | "neq" | "lt" | "lte" | "gt" | "gte" | "ltu" | "lteu" | "gtu" | "gteu" | "and" | "or" | "not" ;
conv_op        ::= "i64.extend_s" | "i64.extend_u" | "i32.wrap"
                 | "f64.convert_i64_s" | "i64.trunc_f64_s" | "f64.reinterpret_i64" | "i64.reinterpret_f64" | "f64.sqrt" ;
sys_op         ::= "sys.print" | "sys.time" | "sys.monotonic" | "sys.random" | "sys.exit" ;
fs_op          ::= "fs.open" | "fs.read" | "fs.write" | "fs.close" | "fs.delete" ;
proc_op        ::= "args.sizes" | "args.get" | "env.sizes" | "env.get" ;
thread_op      ::= "thread.spawn" | "thread.join" ;
str_op         ::= "str.len" | "str.ptr" ;
```

Notes on the grammar as implemented by `src/parser.rs`:
- A module body is imports followed by struct and function definitions, in any order. `let` and `set!` are expressions that appear inside function bodies, not at module level.
- Comments start with `;;` and run to end of line.
- Integer literals are decimal, optionally signed (`-1` and `+1` are one token) and are `i32`; the value must fit in 32 bits, read as signed or unsigned (`-2147483648` to `4294967295`, so `4294967295` is the bit pattern of `-1`); anything larger is an error suggesting the `i64` form. An `i64` literal carries the suffix as part of the token: `42i64`, `-7i64`. Float literals must contain a `.` and at least one digit (`1.0`, `.5`, `1.5e3`; not `1` or `inf`) and are `f64`.
- Tokens are separated by spaces, tabs, and newlines (and CR). Any other whitespace character outside strings and comments, such as a form feed or a no-break space, is an error that names it. Strings are double-quoted and support the escapes `\n \t \r \0 \\ \"`; any other `\x` is an error. Booleans are `true` / `false`.
- `(call f ...)` takes a bare function name, never an expression. Imported functions are called as `(call modname.fn ...)`.
- `i64` is fully supported (section 8.1). `(fn [t1 t2] -> r)` is the type of a function reference (section 4.G).
- The generic forms (`generic_head`, `struct_ref`, `fn_name`, the instance type) are expanded away before type checking (section 4.H), and so are constants: an identifier naming a constant is its literal, and `Enum.member` is a value of that enum (section 4.I).
- `inv` parses and type-checks but is never evaluated (section 3). There is no `mem.free` (memory is never freed) and there are no float loads or stores (`mem.load_f64` ...): floats live in struct fields and `(arr f64)`. The parser rejects all five with that advice.
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
- `Name` for an enum `Name`: one of its members (section 4.I). An `i32` at run time, a distinct type to the checker.
- `Name` for a union `Name`: one of its variants, carrying that variant's fields (section 4.J). A pointer to a heap cell at run time, never null.
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
- `(mem.alloc size)` -> Bump allocation: returns the current heap cursor (the `i32` at address 0) and advances it by `size` rounded up to a multiple of 8, so every block is 8-aligned (the heap start is too): atomics, `i64`/`f64` values, and WASI out-parameters placed in any allocated block are aligned. The claim is one atomic add, so threads may allocate concurrently. If the new cursor is past the end of memory, memory grows by the pages needed to cover it (up to the 32768-page (2 GiB) cap; beyond it nothing grows and the first access past the end fails). `new`, `arr.new`, and `ok`/`err` cells allocate the same way. Never frees. One cursor is shared by the VM, compiled wasm, and AIPL code.
- `(mem.grow pages)` -> Grows linear memory by `pages` × 64 KiB. Returns the previous size in pages, or `-1` if the 32768-page (2 GiB) maximum would be exceeded. The cap is 2 GiB so that every address is a non-negative `i32`: signed comparisons on addresses stay correct.
- There is no `mem.free`: nothing is ever freed. For memory used in phases, allocate from a region and reset it (`std/arena`).

See "Memory layout" (section 7.9) for the reserved runtime block below address 1024.

### B. Strings

A `str` is a pointer to immutable UTF-8 bytes preceded by a 4-byte little-endian length. String literals are interned once per module, in first-use order, into the data area that starts at address 1024; the heap starts at the first 8-aligned address after them (section 7.9). Literals are read-only: a store into one traps like a store into the runtime block. A module may have up to 1 MiB of literals (64 KiB in the self-hosted compiler, section 6.4). `(str.len s)` returns the byte length as `i32`; `(str.ptr s)` returns the address of the bytes as `i32`, which is how a string becomes the `(ptr, len)` pair that `fs.*` and other pointer-taking ops expect (identity in both backends: the VM places the literals at the same addresses as wasm when it loads a module, and copies a string onto the heap only if it is not one of the first-loaded module's literals). Two literals with the same text share one address, so `(eq "a" "a")` is `true` and `(eq "a" "b")` is `false` in both backends. There is no `+` on strings; text is built with `std/buf`. The VM represents a `str` as a Rust string rather than a pointer; the observable semantics above are the same.

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

Paths are `(ptr, len)` byte ranges in linear memory (`(str.ptr s)` / `(str.len s)` produce them from a string). In the VM they are ordinary paths. Under WASI a relative path resolves in the first preopened directory (fd 3, the working directory) and an absolute path (`/...`) in the second (fd 4), which a host grants as `/` when the program may use absolute paths: `wasmtime run --dir . --dir / prog.wasm`. Without that grant, opening an absolute path fails with `-1`. File descriptor 0 is stdin in both backends (`(fs.read 0 buf n)`; `io.read_stdin` reads all of it). A clock failure traps (it does not happen on supported hosts). File descriptors 1 and 2 are stdout and stderr in both backends, so `(fs.write 1 buf n)` prints raw bytes. The standard library builds printing of numbers and whole-file reads on exactly these primitives (section 12.6). Every WASI errno collapses to `-1`, matching the VM. Argument expressions are evaluated left to right in both backends. The `args.*`/`env.*` out-parameters `count_ptr`, `size_ptr`, and the address tables must be 4-aligned: WASI hosts trap on a misaligned one and the VM fails with `... is not 4-aligned, which WASI requires`. Like `fs.read`, the host writes through these addresses without the store guard. Under WASI the host decides what the program sees (wasmtime forwards an environment variable only with `--env`). Use `std/os` rather than these ops directly; time code with `std/time`.

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

### I. Constants and Enums

```lisp
(module shapes
  (const MAX_SIDES:i32 12)
  (const UNIT:f64 0.5)
  (const LABEL:str "shape")
  (enum Kind [triangle square (hexagon 6) octagon])   ;; 0, 1, 6, 7

  (fn sides [k:Kind] -> i32
    (cond
      ((eq k Kind.triangle) 3)
      ((eq k Kind.square) 4)
      (else (enum.ord k))))                            ;; hexagon 6, octagon 7

  (fn main [] -> i32
    (let ks:(arr Kind) (arr.new Kind 2))
    (arr.set Kind ks 0 Kind.square)
    (arr.set Kind ks 1 (enum.cast Kind 7))
    (+ (call sides (arr.get Kind ks 0))
       (+ (call sides (arr.get Kind ks 1)) (+ MAX_SIDES (str.len LABEL))))))   ;; => 28
```

| Form | Meaning |
|---|---|
| `(const NAME:T literal)` | a named literal. `T` is `i32`, `i64`, `f64`, `bool`, or `str`, and the value must be a literal of that type (`65536`, `-1i64`, `0.5`, `true`, `"text"`); no expressions. `NAME` is in capitals: at least two characters from `A-Z`, `0-9`, `_`, starting with a letter |
| `NAME` | anywhere an expression goes: the literal itself. It cannot also be the name of a variable, parameter, or field |
| `(enum Name [a b (c 10) ...])` | a new type `Name` with the named members. A member without a value is one more than the member before it, the first `0`. Members start with a lowercase letter or `_`; names and values must be unique; at least one member |
| `Name.member` | a value of type `Name` |
| `Name` as a type | `[k:Name]`, `-> Name`, struct fields, `(arr Name)`, `(result Name E)`, `(fn [Name] -> r)`, generic type arguments `(vec.Vec Name)` |
| `(enum.ord e)` | `i32`: the member's value |
| `(enum.cast Name n)` | `Name`: the `i32` `n` as a member, unchecked (like `ptr.cast`); for values read back from memory |

Enums are checked like pointers: a `Name` is never an `i32` or another enum, has no arithmetic, and compares only with `eq`/`neq`. Write `(eq k Kind.square)`, and convert explicitly with `enum.ord`/`enum.cast` where a number is meant. A bare type name that is not a scalar or an enum is `Unknown type 'Colr'`; a struct name written as a type (`p:Point`) is reported with the fix (`(ptr Point)`).

At run time a constant is its literal and an enum is an `i32` (4 bytes in structs and arrays), so neither changes the compiled code. Both are expanded after import resolution and generics: in Rust by `src/consts.rs` (constants become their literals and `Name.member` becomes `(enum.cast Name value)` for the checker), and in the self-hosted toolchain by `aipl_src/consts.aipl`, which erases both (types to `i32`, members to numbers) at the start of `codegen.compile_module`; the two produce identical bytes. Like generics, constants need the resolver: `Parser::parse` alone rejects a `(const ...)` item.

Imports qualify them like structs: `palette.LIMIT`, `palette.Color`, `palette.Color.red`, and through an alias `pal.Color.red`. Two modules may each define `Color`.

### J. Unions and `match`

A union is a type whose value is exactly one of several variants, each carrying its own fields: a value of `Shape` below is a circle with a radius, or a rectangle with a width and a height, or a dot with nothing. `match` takes a value apart, running the arm for its variant with the variant's fields bound to names. It works on enums too.

```lisp
(module geom
  (union Shape [(circle r:i32) (rect w:i32 h:i32) (dot)])
  (union List [(nil) (cons head:Shape tail:List)])     ;; may refer to itself
  (enum Dir [north east south west])

  (fn area [s:Shape] -> i32
    (match s
      (Shape.circle [r] (* 3 (* r r)))
      (Shape.rect [w h] (* w h))
      (Shape.dot 0)))

  (fn total [l:List] -> i32
    (match l
      (List.nil 0)
      (List.cons [s rest] (+ (call area s) (call total rest)))))

  (fn turns [d:Dir] -> i32
    (match d (Dir.north 0) (Dir.south 2) (else 1)))

  (fn main [] -> i32
    (let shapes:List (make List.cons (make Shape.rect 2 5)
                      (make List.cons (make Shape.circle 1)
                      (make List.cons (make Shape.dot) (make List.nil)))))
    (+ (call total shapes) (call turns Dir.south))))     ;; 10 + 3 + 0 + 2 => 15
```

| Form | Meaning |
|---|---|
| `(union Name [(variant field:T ...) ...])` | a new type `Name`. A variant without fields is `(variant)`. Variant names start with a lowercase letter or `_` and are unique; field names are unique within a variant; field types are those a struct field may have, including any union (so a union may refer to itself); at least one variant |
| `(make Name.variant v ...)` | a new value of that variant: one value per field, in declaration order, each of the field's type. Type `Name` |
| `(match v arm ... [(else body...)])` | `v` is a union or an enum. A union arm is `(Name.variant [x y ...] body...)`, binding the variant's fields in order (every field, each to a new name or to `_`, which skips it and may repeat; a variant without fields may omit the brackets); an enum arm is `(Name.member body...)` |
| `Name` as a type | everywhere a type goes: parameters, results, `let`, struct fields, variant fields, `(arr Name)`, generic type arguments |

Rules the checker enforces:

- **Exhaustive.** Every variant (member) has an arm, or there is an `(else ...)` arm, which comes last and binds nothing. An `else` arm when every variant already has one is an error (`the else arm never runs`), and so is matching one variant twice.
- **One type.** Every arm (and `else`) yields the same type, or all are `void`, as with `if`. That type is the match's.
- **Binders are new names**, scoped to their arm: they may not shadow a name in scope.
- **Union values are opaque.** They have no arithmetic and do not compare (`eq`/`neq` included): take them apart with `match`. There is no null union value, and no cast to one; `make` is the only way to build one.

A union value is the address of a heap cell: the variant's index (its position in the declaration, from 0) as an `i32` at offset 0, then the variant's fields laid out like a struct's starting at offset 4 (an `i64` or `f64` field is aligned to 8). Each `make` allocates the variant's exact size, rounded up to 8, and the cell is never changed afterwards. `make` claims the cell before evaluating its fields, in both backends. `match` reads the index and compares it with each arm's in order; a match on an enum compares the value. Only a value built with `(enum.cast Name n)` from a number that is no member can reach the end of a `match` without an `else`: the VM reports `unreachable` and compiled code traps.

`Parser::parse` alone knows the unions of the module it parses; across modules the resolver passes them along. Imports qualify unions like structs and enums: `geom.Shape`, `(make geom.Shape.dot)`, and in an arm `(geom.Shape.circle [r] ...)`, or through an alias `(g.Shape.circle [r] ...)`.

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

Every AIPL entry point runs the same stages in order. A failure at any stage aborts with a single diagnostic and a non-zero exit code; nothing downstream runs.

```
source.aipl
   │
   ▼
[1] Resolver   (src/resolver.rs)   reads the file and every (import ...), as S-expressions (src/sexpr.rs);
   │                               renames imported functions and structs to `mod.name`; one flat program
   ▼
[2] Generics   (src/generics.rs)   expands every generic instance into an ordinary struct or function
   │
   ▼
[3] Parser     (src/parser.rs)     each item -> typed AST (Module { name, imports, structs, functions });
   │                               every node keeps its (line, col)
   ▼
[4] Checker    (src/checker.rs)    static types + contract typing; no inference across functions
   │
   ├──────────────────────────────┐
   ▼                              ▼
[5a] VM        (src/vm.rs)       [5b] Wasm backend (src/compiler/wasm.rs)
     tree-walking interpreter         a core wasm module (wasm-encoder)
     runs contracts and               contracts and bounds checks are NOT emitted
     array bounds checks              │
                                      ▼
                                 [6] Native backend (aipl_src/native/, Linux x86-64, `compile --exe`)
                                      translates the wasm to an ELF executable
```

The self-hosted toolchain mirrors every stage in AIPL: resolving and generics (`resolver.aipl`, `generics.aipl`), constants (`expand.aipl`), parsing (`parser.aipl`, into the typed tree of `ast.aipl`), checking (`checker.aipl`), and code generation (`codegen.aipl`). `front.aipl` chains the checking stages; its parser and checker report the same first error as Rust's, word for word (section 6.4).

### 6.1 CLI surface (`src/main.rs`)

| Command | Stages run | Success output |
|---|---|---|
| `aipl verify FILE` | 1-4 | `[AIPL Verifier] OK: module 'NAME' type-checks. Contracts are type-checked, not proven; ...` (nothing is proved: `req`/`ens` run in the VM, section 3) |
| `aipl eval FILE [--func NAME] [-- ARGS...]` | 1-4, 5a | `[AIPL VM] Executing function 'main' from 'FILE'...` then `[AIPL Result]: Int(42)` (Rust `Debug` of the returned `Value`; default `--func main`). The program's argv is `FILE ARGS...` (`std/os`) |
| `aipl compile FILE [-o out.wasm]` | 1-4, 5b | `[AIPL Compiler] Successfully compiled 'FILE' -> 'out.wasm' (N bytes)` |
| `aipl compile --exe [--sandbox] [--target native\|wasm] FILE -o prog` | 1-4, 5b, then 6 for native | a standalone executable `prog`: native machine code on Linux x86-64 (section 6.6), else the launcher bundle (section 6.5) |
| `aipl run [--sandbox] prog.wasm [-- ARGS...]` | the `aipl-run` launcher | runs a compiled module natively (section 6.5); exits with its status |
| `aipl compile --self FILE [-o out.wasm]` | 1-4, 5b, then `resolver.resolve_file` and `codegen.compile_module` in the VM | compiles with the Rust toolchain and with the self-hosted one (AIPL resolver and AIPL codegen) and fails unless the bytes are identical (section 6.4) |
| `aipl test FILE [--func run_all]` | 1-4, 5a | `[AIPL Test] All groups passed.` and exit 0; otherwise `N group(s) failed.` and exit 1 |
| `aipl serve [--addr 127.0.0.1:8080]` | per request | an HTTP server for agents (`src/agent_api/server.rs`): `POST /eval` with JSON `{"source", "fn_name", "args"?}` runs a function in the VM (`args` are integers), `POST /verify` and `POST /compile` take the source as the request body; each answers JSON with `success` and `error`, plus `result` (eval) or `wasm_base64` (compile). Imports resolve against the standard library. Not covered by tests |

Errors are printed as `Error: "MESSAGE"`, the message in Rust debug quoting (inner quotes escaped). `eval` and `test` only invoke zero-argument functions. To exercise a function that takes parameters, wrap it in a zero-arg driver or write a Rust test (section 10.2).

### 6.2 What a compiled `.wasm` module looks like

`WasmCompiler::compile` produces a WebAssembly module with these sections, in this order: **type, import (only if the module does I/O), function, memory, export, code, data**, plus a table and element section when it uses function references. Allocation uses the threads proposal's `i32.atomic.rmw.add`, which wasmtime and every major browser accept on ordinary memory.

A **threaded module** (one that uses `thread.spawn`) differs: it imports its memory as shared (`"env" "memory"`, min 16, max 32768 pages) instead of defining it, adds a global (the address of this thread's runtime scratch cells, 64 in the main thread), a start function, a data-count section, and two compiler-generated functions after the user's: the start function, which copies the heap cursor and the string literals into memory once (guarded by an atomic flag at address 88, since every thread instantiates the module again), and the exported `wasi_thread_start(tid, record)`, which allocates the thread's 24-byte runtime scratch block, calls the worker through the function table, stores its result, and wakes `thread.join`. Its data segments are passive.

| Item | Value |
|---|---|
| Imports | only those used, in this order: from `wasi_snapshot_preview1` `fd_write`, `fd_read`, `path_open`, `fd_close`, `proc_exit`, `path_unlink_file`, `args_sizes_get`, `args_get`, `environ_sizes_get`, `environ_get`; from `wasi` `thread-spawn`; from `wasi_snapshot_preview1` `clock_time_get`, `random_get`. A threaded module also imports its memory (`"env" "memory"`, shared). Their types come first in the type section, and every user function index is offset by the import count. A module that does no I/O has no import section and instantiates with no imports. |
| Memory | one linear memory, min 16 pages (1 MiB, same as the VM), max 32768 pages (2 GiB), exported as `"memory"` |
| Data segments | one writing the heap start at address 0 (1024, or the first 8-aligned address after the string literals); if the module has string literals, a second at address 1024 holding every distinct literal as `[len u32 LE][bytes]` |
| String literal | `i32.const <address of its bytes>`; `str` values are pointers (section 4.B) |
| I/O scratch | functions that do I/O, `arr.new`, `atomic.cas/lock/unlock`, or `thread.spawn` get two extra `i32` locals; the WASI lowerings use runtime cells 64-87 for iovecs and out-parameters (section 7.9), or in a threaded module the same offsets in the current thread's scratch block |
| Heap cursor | the `i32` at address 0; `mem.alloc` compiles to `i32.const 0; <size>; i32.atomic.rmw.add` (the size is evaluated first, then one atomic add claims the block), followed (as for `new`, `arr.new`, and result cells) by a check that grows memory with `memory.grow` when the cursor passes `memory.size` (no locals used) |
| Loops | `block { loop { ... } }`; a `loop` (counted) also wraps its body in a block so `continue` falls into the step (section 7.10) |
| Store guard | every `mem.store*`, `put`, and `arr.set` is preceded by a 12-instruction check that traps (`unreachable`) if the address is in bytes 0-3 or between 64 and the heap start (the runtime block and the string literals); each function gets one extra `i32` scratch local for it, which `ok`/`err`, `match_result`, and `arr.new` also use |
| Function refs | only when the module uses `ref`/`call_ref`: one extra type per distinct `call_ref` signature after the function types, a funcref table (section id 4) of every function, and an element section (id 9) filling it; `call_ref` is `call_indirect` (section 4.G) |
| Results and arrays | `ok`/`err` allocate an 8-byte `[tag][payload]` cell; `match_result` tests the tag with `i32.eqz`; `arr.new` writes the count header and returns the address after it (sections 4.E, 4.F) |
| Exports | **every** function in the flat module, under its AIPL name (`add`, `compiler.tokenize`, ...), plus `memory`; `_start` when the module has a zero-argument `main` (section 6.5); `wasi_thread_start` in a threaded module |
| Function types | params map `i32/bool/str/void -> i32`, `f32 -> f32`, `f64 -> f64`; a `void` return is an empty result list |
| Locals | every `let` anywhere in the body (including nested in `if`/`while`/`loop`/`block`) plus every `loop` induction variable becomes one wasm local, allocated after the params |

Bytes 0..8 are always `00 61 73 6D 01 00 00 00` (`\0asm`, version 1). A module that compiles must also pass `wasmparser::Validator::validate_all`; the test suite enforces this.

### 6.3 Backend support matrix (as of 2026-10-05)

"Yes" means the op runs. The native backend (section 6.6) translates the wasm backend's output, so it supports exactly what that column supports. "Err" means the backend returns an explicit error naming the op; there are no silent defaults or no-ops in any backend. Every op the checker accepts compiles: `tests/test_opcode_conformance.rs` checks each one in value and statement position, validates the wasm, and compares the self-hosted compiler's bytes. The self-hosted column is `aipl_src/codegen.aipl` (section 6.4).

| Ops | Checker | VM | Rust wasm backend | Self-hosted |
|---|---|---|---|---|
| `+ - * / % divu remu ^ shl shr shru bitand bitor` on `i32` | Yes | Yes, wrapping | Yes | Yes |
| the same on `i64` | Yes | Yes, wrapping at 64 bits | Yes (`i64.*`) | Yes |
| `+ - * /`, comparisons on `f32` / `f64` | Yes | Yes | Yes (`f32.*` / `f64.*`); `%`, `divu`, `remu`, shifts, bitwise are Err | Yes (same rejections, compile error 99) |
| `eq neq lt lte gt gte` on `i32` / `bool` / `str`; `and or not` | Yes | Yes | Yes | Yes |
| `ltu lteu gtu gteu` on `i32` / `i64` | Yes | Yes | Yes (`i32.lt_u` ... `i64.ge_u`) | Yes |
| `checked.add checked.sub checked.mul` on `i32` / `i64` | Yes | Yes, error `Integer overflow in checked.add` | Yes, trap on overflow (section 8.2) | Yes |
| `i64` literals | Yes | Yes | Yes | Yes |
| `f64` literals | Yes | Yes | Yes | Yes when the digits form an integer ≤ 2^53 with ≤ 22 after the point (section 6.4); otherwise compile error 973 |
| `i64.extend_s i64.extend_u i32.wrap` | Yes | Yes | Yes | Yes |
| `f64.convert_i64_s i64.trunc_f64_s f64.reinterpret_i64 i64.reinterpret_f64` | Yes | Yes (`trunc` errors on NaN / out of range) | Yes (`trunc` traps) | Yes |
| `f64.sqrt` | Yes | Yes (Rust's correctly rounded `sqrt`) | Yes | Yes |
| `mem.load8/32/64`, `mem.store8/32/64` | Yes | Yes | Yes | Yes |
| `mem.alloc`, `mem.grow` | Yes | Yes | Yes | Yes |
| `atomic.add/cas/lock/unlock` | Yes | Yes, real across OS threads | Yes (wasm atomics; `lock` waits with `memory.atomic.wait32`) | Yes |
| `struct`, `new`, `get`, `put`, `sizeof` | Yes | Yes | Yes | Yes |
| `arr.new`, `arr.get`, `arr.set` | Yes | Yes, bounds-checked | Yes, **not** bounds-checked | Yes |
| `ok`, `err`, `match_result` | Yes | Yes | Yes (8-byte heap cell; 32-bit payloads only) | Yes |
| `sys.print` | Yes, `str` arguments only | Yes | Yes via WASI `fd_write` | Yes |
| `sys.exit` | Yes | returns the error `sys.exit(N) requested` | Yes via WASI `proc_exit` | Yes |
| `sys.time`, `sys.monotonic`, `sys.random` | Yes | Yes | Yes via WASI `clock_time_get` / `random_get` | Yes |
| `fs.open/read/write/close/delete` | Yes | Yes, real `std::fs` | Yes via WASI | Yes |
| `args.sizes/get`, `env.sizes/get` | Yes | Yes (host-set args, process environment) | Yes via WASI `args_*` / `environ_*` | Yes |
| `ref`, `call_ref`, `(fn [..] -> r)` types | Yes | Yes | Yes (funcref table, `call_indirect`) | Yes |
| `const`, `enum`, `enum.cast`, `enum.ord` | Yes | Yes | Yes (literals and `i32`; no code of their own) | Yes (erased by `consts.aipl`) |
| `union`, `make`, `match` | Yes | Yes | Yes (a tagged heap cell; `match` is an `if` chain ending in `unreachable`) | Yes |
| `return`, `break`, `continue`, `cond` | Yes | Yes | Yes (`return`, `br`; `cond` is nested `if`) | Yes |
| `thread.spawn / thread.join` | Yes | Yes, real `std::thread`; the worker is a `(fn [i32] -> i32)` reference | Yes, as a threaded module (section 4.D): needs a host that provides `wasi.thread-spawn` | Yes |
| `str` literals, `str.len`, `str.ptr` | Yes | Yes | Yes (interned data segment, pointer identity) | Yes |
| `(import ...)` | resolved before checking | | | resolved first by `resolver.aipl` (`driver.aipl` chains the two); `compile_module` itself takes one import-free module |

Rule of thumb for code generators: whatever `aipl verify` accepts runs in the VM and compiles. Compiled I/O needs a WASI host with a preopened directory (section 10.5), and threads need a wasi-threads host (AIPL's runner). Array bounds checks and contracts exist only in the VM.

### 6.4 The self-hosted backend (`aipl_src/codegen.aipl`)

`codegen.compile_module [src_ptr:i32 src_len:i32] -> i32` tokenizes and parses AIPL source (via `compiler.tokenize` / `compiler.parse_ast`) and emits a complete wasm module. It stores the output pointer in cell 60 and returns the byte length, or `-1` with a nonzero compile error code in cell 4 (the first error encountered; later ones are usually consequences). For everything it accepts, the output is required to be **byte-identical** to `WasmCompiler::compile`. `tests/test_selfhost.rs` enforces this on over 40 programs, including `memory.aipl`, `compiler.aipl`, and codegen.aipl itself (`compiler.aipl` merged in by hand, since `compile_module` does not resolve imports). `aipl compile --self` checks the same thing for any file and, on a mismatch, reports the first differing byte, the section (and code-section function) it falls in, and a hex window of each side.

It infers each expression's static type the way `expr_type` in `src/compiler/wasm.rs` does (`node_type` / `group_type`) and selects `i32.*` / `i64.*` / `f32.*` / `f64.*` instructions, `if` and `match_result` block types, struct field and array element load/store widths, and alignment from it. It runs in the VM today; compiled to wasm it also compiles itself (section 10.6). Limits that differ from the Rust backend:

- **One module, no imports.** `compile_module` itself takes a single import-free source. Imports are flattened first by `aipl_src/resolver.aipl` (section 11), which `aipl compile --self` runs in the VM; `aipl_src/driver.aipl` chains the two (`driver.compile_file`), and its `main`/`_start` make it a command: compiled to wasm, `wasmtime run --dir . --env AIPL_PATH aiplc.wasm IN.aipl OUT.wasm` compiles a multi-file program with no Rust involved (standard library at `$AIPL_STD`, else `aipl_src/std/`); `aipl compile --exe aipl_src/driver.aipl -o aiplc` makes the same command an executable (`./aiplc IN.aipl OUT.wasm`); exit status 0, 1 on a resolve, type, compile, or write error, 2 on bad usage. The driver type-checks before compiling (`front.aipl`), so it rejects every program `aipl verify` rejects, with the same message; it reports the error as `path: line:col: message` in the file the error is in, mapping the flat program back through the resolver's and the generics pass's re-printing (`origins.aipl`; an error inside a generic template is reported at the template). In the VM: `aipl eval aipl_src/driver.aipl -- IN.aipl OUT.wasm`. The byte-parity tests in `tests/test_selfhost.rs` still flatten with the Rust resolver and `src/printer.rs` (`tests/test_printer.rs` checks that round trip), which keeps them independent of the AIPL resolver. A qualified call such as `(call util.f)` resolves only if a function with that exact name is defined in the source given.
- **Float literals must be exact by construction.** `compile_module` computes an `f64` literal as `m / 10^k`, where `m` is the integer formed by all its digits and `k` is the number of digits after the point, using `f64.convert_i64_s` and one division. That equals Rust's correctly rounded `parse::<f64>` whenever `m ≤ 2^53` and `k ≤ 22`. Anything else (for example `9007199254740993.0`) is compile error 973 rather than a possibly different rounding. Both tokenizers read the same literals (a leading `+`, exponents); an exponent (`1.5e3`) is compile error 973 here, since `m / 10^k` cannot evaluate it exactly.
- Ops listed as compile error 987 in section 6.3 are not in its keyword table.

Buffers are sized from the input: tokens exactly (`compiler.count_tokens` first, `(sizeof compiler.Token)` each), AST `(sizeof compiler.Node) * (tokens + 2)`, and output, section scratch, and function scratch `4 * src_len + 64 KiB` each. Memory is grown with `mem.grow` as needed, so a compile works within the 32768-page limit shared by both backends. Compiling the whole self-hosted toolchain (driver, resolver, checker, codegen, and the standard library it uses), checking included, fits in about 31 MiB.

| Compile error (cell 4) | Meaning |
|---|---|
| 90 | `mem.grow` refused: the compile needs more than 32768 pages |
| 91 | output or a function body exceeded `4 * src_len + 64 KiB` |
| 92 | more than 2048 functions, or a function with more than 16 parameters |
| 93 | more than 1024 locals in one function |
| 94 | more than 255 structs or unions, more than 255 variants in a union, more than 64 fields in a struct or variant, or more than 31 distinct `call_ref` signatures |
| 95 | struct field or array element type is not a scalar (`i32 i64 f32 f64 bool str`) |
| 96 | unknown struct or field in `new`/`get`/`put`/`sizeof` |
| 97 | an `ok`/`err` payload that is not 32-bit |
| 98 | a type the backend cannot lower (anything but the scalars and `(result T E)`) |
| 99 | an operator applied to a type with no wasm instruction for it (e.g. `%` on `f64`, `+` on `str`) |
| 100 | more than 255 nested blocks, loops, and `if`s in one function |
| 101 | `break` or `continue` with no enclosing loop (the Rust checker rejects this first) |
| 768 | string literals exceed the self-hosted buffers (64 KiB of literal data or 1364 distinct literals), or the 1 MiB limit both backends share |
| 971 | a symbol that is not a local or parameter |
| 973 | a float literal outside the exact range above |
| 974 | `Name.member` naming a member its enum does not have (`consts.aipl`; the Rust resolver reports it first) |
| 975 | `make` or a `match` arm naming a variant its union does not have (the Rust checker reports it first) |
| 987 | a form whose head is not a recognised keyword |
| 999 | an empty expression where one is required |
| 1452 | call to an undefined function; cells 44/48 hold the callee name's source offset and length |

### 6.5 Standalone executables and the `aipl-run` launcher

A compiled module runs under `aipl-run` (`src/bin/aipl_run.rs`), the toolchain's native launcher: wasm cannot start itself, so a small Rust program embeds wasmtime, loads the module (compiled by Cranelift at start-up, about 10 ms for a small program), and connects it to the operating system. It contains no compiler logic. On Linux x86-64, `aipl compile --exe` writes native executables instead (section 6.6), which need no launcher.

- **Entry point.** A module with a zero-argument `main` and no `_start` of its own gets an exported `_start` that calls `main` and discards its result, so the module is a WASI command. The exit status is 0 when `_start` returns, or the value given to `sys.exit`, which must be 0-125: as under wasmtime, any other value (including a negative one) is an error reported with status 134, since shells reserve 126 and up. A trap prints one line to stderr, the program as invoked and the reason (`./prog: wasm trap: integer divide by zero`), and exits with 134; `AIPL_BACKTRACE=1` makes `aipl-run` print wasmtime's full report instead. Native executables print the same line. (`main`'s return value is not the exit status: many programs return data.)
- **What the program sees.** The real stdin, stdout, and stderr; the command line (argv[0] is the program); the environment; the working directory as preopened fd 3 and `/` as fd 4, so relative and absolute paths both work (section 4.C); and, for a threaded module, the wasi-threads `thread-spawn` import (section 4.D). `--sandbox` grants only the working directory.
- **`aipl compile --exe --target wasm FILE -o prog`** (the default on platforms without a native backend) writes the launcher followed by the module and a 16-byte trailer: flags (`u32` LE, bit 0 = sandbox), the module's length (`u32` LE), and the magic `AIPLEXE1`. The launcher finds a module appended to its own file and runs it; without one it runs a `.wasm` named on its command line (`aipl-run [--sandbox] prog.wasm ARGS...`). The result is one file with no other dependency, about 18 MB in a release build (the size is wasmtime), built for the host's OS and CPU. The `.wasm` remains the portable form.
- The launcher is found as `aipl-run` next to the `aipl` binary, or at `$AIPL_RUNNER`. Writing the bundle is done by the Rust CLI only because WASI cannot set a file's executable bit; the format is plain bytes.

### 6.6 Native executables (Linux x86-64)

`aipl compile --exe FILE -o prog` on Linux x86-64 (or `--target native`) writes a native executable: an ELF file of machine code, statically linked, with no libc, launcher, or runtime (`aipl_src/native/`, designed in `docs/NATIVE_BACKEND_PLAN.md`). It is made from the same wasm module (`aipl_src/native/native.aipl` translates wasm to x86-64; the CLI runs it in its embedded wasmtime and sets the file's execute bit), so its behaviour is the wasm module's, and every test program is checked to match `aipl-run`'s output, error output, exit status, and files byte for byte (`tests/test_native.rs`).

- **Size and speed.** `word_count` is about 31 KB; the AIPL compiler itself about 348 KB (the launcher build is 18 MB). Start-up is a few milliseconds (no engine to load or compile). The translator is a baseline one-pass compiler: long-running code is somewhat slower than Cranelift's (about 10% compiling the compiler).
- **Same behaviour as section 6.5:** `_start`/`main`, exit statuses (0-125, otherwise an error with 134), traps as one line with status 134 (`./prog: wasm trap: ...`; WASI functions given a bad pointer report wasmtime's `Pointer out of bounds: Region { start: S, len: L }`), the command line, environment, stdio, fd 3 = working directory, fd 4 = `/` (not with `--sandbox`), threads (one OS thread each), clocks, and randomness.
- **File access.** Paths resolve beneath their directory descriptor with `openat2(RESOLVE_BENEATH)`, so the kernel refuses `..` escapes, absolute paths under fd 3, and symlinks leading out, as wasmtime's preopens do. File descriptors are numbered as under wasmtime (the lowest free).
- **Platforms.** Linux x86-64 only; elsewhere `--exe` writes the launcher bundle (section 6.5), and `--target native` is an error. `--target wasm` asks for the bundle on Linux too.

---

## 7. Typing and Evaluation Rules by Example

The checker is `src/checker.rs`; the VM is `src/vm.rs`. Both agree on these rules.

### 7.1 Typing and Scoping Rules (P7 Specification)

The checker (`src/checker.rs`) enforces these six rules; the VM (`src/vm.rs`), the Rust wasm backend (`src/compiler/wasm.rs`), and the self-hosted compiler (`aipl_src/codegen.aipl`, which assumes checked input and emits the same bytes as the Rust backend) implement the same semantics:

1. `set!` has type void.
2. `if` whose two branches are both void is void; otherwise both branches must have the same non-void type — an if mixing void and non-void is a type error with a message suggesting `(block ... value)`.
3. `let` has type void (it declares, it does not yield); a function body's last expression must therefore be a value expression when the return type is non-void.
4. `let` is block-scoped: a `let` inside if/while/loop/block/match arms is visible only within that construct; shadowing an outer name is a type error. A name declared again in a sibling scope (another block, another `match` arm) keeps the type it had: a compiled function has one local per name, so `(block (let y:f64 1.5)) (block (let y:i32 3))` is `'y' is I32 here but F64 elsewhere in this function`.
5. `set!` on an undeclared name is a type error (and a VM runtime error); there are no implicit globals.
6. `match_result` binds its `ok` variable to the result's ok type and its `err` variable to the error type. `(ok v)` takes its ok type from `v` and defaults its error type to `i32`; `(ok:E v)` names the error type, and `(err:T e)` names the ok type (section 7.7).

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
| `NAME` (a constant) | the type of its literal | `PAGE_SIZE` |
| `Name.member` | the enum `Name` | `Kind.square` |
| `(enum.ord e)` / `(enum.cast Name n)` | `i32` / `Name` | `e` must be an enum, `n` an `i32` |
| `(make Name.v x ...)` | the union `Name` | one value per field of `v`, each of the field's type |
| `(match v arm ... [(else ...)])` | the type of the arms' last expressions, which must agree (or `void`) | `v` a union or enum; every member matched, or an `else` arm (section 4.J) |
| `(new S)` / `(arr.new T n)` | `(ptr S)` / `(arr T)` | |
| `(sizeof S)`, `(arr.len a)`, `(ptr.addr p)`, `(arr.addr a)` | `i32` | |
| `(get p S.f)` / `(arr.get T a i)` | the field's type / `T` | `p` must be `(ptr S)`, `a` must be `(arr T)`, `i` must be `i32` |
| `(put p S.f v)` / `(arr.set T a i v)` | `void` | as above; `v` must match the field / `T` |
| binary arithmetic / bitwise | type of the operands, which must be equal | `+ - * /` on numbers (`i32 i64 f32 f64`); `% divu remu ^ shl shr shru bitand bitor` on `i32`/`i64` only |
| comparisons | `bool`; operands must have equal type | `eq neq` on anything but a union; `lt lte gt gte` on numbers; `ltu lteu gtu gteu` on `i32`/`i64` only |
| `checked.add` / `checked.sub` / `checked.mul` | type of the operands, which must be equal | `i32` or `i64` only |

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
`(loop i 0 9 1 ...)` runs 10 times. `(loop i 0 0 1 ...)` runs once. `start`, `end`, and `step` are each evaluated **once**, in that order, before the first pass (like Python's `range`), so `(loop i 0 (- (call count r) 1) 1 ...)` calls `count` once. Changing a variable used in the bound inside the body does not change the bound. The body may still `set!` the loop variable itself. Leave a loop early with `break`, or skip to the next iteration with `continue` (section 7.10).

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

Linear memory is byte-addressed. Both backends start with 16 pages (1 MiB) and grow up to 32768 pages (2 GiB): automatically when an allocation needs it (section 4.A), or explicitly with `mem.grow`. Loads and stores are little-endian, unaligned access is allowed, and out-of-bounds access is a VM runtime error (`Memory store out of bounds: ptr N`) and a wasm trap. Get memory from `mem.alloc`; never pick an address yourself (section 7.9).

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
| 4 | i32 | codegen | the first compile error's code (`0` = none; section 6.4), read by the host |
| 8..12 | | unused | |
| 16 | i32 | codegen | pointer to the keyword map, built once by `codegen_init` and reused by later compiles |
| 20..40 | | unused | (the rest of a compile's state is in the `Cg` struct `compile_module` creates) |
| 44, 48 | i32, i32 | codegen | source offset and length of the callee name, after compile error 1452, read by the host |
| 52, 56 | | unused | |
| 60 | i32 | codegen | pointer to the module `compile_module` emitted, read by the host |
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
- **Codegen state is per compile.** `compile_module` keeps its tables in a fresh `Cg`; `codegen_init` builds the keyword map only when cell 16 is zero and always clears cell 4.


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
| `(f64.sqrt x)` | `f64 -> f64`, the correctly rounded square root (IEEE 754); NaN for a negative `x`, `(f64.sqrt -0.0)` is `-0.0` | `f64.sqrt` |
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

### 8.2 Unsigned comparisons and checked arithmetic

The signed comparisons (`lt lte gt gte`) read their operands as two's-complement numbers. `ltu lteu gtu gteu` compare the same bits as unsigned numbers (wasm `i32.lt_u` ... `i64.ge_u`), so `-1` is the largest value: the right test for sizes, hashes, and addresses that may exceed `2^31`.

`+ - *` wrap. `checked.add`, `checked.sub`, and `checked.mul` compute the same result when it fits in the operands' type and stop the program when it does not: the VM fails with `Integer overflow in checked.add` (`.sub`, `.mul`), and compiled code traps with the host's integer-overflow trap. Use them where a wrapped result would be a wrong answer rather than an intended one (money, sizes, counters).

| Expression | Result |
|---|---|
| `(ltu 1 -1)` | `true` (`-1` is `4294967295` unsigned) |
| `(gtu -2147483648 2147483647)` | `true` |
| `(lteu -1i64 0i64)` | `false` |
| `(checked.add 2147483646 1)` | `2147483647` |
| `(checked.add 2147483647 1)` | runtime error / trap |
| `(checked.sub -2147483648 1)` | runtime error / trap |
| `(checked.mul 46341 46341)` | runtime error / trap (`46340` squared fits) |
| `(checked.mul -1i64 -9223372036854775808i64)` | runtime error / trap |

The wasm lowering (`compile_checked` in `src/compiler/wasm.rs`, `codegen.aipl` alike): for `i32`, both operands are widened to `i64`, the exact result is computed there, and it traps unless it lies in `[-2^31, 2^31)`; for `i64`, addition and subtraction test the sign bits (`(a ^ r) & (b ^ r) < 0` for `+`, `(a ^ b) & (a ^ r) < 0` for `-`) and multiplication checks `r / a == b` when `a` is not 0 (and the `-1 * MIN` case through the same division). The trap is an `i32.div_s` of `MIN` by `-1`, so every host reports it as integer overflow. `tests/test_differential.rs` runs every pair of 12 edge values per width through the VM and wasmtime and compares both with Rust's `checked_*`.

### 8.3 Type-directed code generation

The wasm backend selects instructions from the static operand type (`i32` / `i64` / `f32` / `f64`), and an `if` whose branches are `i64` or `f64` gets a matching block result type. `f64` arithmetic (`+ - * /`) and comparisons therefore compile and validate. `%`, `divu`, `remu`, shifts, and bitwise ops on floats are rejected at compile time with `Wasm Codegen: <op> is not supported for operands of type F64`.

**Status:** the wrapping rules above for both widths are implemented in `src/vm.rs`, covered by `tests/test_i64.rs` (VM result plus wasm validation), and proven equal across backends by `tests/test_differential.rs`, which executes every case in this section in both the VM and wasmtime and asserts identical results. When the two disagree, wasm is right and the VM is fixed (section 10.4).

---

## 9. Diagnostics

Every parser and checker error is a single line of the form

```
<line>:<col>: <message>
```

with 1-based line and column of the offending token or the opening `(` of the offending form. Syntax errors, which the resolver finds while reading a file, are prefixed with that file's path (`examples/x.aipl: 3:5: Unknown op/keyword: badop`); type errors are not, so a type error in an imported module gives a position but not the file. Only the first error is reported. Contract failures carry the contract's position (section 7). Other VM runtime errors (division by zero, out-of-bounds memory, unknown thread handle) currently have **no** position.

Representative messages, exactly as produced:

| Situation | Message |
|---|---|
| missing `)` at end of file | `1:1: this bracket is never closed` (the position of the unclosed bracket) |
| extra `)` after the module | `3:1: unexpected input after the module's closing ')'` |
| `if` with no else branch | `1:37: Unexpected token parsing expression: RParen` (the `)` where the else was expected) |
| unknown operator | `3:5: Unknown op/keyword: badop` |
| unterminated string | `4:12: Unterminated string literal` |
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
| arithmetic on two pointers, `(+ p p)` | `3:5: Add on Ptr(Struct("Point")): pointers, arrays, and function refs have no arithmetic; use get/put or arr.get/arr.set, or convert with ptr.addr/arr.addr and ptr.cast/arr.cast` |
| a pointer plus a number, `(+ p 4)` | the same message as two pointers |
| arithmetic on an enum, `(+ k 1)` | `3:5: Add on enum 'Kind': enums have no arithmetic; compare them with eq/neq, or convert with (enum.ord x) and (enum.cast Kind n)` |
| two different enums compared | `3:5: Type mismatch in comparison: Enum("C") vs Enum("D")` |
| an enum member that does not exist | `m.aipl: 3:5: enum 'Kind' has no member 'circle'` |
| a struct written as a type, `p:Point` | `1:30: 'Point' is a struct, which is only used through a pointer: write (ptr Point)` |
| a constant reused as a variable | `m.aipl: 3:10: 'MAX' is a constant; it cannot also name a variable, parameter, or field` |
| a constant whose value does not match its type | `m.aipl: 1:25: constant 'NN' is declared i32 but its value is not an i32 literal` |
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
| an op that does not exist | `3:5: there is no mem.free: memory is never freed. For memory used in phases, allocate from a region and reset it (std/arena)` |
| non-`str` `sys.print` | `3:5: sys.print prints str values, got I32; for numbers use io.print_int / io.print_i64 / io.print_f64 (import io)` |
| `(+ str str)` | `3:5: + does not join strings; build them with std/buf (buf.push_str, buf.bytes)` |
| arithmetic or ordering on something that is not a number | `3:5: Add needs numbers (i32, i64, f32, f64), got Bool` / `Mod is integer arithmetic (i32 or i64), got F64` / `Lt on Str: only numbers are ordered; bool and str compare only with eq/neq` |
| wrong operand count or type for an op | `3:5: atomic.cas takes 3 operands, (atomic.cas p expected new); got 2` / `atomic.add operand 2 must be I32, got I64` |
| non-`bool` contract | `1:50: Contract expression in 'safe_div' must evaluate to Bool, got I32` |
| store or lock at literal address 0 | `3:5: AtomicLock at address 0: bytes 0-3 are the heap cursor owned by mem.alloc; locking it hangs and storing to it corrupts the allocator. Take memory from (mem.alloc n) instead` |
| literal address in 64-1023 | `3:5: MemStore32 at literal address 512: bytes 64-1023 are the reserved runtime block. Take memory from (mem.alloc n) instead` |
| locking a word that is not 0/1 (runtime, VM) | `atomic.lock: word at ptr 1024 holds 1024, which is not a lock state (0 = free, 1 = held); this address is data, not a mutex. Allocate a dedicated lock word with (mem.alloc 4)` |

Type names in messages are the Rust `Debug` spelling, not AIPL syntax: `I32`, `F64`, `Bool`, `Str`, `Void`, `ResultType(I32, I32)`, `Ptr(Struct("Point"))`, `Array(I32)`, `Fn([I32], I32)`. A few messages also give the AIPL spelling of the expected type (`needs a (ptr Point)`).

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

Expected terminal output (22 groups; abridged):
```
[AIPL Test] Running 'run_all' from 'aipl_src/test_suite.aipl'...

[PASS] compiler: tokenizer (2 tests)
[PASS] compiler: parser (2 tests)
[PASS] codegen: signatures + 3 real wasm modules (4 tests)
...
[PASS] std/bigint: arbitrary-precision integers (13 tests)
[PASS] consts: constants and enums erased for codegen (4 tests)
[PASS] thread_sync: 4 threads x 1000 atomic adds = 4000

[AIPL Test] All groups passed.
```

`aipl test` runs the suite in the VM; it also compiles to wasm.

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
let module = Parser::parse("(module m (fn f [] -> void (sys.print 5)))").unwrap();
let err = WasmCompiler::compile(&module).unwrap_err();
assert!(err.contains("sys.print supports str arguments only in the wasm backend"));
```

Files today (25 files, 249 tests as of 2026-10-04): `tests/test_all.rs` (pipeline smoke), `tests/test_v2.rs` (memory, atomics across real threads, real file I/O, results, imports), `tests/test_diagnostics.rs` (exact `L:C:` prefixes), `tests/test_i64.rs` (64-bit type, VM plus wasm validation), `tests/test_memory_layout.rs` (reserved-block enforcement in both backends), `tests/test_opcode_conformance.rs` (10.3), `tests/test_differential.rs` (10.4), `tests/test_wasi.rs` (10.5), `tests/test_selfhost.rs` (10.6), `tests/test_doc_examples.rs` (10.7), `tests/test_pointers.rs` (strict pointer/array typing, VM/wasm agreement, struct namespacing across imports), `tests/test_std.rs` (every eligible standard-library function in both backends under WASI, plus exact printed output), `tests/test_printer.rs` (source round trip of every repository program), `tests/test_refs.rs` (function references in both backends, signature checks, refs across imports), `tests/test_control_flow.rs` (return/break/continue/cond, short-circuit `and`/`or`, loop bounds evaluated once, in both backends; checker rejections), `tests/test_threads.rs` (threaded modules under a wasi-threads host against the VM), `tests/test_generics.rs` (template expansion in both backends, Rust/AIPL parity, errors), `tests/test_resolver_aipl.rs` (the AIPL resolver against the Rust one, subdirectory imports), `tests/test_runner.rs` (the `aipl-run` launcher and `aipl compile --exe` executables), `tests/test_benchmarks.rs` (every program in `benchmarks/`, as wasm and natively, against its `expected.txt`), `tests/test_consts_enums.rs` (constants and enums in the VM, wasm, and natively, and every checker rule), and the native backend's `tests/test_native_reader.rs` (the wasm reader against wasmparser), `tests/test_native_x64.rs` (the encoder against GNU as), `tests/test_native_elf.rs` (runs the first native executable), and `tests/test_native.rs` (the native backend's harness: every program built as wasm and natively, run under `aipl-run` and directly, with identical output, error output, exit status, and files; the AIPL compiler built natively and reproducing itself). `tests/test_differential.rs` also checks that allocation grows memory to the same page count in both backends.

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

To run compiled I/O outside the tests: `aipl run module.wasm` (the working directory is preopened), or build an executable with `aipl compile --exe`.

### 10.6 Self-hosted byte parity (`tests/test_selfhost.rs`)

`self_host(src)` runs `codegen.compile_module` in a fresh VM, validates the output with `wasmparser`, and returns the bytes or the compile error code. `assert_self_hosted_matches_rust` asserts the whole module equals `WasmCompiler::compile` for the same source and prints the first differing function and byte if not. Coverage: minimal, `add`, `i64` (literals at both extremes, unsigned ops, an `i64` struct field after a `bool`, `i64` arrays, `mem.load64`/`store64`), `f32`/`f64` parameters, arithmetic and struct fields, 200 pseudo-random float literals plus out-of-range ones (973), `(ok:T v)` with compound `T`, an escaped quote inside a string, `compute` (loop + call), structs with `bool`/`str` fields, arrays, `sys.print`, results (including `match_result` as a statement), file I/O, repeated string literals with a `str` let plus `mem.alloc`/`mem.grow`/`sys.exit`, a mixed program, void `if` with block-scoped `let`s and an empty `(block)` else, a store, arrays plus results, `aipl_src/memory.aipl`, `aipl_src/compiler.aipl`, and codegen.aipl compiling itself (about 80 s in a debug build). Behavioural checks run the self-hosted output in wasmtime: the store guard traps on bytes 0-3 and 64-1023 and nowhere else in a module without literals, and also on the literals in a module with 800 bytes of them, whose heap then starts at 1832 in both the VM and wasm; arrays and results compute the same values as the VM and a negative `arr.new` traps; a file write plus `sys.print` under WASI produces the file and the exact stdout. Compile errors 95, 96, and 768 are asserted for non-scalar fields/elements, unknown structs, and a 70 000-byte literal (over the self-hosted 64 KiB buffer).

**Bootstrap fixpoint.** `self_hosted_compiler_reproduces_itself_under_wasmtime` compiles the self-hosted compiler with the Rust backend (stage 1), runs that wasm module under wasmtime on its own source, and requires the output (stage 2) to be byte-identical to stage 1. The second compile involves neither the VM nor any Rust compiler code, and takes about 20 ms. Since P9, codegen.aipl itself uses `i64` and `f64` arithmetic (literal parsing, LEB128, float literal bits), so the fixpoint also covers those paths of the self-hosted compiler.

### 10.7 Documentation examples (`tests/test_doc_examples.rs`)

Every ```` ```lisp ```` block in `PROMPT_GUIDE_FOR_AIS.md`, `README.md`, and this specification is checked. A complete module must parse, check, and compile, and if it defines `main`, `main` must return the value listed in the test's `EXPECTED` table in both the VM and wasmtime (with WASI and a scratch directory); the few spec modules that cannot run that way are listed with a reason in `SPEC_SPECIAL`. A fragment of this specification (a few definitions without `(module`) is wrapped in a module and type-checked; the deliberately invalid example in 7.3 must fail with its documented message. Adding an example with a `main` means adding its expected value there. This test found that a `match_result` used as a statement compiled to invalid wasm.

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
- Constants and enums follow the struct rule: `util.LIMIT`, `util.Color`, `util.Color.red` outside `util` (and `u.Color.red` through an alias). The head of a form is never renamed, so `(import util as mem)` leaves the op `(mem.alloc n)` alone.

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
`aipl eval hello.aipl` prints `hello, aipl` then `[AIPL Result]: Int(11)`. `aipl compile hello.aipl` produces a module importing `wasi_snapshot_preview1::fd_write`; run it with `aipl run hello.wasm`, or any WASI host, and it prints the same line. String concatenation `(+ "a" "b")` is the one string feature still VM-only: the wasm backend rejects it.

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

Paths are `(ptr, len)` pairs into linear memory, matching the WASI convention. `fs.open` flags: `0` read-only, non-zero write/create/truncate. All `fs.*` return `-1` on failure rather than raising. In the VM the path is relative to the process cwd; under WASI it is relative to the preopened directory (fd 3), which `aipl run` and executables built with `aipl compile --exe` set to the working directory.

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
| `fmt` | `uint_to_bytes [n out] -> i32` (n read as unsigned), `int_to_bytes` (leading `-`), `hex_to_bytes` (lowercase, no prefix), and for `i64`: `uint64_to_bytes`, `int64_to_bytes`: each writes ASCII at `out` and returns the count (at most 10, 11, 8, 20, and 21 bytes); `f64_fixed [x digits out]` writes `x` with `digits` decimals exactly as C's `printf("%.*f")` (the exact decimal value of the double, rounded half to even; `inf`, `nan`, a `-` for any negative sign bit; at most 312 + digits bytes) |
| `io` | `read_stdin [] -> (ptr str.Bytes)` (all of stdin), `write_str [fd s]`, `println [s]`, `eprintln [s]` (stderr), `print_int [n]`, `println_int [label n]` (prints `label`, then `n`, then a newline), `print_i64 [n:i64]`, `println_i64 [label n:i64]`, `print_f64 [x digits]`, `println_f64 [label x digits]`, `read_file [path:str] -> (ptr str.Bytes)` (whole file; `len` -1 on failure), `read_fd [fd] -> (ptr str.Bytes)` (reads an open descriptor to its end), `write_file [path:str b:(ptr str.Bytes)] -> i32` (bytes written or -1), and `read_path` / `write_path`, the same for a path held as `(ptr str.Bytes)` |
| `vec` | generic growable list `(vec.Vec T)`: `(call (vec.make T) capacity)`, `push`, `pop`, `at [v i]`, `set [v i x]`, `len`, `clear`, `index_of`, `sort_by [v cmp:(fn [T T] -> i32)]` (stable merge sort; `cmp` negative puts the first argument first), each called as `(call (vec.push T) v x)`; plus `sort_i32 [v:(ptr (vec.Vec i32))]` and `cmp_i32` |
| `map` | generic hash map `(map.Map V)` from `i32` keys: `(call (map.make V) capacity)`, `set [m k v]`, `get_or [m k default]`, `has`, `remove -> bool`, `count`; iterate with `(loop i 0 (- (call (map.capacity V) m) 1) 1 (if (call (map.slot_used V) m i) ... (block)))` reading `slot_key` / `slot_val` |
| `strmap` | generic hash map `(strmap.StrMap V)` from byte strings (symbol tables, word counts): the same API as `map` with keys of type `(ptr str.Bytes)`; the map keeps the key pointer, so a key's bytes must not change while it is stored |
| `os` | the command line and environment: `arg_count [] -> i32` (argv[0], the program, included), `arg [i] -> (ptr str.Bytes)` (`len` -1 past the end), `env [name:str] -> (ptr str.Bytes)` (`len` -1 if unset), each fetching a fresh copy; `random_i32 [] -> i32` from the OS generator |
| `buf` | string builder: `make [capacity] -> (ptr buf.Buf)`, `push_byte`, `push_str [b s:str]`, `push_bytes [b (ptr str.Bytes)]`, `push_int`, `push_i64`, `push_f64 [b x digits]`, `len`, `clear`, `bytes [b] -> (ptr str.Bytes)` (a view of the contents; take it after building) |

| `arena` | a region allocator (no general `free` exists): `make [chunk_size] -> (ptr arena.Arena)`, `(call (arena.alloc T) a) -> (ptr T)` (zeroed, like `new`, no cast), `raw [a n] -> i32` (n zeroed bytes, 8-aligned), `reset [a]` (frees everything from `a`; its chunks are reused), `reserved [a]`; never fails while memory remains (a full chunk moves on to another) |
| `time` | timing code with the monotonic clock: `now [] -> i64` (nanoseconds), `since [start] -> i64`, `push_duration [b ns]` ("850 ns", "12.345 us", "3.071 ms", "4.200 s": three decimals in the largest fitting unit, integer arithmetic), `report [label start]` ("label: 12.345 ms" on stderr, so a program's output stays clean) |
| `bigint` | arbitrarily large signed integers, changed in place (reuse numbers rather than making new ones; nothing is freed): `(ptr bigint.Int)` from `from_i32 [v]`, `with_capacity [limbs]`; `set_i32 [a v]`, `assign [dst src]`, `add [a b]`, `sub [a b]`, `add_mul_small [a b m]` (a += b·m), `sub_mul_small [a b m]`, `mul_small [a m]` (\|m\| < 2^31), `div_small [a d] -> i32` (a /= d toward zero, returns the remainder, 0 < d < 2^31), `div_small_quotient [a b] -> i32` (a ≥ 0, b > 0, quotient < 2^31: returns a / b, leaves the remainder in a), `negate [a]`, `compare [a b] -> i32`, `sign [a] -> i32`, `is_zero [a]`, `limb_count [a]`, `push_decimal [out a]` |

Containers are generic (section 4.H): a list of points is `(ptr (vec.Vec (ptr Point)))`, filled with `(call (vec.push (ptr Point)) v p)` and read with `(call (vec.at (ptr Point)) v i)`, with no casts.

From outside, the slice type is `str.Bytes`: `(ptr str.Bytes)`, `(get b str.Bytes.len)`. Every module ends in a `run_<module>_tests` runner wired into `aipl_src/test_suite.aipl`. Allocation grows memory as needed (section 4.A), but nothing is freed (there is no `mem.free`): `print_int` allocates 11 bytes per call, `read_file` a buffer per file, and growing a `vec`, `map`, or `buf` abandons the old storage. Long-running programs should reuse containers (`clear`) rather than make new ones.

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
$ aipl compile --exe examples/word_count.aipl -o word_count
$ cd examples && ../word_count
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
| `(+ str str)` | there is no string `+`; build strings with `std/buf`. Threads and atomics compile; a program using `thread.spawn` needs AIPL's runner (or another wasi-threads host) to run |
| `(get p x)` or `(get p Point x)` | the field is one symbol: `(get p Point.x)`; arrays name the element type every time: `(arr.get i32 a i)` |
| relying on `arr.get` to catch a bad index in compiled code | only the VM bounds-checks; check `(lt i (arr.len a))` yourself where it matters |
| `(ok 1i64)` or an `f64` payload in code meant for `aipl compile` | result payloads must be 32-bit in wasm; return an `i32` pointer to a struct instead |
| ending a function in `(let ...)` | `let` is void; end with the value, e.g. the variable name |
| `(sys.print n)` with an `i32` in code meant for `aipl compile` | the wasm backend prints `str` only; use `(call io.print_int n)` or `(call io.println_int "label " n)` from the standard library |
| hand-writing digit formatting, file-reading loops, byte counting, growable arrays, hash tables, or string building | the standard library (section 12.6): `io`, `str`, `fmt`, `vec`, `map`, `strmap`, `buf`, `os` |
| `(call vec.push v x)` on a generic container | name the element type: `(call (vec.push i32) v x)`; the container's type is `(ptr (vec.Vec i32))` (section 4.H) |
| `(get b Box.value)` on a generic struct | name the instance: `(get b (Box i32) value)` |
| calling `mem.grow` before allocating | not needed: allocation grows memory itself (up to 32768 pages, 2 GiB) |
| passing a `str` literal where a `(ptr, len)` path or buffer is expected, e.g. `(fs.open "t.bin" 5 0)` | type error: `fs.*` take `i32` pointers. Write `(fs.open (str.ptr "t.bin") (str.len "t.bin") 0)` |
| `(if (lt i 0) (return -1) i)` | `return` is a statement (void): `(if (lt i 0) (return -1) (block))`, then the value |
| `(cond ((lt n 0) -1) ((eq n 0) 0))` without `else` | `cond` needs a final `(else ...)` clause; use `(else (block))` when the clauses are statements |
| `else if`, `elif`, `switch`, `case` | do not exist; use `cond` |
| a zero-argument function or a bare number standing for a fixed value | `(const PAGE_SIZE:i32 65536)`; for a set of related codes, `(enum Kind [a b c])` (section 4.I) |
| `(+ k 1)`, `(lt k Kind.b)`, or passing `Kind.b` where an `i32` is expected | enums are not numbers: compare with `eq`/`neq`; convert with `(enum.ord k)` and `(enum.cast Kind n)` |
| `(const max:i32 5)` or `(let MAX:i32 1)` after `(const MAX ...)` | constants are named in capitals and cannot double as variables |
| `p:Point` for a struct | structs are always behind a pointer: `p:(ptr Point)`; a bare type name must be a scalar, an enum, or a union |
| a struct with a `kind` field and fields that mean different things per kind | a union: `(union Shape [(circle r:f64) (rect w:f64 h:f64)])`, built with `make`, taken apart with `match` (section 4.J) |
| `(match s (circle [r] ...))`, `(match s (Shape.circle r ...))` | arms name `Union.variant` in full, and binders go in brackets: `(Shape.circle [r] ...)` |
| a `match` arm per variant plus an `else` "just in case" | the checker rejects an `else` that can never run; leave it out when every variant has an arm |
| `(eq s1 s2)` or `(ptr.null Shape)` on a union | union values do not compare and are never null: use `match`; for "maybe a value", add a variant such as `(none)` |
| the same name declared with two types in one function (`(let x:f64 ...)` in one block, `(let x:i32 ...)` in another, or as binders of two arms) | a name keeps one type per function; rename one, or bind a field you do not read as `_` |
| `(lt size limit)` on sizes or hashes that may pass `2^31` | `(ltu size limit)`: compares as unsigned |
| `(+ balance amount)` where wrapping would be a silent wrong answer | `(checked.add balance amount)`: stops the program on overflow |
| `(break)` in a `while` condition or outside any loop | only inside a `while` or `loop` body |
| a `done`/`found` flag variable to stop a loop | `(break)`, or `(return v)` from the function |
| an extra `)` after the closing `(module` paren | reported as `L:C: unexpected input after the module's closing ')'` |
| `inf`, `nan`, `1e9` as literals | not literals; `1e9` is a symbol and will be reported as an undefined variable |
| `(req n > 0)` | contracts are S-expressions: `(req (gt n 0))` |
| `(ens (gt result 0))` | the result is named `res`, always |
| declaring a test "done" because it returned a number | assert the value; see section 10 |
