# Generation checks: stopping use after free

Status: planned, after launch (ROADMAP.md, "Next" 2). Nothing here is built.

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
and writes the new object, and nothing notices.

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
   field, across a thread join; mutation checks on the comparison and on
   the bump; `run_fuzz.py` generating frees.
4. **Claims:** with visibility done as well (the other gap, ROADMAP.md),
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
