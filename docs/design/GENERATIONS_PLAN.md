# Generation checks: stopping use after free

Status: done (2026-10-16) in every backend and both toolchains: the VM,
`src/compiler/wasm.rs`, `aipl_src/codegen.aipl` (byte for byte), the
native backend (which needed nothing: it translates the wasm), and the
standard library (`std/gen`). tests/test_generations.rs checks it in all
three backends. Parallel work without `unsafe` (`std/par`, "Data races"
below) is done too; the pre-checked small functions under "What remains"
were measured and not built.

## The gap

Code without `unsafe` cannot reach memory through a raw address
(AIPL_SPEC.md 3, "Unchecked operations"). One way to reach memory it should
not remains: keep a pointer, free its object, and use the pointer again.

```lisp
(let p:(ptr Point) (call (heap.create Point) h))
(call (heap.destroy Point) h p)
(get p Point.x)                     ;; reads freed memory: today, the junk pattern
(let q:(ptr Point) (call (heap.create Point) h))
(get p Point.x)                     ;; p and q are now the same block: reads q's data
```

`std/heap` already catches a double free and a write after free (when the
block is handed out again), and a read of freed memory gets the junk
pattern, not old data. But once the block is reused, the old pointer reads
and writes the new object, and nothing notices. That is worse than reading
stale data: writing an integer where the new object keeps a pointer forges
a pointer, so code without `unsafe` reaches any address in the program's
memory. `std/arena` is the same with no checks at all, for every pointer
into an arena after `reset`:

```lisp
(let old:(ptr Num) (call (arena.alloc Num) a))
(call arena.reset a)
(let b:(ptr Box) (call (arena.alloc Box) a))   ;; the same memory, now a Box
(put old Num.n 2048)                            ;; writes Box.p: a pointer to address 2048
(get (get b Box.p) Num.n)                       ;; reads whatever is at 2048
```

