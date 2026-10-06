# Freeing memory: allocators in the style of Zig

AIPL's memory only grows: `mem.alloc`, `new`, and `arr.new` take from a
bump cursor that never moves back, and `std/arena` frees many values at
once. Long-running programs leak, and growing a `vec`, `buf`, or `strmap`
abandons its old storage. This plan adds per-object `free` the way Zig
does it: nothing allocates freeable memory unless it is handed an
allocator, and code says which allocator it uses.

## Design

- **Allocators are values.** A general-purpose heap, an arena, a fixed
  buffer, or one a program defines. Code that allocates takes one;
  `(call alloc.default)` is a shared general-purpose heap for code that
  does not care.
- **`Allocator` is a union with an open end.** Its variants are the
  built-in allocators, dispatched directly (a closed set, fast), plus
  `custom`, which holds a table of a program's own `alloc`/`free`
  functions. Code taking an `Allocator` works with any of them.
- **A free names what it frees** (its type or size), as in Zig, so the
  allocator needs no size lookups and a mismatch is caught.
- **Mistakes stop the program** with a contract failure naming the
  check, in every backend: double free, freeing what this heap did not
  allocate, a size or type mismatch, and a write after free (freed memory
  holds a junk pattern, checked when the block is reused). Leaks are
  counted (`heap.live`), so tests can require none.
- **The built-ins stay as they are.** `new`, `arr.new`, and `mem.alloc`
  keep the never-freed cursor, which suits data that lives as long as the
  program; freeable data comes from an allocator, which makes "this is
  freed" visible in the code.
- **All AIPL.** The allocators are standard-library code on top of
  `mem.alloc`, so they behave the same in the VM, wasm, and native code
  with no compiler support, except where noted.

## Steps

| Step | What | Status |
|---|---|---|
| H0 | `sizeof` of any memory type (`(sizeof i64)` 8), so generic code can size a `T` | done |
| H1 | `std/heap`: size classes, free lists, the checks above, `live`/`live_bytes`/`reserved`; typed `create`/`destroy`/`array`/`free_array` | done |
| H2 | `std/alloc`: the `Allocator` union (heap, arena, fixed buffer), `alloc.default`, typed helpers over any allocator | |
| H3 | `vec`, `buf`, `strmap` (and `map`) take an allocator and free old storage when they grow; callers updated, the compiler included | |
| H4 | Function references in struct fields (a language change), then `Allocator.custom` | |
| H5 | Docs (spec, prompt guide, gaps), benchmarks (binarytrees with a heap as well as an arena) | |

## H1: std/heap

A request is rounded up to a size class: 8, 16, 24, then four per
doubling (32 40 48 56 64 80 96 112 128 ...), at most a quarter spare. Each
block has an 8-byte header: the heap's address xor a live or freed marker,
and the size asked for. A freed block goes on its class's free list; new
blocks are cut from chunks of at least 64 KiB from `mem.alloc`. A heap's
memory is reused but never returned (the cursor only grows).

The checks are `req`s on predicates named for what they catch, so the
message says what went wrong:
`Pre-condition failed in 'heap.free_raw': (req (call heap.not_freed_already h p)) with h = 1704, p = 2168, n = 8`.
`tests/test_heap.rs` makes each mistake and checks that the VM, `aipl-run`,
and native executables stop with the same message.