So generations must cover arenas too: `reset` bumps the generation of
every block it hands out again (or the arena's memory carries generation
words like the heap's).

## The idea

Every object carries a **generation**, a counter in its header. Every
pointer carries the generation its object had when the pointer was made.
Freeing an object adds one to its generation. Every `get`, `put`,
`arr.get`, `arr.set` and `arr.len` compares the two, next to the null check
it already does; a mismatch stops the program with "Use after free",
naming the function and position like every other runtime error.

In the example, `p` carries generation 0; the destroy makes the block's
generation 1; both reads of `p` then stop the program, whether or not the
block was reused.

This is the scheme of Vale's "generational references" and of hardware
memory tagging (Arm MTE), done in software with a full 32-bit counter
instead of a 4-bit tag, so a stale pointer is caught every time, not 15
times in 16.

## Design

**Pointers become 8 bytes.** A `(ptr S)` or `(arr T)` value is an `i64`:
the address in the low half, the generation in the high half. Struct
fields, array elements and locals of pointer type double in size. This is
the largest change: every layout (`sizeof`, field offsets, `(arr (ptr S))`)
changes, in both toolchains, and the spec's layout sections with them.

**Every object has a generation word.** Directly before the object:

- `std/heap` blocks already have an 8-byte header (heap and state; size).
  The generation takes a third word, or replaces the state half of word 0
  (an even generation is live, odd is freed), which keeps the header at 8
  bytes. The prototype decides which.
- Objects from `new`, `arr.new` and `make` are never freed; they get a
  4-byte word holding 0, so the check does not need a special case.
- An array already has its length before its elements; the generation goes
  before the length.

**Freeing bumps the generation; reusing keeps it.** A freed block keeps its
new generation while it waits on the free list and when it is handed out
again, so the new object's pointers carry the new number and every older
pointer is stale. A block whose generation would wrap around is retired
(never reused) instead: at most once per 2^31 reuses of one block.

**Unchecked operations stay unchecked.** `ptr.addr` drops the generation
(it gives an `i32` address). `ptr.cast` inside `unsafe` reads the current
generation from the header, so a cast pointer is valid until the next free.
`mem.*` never checks.

**Null stays null.** The null pointer is address 0, generation 0; the null
check runs first, so its message is unchanged.

## Cost

One extra load and compare on each pointer access, and pointers twice as
large: more memory, more cache misses on pointer-heavy data. The null
checks cost 2 to 8% natively (docs/BENCHMARKS.md); this is likely several
times that on binarytrees, which is almost all pointer accesses, and close
to nothing on nbody and spectralnorm, which are arithmetic on arrays of
floats. The prototype measures it before anything is decided.

Known ways to cut it later: skip the check when the same pointer was
checked earlier in the function and nothing in between can free; keep the
generation word and the first field in one cache line.

## Stages

1. **Prototype in the Rust toolchain** (VM and `src/compiler/wasm.rs`) on a
   branch: 8-byte pointers, generation words, the check, `std/heap`
   bumping on free. Run the eight benchmarks. If the cost is out of line
   with the other checks, stop and revisit.
2. **The AIPL toolchain** (`codegen.aipl`, `checker.aipl` layouts, the
   native backend's lowering), byte for byte as always, with
   `test_selfhost` and `test_checker_aipl` as the gate.
3. **Tests:** use after free, after reuse, through an array, in a struct
   field, across a thread join, after an arena's reset (the forged pointer
   above); mutation checks on the comparison and on
   the bump; `run_fuzz.py` generating frees.
4. **Claims:** with visibility done (2026-10-14, the other gap),
   code without `unsafe` is memory-safe except for data races between
   threads, which already require `unsafe`. Only then do the README, FAQ
   and spec say "memory-safe".

## Alternatives considered

- **A borrow checker** (Rust). Proves the absence of use after free at
  compile time with no run-time cost, but it is the hardest part of Rust
  for people and for code generators to get right, and it would reshape
  every program and the whole standard library.
- **Garbage collection.** Removes freeing altogether, but AIPL's model is
  explicit allocators (Zig's); a collector changes what programs mean and
  adds pauses.
- **Never reusing freed memory** (quarantine). Catches the reuse case
  cheaply, but memory only grows, and reads of freed blocks still go
  unnoticed.

## Prototype (2026-10-16)

Built in `src/compiler/wasm.rs`, the Rust checker's layouts, and the
standard library, enough to run every benchmark as wasm and natively (the
native backend, compiled by the prototype, runs with the checks on too).
The VM and the AIPL toolchain are unchanged, so their tests fail on the
branch.

**What it does.**

- A `(ptr S)` or `(arr T)` is an `i64`: the address in the low half, the
  generation in the high half; 8 bytes, 8-aligned, in memory.
- Every object has a generation word 8 bytes before it. `new` and
  `arr.new` reserve it (the bump allocator never frees, so it stays 0);
  for an array the length follows it, as before, just before the first
  element.
- `get`, `put`, `arr.get`, `arr.set`, and `arr.len` check, after the null
  check, that the word equals the pointer's generation, else "Use after
  free" with the usual position and call chain. Same in wasm and natively.
- `ptr.cast` / `arr.cast` (already `unsafe`) make *unchecked* pointers
  (generation `0xFFFFFFFF`): the toolchain casts interior addresses of big
  buffers (tokens, AST nodes), where the word before is another object's
  field. A mismatch takes a slow path that lets them through, so checked
  pointers pay nothing for this. A cast of 0 is null.
- New, `unsafe`: `(ptr.make S addr gen)` / `(arr.make T addr gen)` build a
  checked pointer, `(ptr.gen p)` / `(arr.gen a)` read one's generation.
- `std/gen`: fresh generations from a counter in runtime cell 56,
  scrambled (an odd multiplier, so unique until 2^32) so stale data is
  unlikely to equal one by accident; `adopt` gives an allocator's block a
  fresh generation, `retire` checks it is still live (so a double free is
  caught) and marks it freed.
- `std/heap` and `std/alloc` (every allocator kind): each typed object
  gets the 8-byte prefix and its own generation. `std/arena`: an 8-byte
  size header per block, so `reset` walks the blocks and marks every
  object freed, then takes one fresh generation for everything allocated
  until the next reset.
- `eq` on pointers compares address and generation.

Caught, in wasm and natively: use through a copy after `destroy`, use
after the block holds a new object at the same address, a double
`destroy`, use after `arena.reset`, and an array after `free_array`.

Exact for the heap (a block keeps its size class, so the generation word
is always at the same place). After an arena reset, new objects can start
at different addresses, so an old pointer's word can be in the middle of
new data: caught unless that data happens to equal the old (scrambled)
generation.

**Cost** (`tools/bench.py --runs 3`, time with checks / time without):

| Benchmark | Native | Wasm | What it does |
|---|---|---|---|
| spigot | 1.00 | 1.00 | integer arithmetic |
| mandelbrot | 1.03 | 1.04 | float arithmetic |
| nbody | 1.07 | 1.06 | value structs (not pointers) |
| knucleotide | 1.16 | 1.20 | hash map |
| fannkuch | 1.38 | 1.24 | small array reads and writes |
| pidigits | 1.39 | 1.51 | big-integer arrays |
| spectralnorm | 2.48 | 1.00 | array reads: wasmtime removes the cost, the native backend does not |
| binarytrees | 2.00 | 1.62 | allocation and pointer chasing; a node is 32 bytes, was 8 |

binarytrees was 2.38 / 1.79 with a fresh generation per arena allocation
(an atomic add each); one per reset brought it to the line above.

**Checks once per loop (built, 2026-10-16).** `wasm.rs` versions loops: a
loop in which nothing can free memory is compiled twice, and before it
each pointer or array variable it accesses and never reassigns is tested
once (non-null, and live or unchecked) without failing. If all pass, a
copy runs whose accesses to them skip the null and generation checks;
else the usual copy runs, whose checks fail exactly where they would have
(a stale pointer the loop never touches still causes no failure). "Can
free" is decided over the call graph: a function may free if it uses an
unchecked operation that writes memory (raw stores, atomics, threads,
WASI calls that fill buffers; raw loads do not count), calls through a
function reference, or calls one that may. Loops inside a slow copy are
not versioned again, so code grows linearly with nesting depth. It trades
compile time and code size for speed, by choice.

Cost with it, as time over the time without generations, each divided by
C's time in the same run (the machine's speed drifted between runs):

| Benchmark | Native | Wasm |
|---|---|---|
| spigot | 1.07 | 1.00 |
| mandelbrot | 1.08 | 1.15 |
| nbody | 0.93 | 1.07 |
| fannkuch | 1.00 | 1.00 |
| spectralnorm | 1.08 | 1.02 |
| pidigits | 1.01 | 0.82 |
| knucleotide | 1.13 | 1.23 |
| binarytrees | 2.00 | 1.57 |

(pidigits under wasmtime is faster than before: the versioned loops skip
the null checks too.) What remains:

1. **Checks inside small calls** (knucleotide: `str.byte_at` once per
   byte, each call checking its argument): a loop's proof cannot reach a
   callee. The fix would be a pre-checked copy of a small function, called
   from a versioned loop where its pointer argument is already proven.
   **Measured first (2026-10-16), and not built:** removing every
   generation check from knucleotide saves 2.5% natively and 12% under
   wasmtime, an upper bound for any such copies; its remaining cost is
   memory. Dropping the 8-byte prefix from `new` objects (never freed, so
   they could be unchecked) saved 6% under wasmtime and nothing natively,
   also not worth a layout change in both toolchains.
2. **Memory** (binarytrees): a node is 32 bytes, was 8: two 8-byte
   pointers, the 8-byte generation prefix, and the arena's 8-byte block
   header. Dropping the header and the reset walk measured 1.63 natively
   and 1.39 under wasmtime; the price is that a pointer used after a reset
   is caught only once the memory is reused (before that it reads the old
   object, which is not unsafe, only wrong). **Decided (2026-10-16):** the
   default arena keeps the headers and catches a use after reset at once;
   a separate fast arena (`arena.Fast`: `make_fast`, `fast_alloc`,
   `fast_reset`, and `alloc.of_fast_arena`) drops them, for code that opts
   in. Its own type, not a flag, since testing a flag on every allocation
   cost binarytrees 10% natively. binarytrees on a fast arena: 1.67
   natively (2.00 on the default one).
3. **Per-allocator generations** for the heap (a counter under its lock,
   no atomic), as the arena already does.

**Not done in the prototype:** the VM, the AIPL toolchain (codegen.aipl,
checker.aipl, the native backend's reader is unchanged and needed nothing),
tests, and contract messages show a pointer as its 64-bit value.

## Data races (decided 2026-10-16)

Generation checks do not stop data races (two threads using the same
memory at once, one writing), which can forge a pointer too. Starting a
thread and atomics already require `unsafe`, so the claim with generations
is: **code without `unsafe` is memory-safe; threads need `unsafe`.**

**Built (2026-10-16): `std/par`**, so that ordinary programs need no
`unsafe` threads. `(call (par.map T U C) out f ctx xs threads)` sets
`out[i]` to `(call_ref f ctx xs' i)` for every `i`, in contiguous parts,
one on the calling thread and each other on a thread of its own, and
returns when all are done. Nothing a worker can reach is written by
another while it runs:

- `T`, `U`, and `C` must be **plain**, a new kind: `(where (plain T))`,
  checked by both generics passes, means no pointer or array at any depth
  (numbers, `bool`, `str`, enums, function references, and value structs
  and unions of plain fields; not yet a generic instance). So the context
  and each result are copies, and a result cannot carry a pointer out.
- Each part gets its **own copy** of the input array, from the shared heap
  (freed at the end): its worker reads it at full speed, in versioned
  loops, and may even change it.
- `par` itself writes each result to its own element of `out`.
- There are no globals outside `unsafe`; allocation is atomic (`new`,
  `arr.new`) or locked (`std/heap`), and `par` makes the shared heap
  before any worker can ask for it.

A first version read the input through a `par.Shared` view (a struct whose
array field only `par` could reach); every element read was a call, and
one thread took 2.4 times the sequential time, so it became a copy.
`examples/spectralnorm_par.aipl` (each row of a matrix-vector product is
one element of `out`) takes the sequential benchmark's time on one thread
and 0.26 of it on eight, natively; `tests/test_par.rs` checks its result
in every backend. Not covered: work that does not fit "one result per
index" (reductions, shared maps), freezing, and transfer; Rust's
Send/Sync (it needs a borrow checker), isolates that copy every message,
and a race detector were considered and not chosen.

