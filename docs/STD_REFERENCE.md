# The standard library reference

The contract of every function the standard library offers, for language version 1. A program meets the library by
`(import NAME)` and calls `(call NAME.function ...)`. Part 1 states the rules every module follows; part 2 has one
section per module in `aipl_src/std/`: a summary (what the module is for, and what a caller needs to know that no
single function shows), then the module's public items as `aipl doc` prints them (signature, the comment above it,
and its `req` and `ens`). The listings are generated: `python3 tools/std_reference.py --write` rewrites them, and
CI fails when one is not what `aipl doc` prints. The same text is at the prompt: `aipl doc NAME`,
`aipl doc NAME.item`, `aipl doc --find WORD`.

[AIPL_SPEC.md](../AIPL_SPEC.md) section 10 says what the library is as a whole; a function's entry here says what it does.

## 1. Rules for every module

- **Written in AIPL.** Each module is AIPL over `fs.*`, `mem.*`, `str.len`, and `str.ptr`, so a function behaves identically in the VM and in compiled code, on every backend.
- **Allocation.** Memory grows as needed (AIPL_SPEC.md 4.A). A function that makes something that must be freed takes the allocator to make it from (`std/alloc`; `(call alloc.default)` is a shared heap), and the matching `free` gives it back. A container frees its old storage when it grows. Memory from `mem.alloc` and `alloc.forever` is never freed: `io.read_file` makes a buffer per file that is not.
- **Failure.** A failure a caller can meet (a file that is not there, text that is not a number) comes back as a `result` or an `option.Option`. A misuse (an index out of range, a freed object) is a `req` that stops the program, with the call chain.
- **Containers are generic** (AIPL_SPEC.md 4.H): a list of points is `(ptr (vec.Vec (ptr Point)))`, filled with `(call (vec.push (ptr Point)) v p)` and read with `(call (vec.at (ptr Point)) v i)`, with no casts.
- **Text is `str.Bytes`.** The slice type is `str.Bytes` from outside: `str.Bytes`, `(get b str.Bytes.len)`.
- **Tests.** Every module ends in a `run_NAME_tests` function, wired into `aipl_src/test_suite.aipl` and run by `aipl test`.
- **Unsafe** functions say so in their signature, `(unsafe)`; they are called inside an `unsafe` block (AIPL_SPEC.md 3).

## 2. The modules

### std/alloc

Any allocator as one value (docs/design/HEAP_PLAN.md): the union `alloc.Allocator` of a heap, an arena, a fast arena, or a fixed buffer; `of_heap [h]`, `of_arena [r]`, `of_fast_arena [f]`, `fixed [base len]` (unsafe to call: hands out those bytes; a free takes back only the most recent allocation), `default []` (one shared heap, made on first use); `raw [a n]`, `free_raw [a p n]` (unsafe to call), `(call (alloc.create T) a)`, `(call (alloc.destroy T) a p)`, `(call (alloc.array T) a n)`, `(call (alloc.free_array T) a xs)`.

A free does what the allocator does: a heap reuses and checks, an arena ignores it until `reset` ; `(custom C) [state:C alloc_fn free_fn]` for a program's own allocator, unsafe to call (`alloc_fn [state:C n] -> i32`, 8-aligned; `free_fn [state:C p n]`; the state keeps its type, typically a pointer to the allocator's struct)

<!-- api:alloc -->
```text
module alloc
    std/alloc: any allocator, as one value (docs/design/HEAP_PLAN.md). Code that
    allocates freeable memory takes an Allocator and does not care which:

      (fn (fill T) [a:alloc.Allocator n:i32 x:T] -> (arr T)
        (let xs:(arr T) (call (alloc.array T) a n)) ...)

      (call (fill i32) (call alloc.default) 10 7)               ;; the shared heap
      (call (fill i32) (call alloc.forever) 10 7)               ;; never freed
      (call (fill i32) (call alloc.of_heap h) 10 7)             ;; a heap of your own
      (call (fill i32) (call alloc.of_arena ar) 10 7)           ;; freed by arena.reset
      (call (fill i32) (call alloc.fixed (mem.alloc 4096) 4096) 10 7)  ;; inside a buffer
      (call (fill i32) (call (alloc.custom (ptr Pool)) pool (ref my_alloc) (ref my_free)) 10 7)

    A custom allocator is a program's own: my_alloc [state:C n] -> address
    of n bytes, 8-aligned; my_free [state:C p n]. The state is a value of any
    type C, typically a pointer to the allocator's own struct, and is passed
    to both as that type. raw zeroes what it returns, so a custom allocator
    need not.

    Make an Allocator once and pass it around: each of_heap, of_arena, and
    fixed makes a small cell that is never freed. What a free does depends
    on the allocator: a heap reuses the block and checks the free (std/heap);
    an arena ignores it (arena.reset frees everything); a fixed buffer takes
    back only its most recent allocation, as Zig's FixedBufferAllocator does.

struct alloc.Fixed [] and 3 private fields
    A region inside a buffer: [base, base + len), the first `used` bytes
    handed out.

struct alloc.Custom [] and 5 private fields
    What an Allocator holds for one, whatever C is: the CustomOf<C> cell's
    address, and the two functions that read it back as a CustomOf<C>
    (alloc_through, free_through), which only custom makes.

union alloc.Allocator [(heap h:(ptr heap.Heap)) (arena r:(ptr arena.Arena)) (fast_arena fr:(ptr arena.Fast)) (fixed f:(ptr alloc.Fixed)) (custom c:(ptr alloc.Custom)) (forever)]
    Any allocator as one value, to pass to code that allocates: make it
    with of_heap, of_arena, of_fast_arena, fixed, custom, default, or
    forever.

fn alloc.forever [] -> alloc.Allocator
    Memory that lives until the program exits: never reused, so what it
    hands out is never freed (a destroy still ends the object: a pointer
    kept past it stops the program). Right for what a program makes once
    and keeps; a call to it is what (no_leak) refuses (docs/design/
    MEMORY_PLAN.md decision 5).

fn (alloc.custom C) [state:C alloc_fn:(fn [C i32] -> i32) free_fn:(fn [C i32 i32] -> void)] -> alloc.Allocator (unsafe)
    Unsafe to call: the allocator's functions hand out raw addresses.

fn alloc.of_heap [h:(ptr heap.Heap)] -> alloc.Allocator
    An allocator over a heap of your own (std/heap).

fn alloc.of_arena [r:(ptr arena.Arena)] -> alloc.Allocator
    An allocator over an arena: frees do nothing, arena.reset frees all.

fn alloc.of_fast_arena [fr:(ptr arena.Fast)] -> alloc.Allocator
    An allocator over a fast arena (arena.make_fast).

fn alloc.fixed [base:i32 len:i32] -> alloc.Allocator (unsafe)
    An allocator handing out the len bytes at base (from mem.alloc, or
    from another allocator), 8 bytes at a time.
    Unsafe to call: the allocator hands out whatever memory base names.
    (req (gte len 0))

fn alloc.default [] -> alloc.Allocator
    The shared general-purpose heap, made on first use ((no_leak): it
    is made once, then handed out each time).

fn alloc.raw [a:alloc.Allocator n:i32] -> i32
    n zeroed bytes, 8-aligned.
    (no_leak): every allocator here hands out memory it gets back (a
    heap's freed blocks, an arena's chunks after reset, a fixed buffer);
    a custom allocator is its maker's word, as making one is unsafe.

fn alloc.free_raw [a:alloc.Allocator p:i32 n:i32] -> void (unsafe)
    Frees the n bytes at p, which raw returned for n bytes.
    Unsafe to call: p must be what raw returned for n bytes.
    (no_leak): it frees; a custom allocator's free is its maker's word.

fn (alloc.create T) [a:alloc.Allocator] -> (ptr T) (where (struct T))
    A zeroed T.

fn (alloc.destroy T) [a:alloc.Allocator p:(ptr T)] -> void (where (struct T))
    Frees p, which create made from the same allocator; any pointer to it
    used afterwards stops the program with "Use after free".

fn (alloc.array T) [a:alloc.Allocator n:i32] -> (arr T)
    n zeroed Ts, laid out like (arr.new T n).
    (req (gte n 0))

fn (alloc.free_array T) [a:alloc.Allocator xs:(arr T)] -> void
    Frees xs, which array made from the same allocator.

fn alloc.run_alloc_tests [] -> i32
    Pass count; test_suite.aipl expects 9.
```
<!-- /api:alloc -->

### std/arena

A region allocator (frees everything at once; for single objects see `heap`): `make [chunk_size] -> (ptr arena.Arena)`, `(call (arena.alloc T) a) -> (ptr T)` (zeroed, like `new`, no cast), `raw [a n] -> i32` (n zeroed bytes, 8-aligned), `reset [a]` (frees everything from `a`, marking every object freed, so a pointer used after it fails at once; its chunks are reused), `reserved [a]`; never fails while memory remains (a full chunk moves on to another).

A fast arena, `make_fast [chunk_size] -> (ptr arena.Fast)` with `(call (arena.fast_alloc T) f)`, `fast_raw`, and `fast_reset`, keeps no per-block headers and marks nothing at reset: less memory and time, but a pointer used after its reset is caught only once the memory holds a new object (before that it reads the old one)

<!-- api:arena -->
```text
module arena
    std/arena: a region allocator. Allocate many values, then free them all at
    once with reset (for freeing single objects, see std/heap). Typed, with no
    casts at the call site:

      (let a:(ptr arena.Arena) (call arena.make 65536))
      (let n:(ptr Node) (call (arena.alloc Node) a))   ;; zeroed, like (new Node)
      ...
      (call arena.reset a)    ;; everything allocated from a is free again

    Memory comes in chunks from mem.alloc; a full chunk moves on to the next
    (allocating one if needed), and reset reuses them all, so an arena never
    fails while memory remains. Each block has an 8-byte header holding its
    size, so reset can mark every object freed (std/gen): a pointer from an
    arena used after reset stops the program with "Use after free".

    A fast arena (make_fast, fast_alloc, fast_reset) has no headers and marks
    nothing at reset, so it allocates in less memory and time and resets at
    once. A pointer used after its reset is caught only once the memory
    holds a new object; until then it reads the old object as it was (a
    bug, but not unsafe: nothing else lives there, and a new object's memory
    is zeroed first). Choose it where allocation speed matters and the code
    drops its pointers at reset:

      (let f:(ptr arena.Fast) (call arena.make_fast 65536))
      (let n:(ptr Node) (call (arena.fast_alloc Node) f))
      (call arena.fast_reset f)

struct arena.Arena [] and 7 private fields
    A region allocator: alloc hands out memory, reset frees all of it
    at once; a pointer used after reset stops the program.

struct arena.Fast [] and 7 private fields
    A faster arena that catches a pointer used after fast_reset only once
    its memory is reused.

fn arena.make [chunk_size:i32] -> (ptr arena.Arena)
    An arena that takes memory in chunks of at least chunk_size bytes.
    (req (gt chunk_size 0))

fn arena.raw [a:(ptr arena.Arena) n:i32] -> i32
    n zeroed bytes, 8-aligned, after an 8-byte header holding the block's
    size (header included).
    (no_leak): after reset the arena hands out its chunks again; it takes
    a new one only when they are all in use.

fn (arena.alloc T) [a:(ptr arena.Arena)] -> (ptr T) (where (struct T))
    A zeroed T from the arena. Everything allocated between two resets
    shares the arena's generation: a reset marks them all freed and takes
    a fresh one, so no allocation needs its own (std/gen).

fn arena.reset [a:(ptr arena.Arena)] -> void
    Frees everything allocated from the arena: every block's object is
    marked freed (std/gen), then the chunks are handed out again.

fn arena.make_fast [chunk_size:i32] -> (ptr arena.Fast)
    A fast arena taking its memory chunk_size bytes at a time.
    (req (gt chunk_size 0))

fn arena.fast_raw [f:(ptr arena.Fast) n:i32] -> i32
    n zeroed bytes, 8-aligned (no header).
    (no_leak): as raw.

fn (arena.fast_alloc T) [f:(ptr arena.Fast)] -> (ptr T) (where (struct T))
    A zeroed T, with the fast arena's generation until its next reset.

fn arena.fast_reset [f:(ptr arena.Fast)] -> void
    Frees everything at once: a fresh generation for what comes next, so
    an old pointer fails once its memory holds a new object.

fn arena.reserved [a:(ptr arena.Arena)] -> i32
    Bytes the arena holds from the heap (all chunks).

fn arena.run_arena_tests [] -> i32
    Pass count; test_suite.aipl expects 7.
```
<!-- /api:arena -->

### std/assertion

What stops the program when an `assert` fails (section 4, Contracts), imported by a module that uses one: `failed [label:str msg:str]` and `failed_cmp [label left right msg]` (the sides as `show` wrote them; `msg` is "" without a message) stop with `Assertion failed: LABEL[: left L, right R][: MSG]`

<!-- api:assertion -->
```text
module assertion
    std/assertion: what stops the program when an (assert c) or (assert c msg) fails (AIPL_SPEC.md 4, Contracts).
    A module that uses assert imports this one, as one that uses a formatting form imports std/show; the
    resolver lowers (assert (eq n 3) "why") to a call of failed_cmp with the assert's place and text, both sides
    as show writes them, and the message ("" without one). They are ordinary functions, usable directly. The
    program ends here, so the memory that builds the message is no leak (src/leaks.rs).

fn assertion.failed [label:str msg:str] -> void
    Stops the program with "Assertion failed: LABEL", the label being the place and the text of the assert as
    written, then ": MSG" when the program gave one (msg is "" when it did not).

fn assertion.failed_cmp [label:str left:str right:str msg:str] -> void
    The same with ": left L, right R" after the label, the two sides of the comparison that failed.
```
<!-- /api:assertion -->

### std/bigint

Arbitrarily large signed integers, changed in place (reuse numbers rather than making new ones), each from the allocator it is made with, which it grows through: `(ptr bigint.Int)` from `from_i32 [a v]`, `with_capacity [a limbs]`; `free [n]` gives it back; `set_i32 [a v]`, `assign [dst src]`, `add [a b]`, `sub [a b]`, `add_mul_small [a b m]` (a += b·m), `sub_mul_small [a b m]`, `mul_small [a m]` (|m| < 2^31), `div_small [a d] -> i32` (a /= d toward zero, returns the remainder, 0 < d < 2^31), `div_small_quotient [a b] -> i32` (a ≥ 0, b > 0, quotient < 2^31: returns a / b, leaves the remainder in a), `negate [a]`, `compare [a b] -> i32`, `sign [a] -> i32`, `is_zero [a]`, `limb_count [a]`, `push_decimal [out a]`

<!-- api:bigint -->
```text
module bigint
    std/bigint: arbitrarily large signed integers. Operations work in place on
    numbers you keep and reuse. A number's memory comes from the allocator it
    was made with, and goes back to it when the number grows or is freed.

      (let a:(ptr bigint.Int) (call bigint.from_i32 (call alloc.default) 1))
      (for k 1 31 (call bigint.mul_small a k))         ;; a = 30!
      (call bigint.push_decimal b a)                    ;; "265252859812191058636308480000000"

    Sign and magnitude. The magnitude's limbs are 32 bits, least significant
    first, each held in an i64 so a limb times a multiplier below 2^31 plus a
    carry still fits (AIPL has no unsigned comparison or 64x64 -> 128-bit
    multiply). A number's limb array grows by doubling (the old one freed); len is the count of limbs in use, with no leading zero limbs,
    and every limb from len to the array's end is zero (so loops may read
    past len without a check). Zero has len 0 and is never negative.

struct bigint.Int [] and 5 private fields
    An arbitrary-precision signed integer, changed in place; a is the
    allocator its memory comes from.

fn bigint.with_capacity [a:alloc.Allocator limbs:i32] -> (ptr bigint.Int)
    Zero from a, with room for `limbs` 64-bit limbs (at least 4) before it
    grows.

fn bigint.from_i32 [a:alloc.Allocator v:i32] -> (ptr bigint.Int)
    A number from any i32, from a.

fn bigint.free [n:(ptr bigint.Int)] -> void
    Gives n's memory back to the allocator it came from.

fn bigint.set_i32 [a:(ptr bigint.Int) v:i32] -> void
    a = v

fn bigint.is_zero [a:(ptr bigint.Int)] -> bool
    Whether a is zero.

fn bigint.limb_count [a:(ptr bigint.Int)] -> i32
    The number of 32-bit limbs in |a| (0 for zero).

fn bigint.sign [a:(ptr bigint.Int)] -> i32
    -1, 0, or 1.

fn bigint.assign [dst:(ptr bigint.Int) src:(ptr bigint.Int)] -> void
    dst = src (dst keeps its own limbs)

fn bigint.compare_magnitude [a:(ptr bigint.Int) b:(ptr bigint.Int)] -> i32
    Compares magnitudes: -1, 0, or 1 as |a| is less than, equal to, or greater than |b|.

fn bigint.compare [a:(ptr bigint.Int) b:(ptr bigint.Int)] -> i32
    -1, 0, or 1 as a is less than, equal to, or greater than b.

fn bigint.add_mul_small [a:(ptr bigint.Int) b:(ptr bigint.Int) m:i32] -> void
    a = a + b * m (|m| < 2^31; a and b may be the same number)
    (req (gt m -2147483648))

fn bigint.sub_mul_small [a:(ptr bigint.Int) b:(ptr bigint.Int) m:i32] -> void
    a = a - b * m (|m| < 2^31)
    (req (gt m -2147483648))

fn bigint.add [a:(ptr bigint.Int) b:(ptr bigint.Int)] -> void
    a = a + b.

fn bigint.sub [a:(ptr bigint.Int) b:(ptr bigint.Int)] -> void
    a = a - b.

fn bigint.mul_small [a:(ptr bigint.Int) m:i32] -> void
    a = a * m (|m| < 2^31)
    (req (gt m -2147483648))

fn bigint.div_small [a:(ptr bigint.Int) d:i32] -> i32
    a = a / d rounded toward zero; returns the remainder, which has a's
    sign (as i32 / and %). 0 < d < 2^31.
    (req (gt d 0))

fn bigint.div_small_quotient [a:(ptr bigint.Int) b:(ptr bigint.Int)] -> i32
    Returns a / b rounded down and leaves the remainder in a, for a >= 0,
    b > 0 and a quotient below 2^31 (pidigits' digit extraction). The
    quotient is estimated in floating point from the top limbs, taken off
    in one multiply-subtract, then corrected (by at most a step or two).
    (req (gte (call sign a) 0))
    (req (gt (call sign b) 0))

fn bigint.negate [a:(ptr bigint.Int)] -> void
    a = -a

fn bigint.push_decimal [out:(ptr buf.Buf) a:(ptr bigint.Int)] -> void
    Appends a in decimal (works on a copy).

fn bigint.run_bigint_tests [] -> i32
    Pass count; test_suite.aipl expects 13.
```
<!-- /api:bigint -->

### std/buf

String builder: `make [a:alloc.Allocator capacity] -> (ptr buf.Buf)`, `free [b]` (a view taken before then stops the program when read: "Use after free"; so does one taken before a push that moved the bytes), `push_byte`, `push_str [b s:str]`, `insert [b at n c]` (n copies of byte c at index at), `push_unsigned [b u:i64 base upper]` (base 2, 8, 10, or 16, read as unsigned), `push_bytes [b str.Bytes]`, `push_int`, `push_i64`, `push_f64 [b x digits]`, `len`, `byte_at [b i]` (checked; no view made), `clear`, `truncate [b n]` (keeps the first n bytes), `bytes [b] -> str.Bytes` (a view of the contents; take it after building), `room [b n] -> i32` and `grew [b n]` (unsafe to call: the address where n more bytes go, for a system call to write, then how many it wrote), `write [b fd] -> i32`, `print [b]` (the contents to standard output), `to_str [b] -> str` (a copy of the contents, which must be UTF-8; later pushes and `free` leave it alone) `concat [a x y] -> str.Bytes` (x then y in memory from `a`, for `(no_leak)` code that cannot use `str.concat`; so too `(let b (call buf.make a 64)) (format_to b "...") (call buf.bytes b)` for `format`).

<!-- api:buf -->
```text
module buf
    std/buf: a growable byte buffer for building text (a string builder).
    (call buf.bytes b) views the contents as a str.Bytes without copying.
    The view is checked: a push that moves the data frees the old storage,
    and free frees it all, after which reading an older view stops the
    program ("Use after free"), so take the view after building. Its memory
    comes from the allocator it is made with (std/alloc); free releases it.

struct buf.Buf [(read data:i32) (read store:(arr u8))] and 3 private fields
    A growable byte buffer: its bytes are the first len of store (cap
    long, from allocator), and data is store's address, for mem.* in
    unsafe code (it moves as the buffer grows).

fn buf.make [a:alloc.Allocator capacity:i32] -> (ptr buf.Buf)
    An empty buffer with room for `capacity` bytes (at least 16), from a.

fn buf.free [b:(ptr buf.Buf)] -> void
    Frees the storage and the buffer itself; any view of them then stops
    the program when read.

fn buf.len [b:(ptr buf.Buf)] -> i32
    The number of bytes in b.

fn buf.byte_at [b:(ptr buf.Buf) i:i32] -> i32
    Byte i of b (0 <= i < len), without making a view.
    (req (and (gte i 0) (lt i (get b Buf.len))))

fn buf.clear [b:(ptr buf.Buf)] -> void
    Empties b, keeping its storage.

fn buf.truncate [b:(ptr buf.Buf) n:i32] -> void
    Keeps only the first n bytes (0 <= n <= len), keeping the storage.
    (req (and (gte n 0) (lte n (get b Buf.len))))

fn buf.room [b:(ptr buf.Buf) n:i32] -> i32 (unsafe)
    For a writer outside AIPL, such as a system call: makes room for n
    more bytes and gives the address they go to; `grew` then counts the
    ones written. Unsafe to call: the address is good only until the
    buffer next grows, and nothing checks what is written there.
    (req (gte n 0))

fn buf.grew [b:(ptr buf.Buf) n:i32] -> void (unsafe)
    Counts n bytes written at the address `room` gave (n at most what it
    made room for).
    (req (and (gte n 0) (lte (+ (get b Buf.len) n) (get b Buf.cap))))

fn buf.push_byte [b:(ptr buf.Buf) c:i32] -> void
    Appends the byte c (its low 8 bits).

fn buf.push_bytes [b:(ptr buf.Buf) s:str.Bytes] -> void
    Appends the bytes of s.

fn buf.push_str [b:(ptr buf.Buf) s:str] -> void
    (Copied straight from the string: no slice is made, so nothing is kept.)

fn buf.insert [b:(ptr buf.Buf) at:i32 n:i32 c:i32] -> void
    Inserts n copies of byte c at index `at` (0 .. len), moving what
    follows it right.
    (req (and (gte at 0) (and (lte at (get b Buf.len)) (gte n 0))))

fn buf.push_unsigned [b:(ptr buf.Buf) u:i64 base:i32 upper:bool] -> void
    u read as unsigned (so -1i64 is 2^64 - 1), in base 2, 8, 10, or 16
    (lowercase digits unless `upper`), with no sign or prefix.
    (req (or (eq base 2) (or (eq base 8) (or (eq base 10) (eq base 16)))))

fn buf.push_int [b:(ptr buf.Buf) n:i32] -> void
    Decimal, with '-' when negative.

fn buf.push_i64 [b:(ptr buf.Buf) n:i64] -> void
    An i64 in decimal, with '-' when negative.

fn buf.push_f64 [b:(ptr buf.Buf) x:f64 digits:i32] -> void
    An f64 with `digits` decimals (fmt.f64_fixed: exact, ties to even).
    (req (and (gte digits 0) (lte digits 40)))

fn buf.bytes [b:(ptr buf.Buf)] -> str.Bytes
    The contents as a checked view (no copy, nothing allocated): good
    until the next push that moves the data, or free; reading it after
    either stops the program.

fn buf.concat [a:alloc.Allocator x:str.Bytes y:str.Bytes] -> str.Bytes
    x then y, in memory from a (an arena, say, which frees it with the rest): a
    checked view, for (no_leak) code that cannot use str.concat.

fn buf.to_str [b:(ptr buf.Buf)] -> str
    The contents as a new str: a copy, so later pushes and free leave it
    alone. Never freed (str.from_bytes); the contents must be UTF-8.

fn buf.write [b:(ptr buf.Buf) fd:i32] -> i32
    Writes the contents to file descriptor fd; the bytes written, or a
    negative errno (fs.write).

fn buf.print [b:(ptr buf.Buf)] -> void
    Writes the contents to standard output: the usual end of a program that
    builds its output in a buffer and prints it once.

fn buf.run_buf_tests [] -> i32
    Pass count; test_suite.aipl expects 3.
```
<!-- /api:buf -->

### std/cli

Command-line options: `parser [a name about] -> (ptr cli.Parser)`, then `flag [p name short help]`, `option [p name short default help]`, `positional [p name help]` (required, in order), `rest [p name help]` (any after); `parse [p] -> (result (ptr cli.Args) str)` over the program's arguments, `parse_list [p argv]`, or `parse_or_exit [p]` (usage on standard output and exit 0 for `--help` or `-h`; `NAME: ERROR (NAME --help shows the options)` on standard error and exit 2 for arguments that do not fit); `get_str [args name] -> str`, `get_bool`, `get_i64 -> (result i64 str)`, `rest_args`; `usage [p] -> str`.

Accepted: `--name`, `--name value`, `--name=value`, `-n`, `-n value`, `-nvalue`, flags together (`-vq`), `-` as an argument, and `--` to end the options. Errors: `unknown option '--x'`, `option '--name' takes no value`, `option '--name' needs a value`, `missing argument 'file'`, `unexpected argument 'x'`

<!-- api:cli -->
```text
module cli
    std/cli: command-line options. Declare them, then parse the arguments:

      (let p:(ptr cli.Parser) (call cli.parser a "wc" "count the words in files"))
      (call cli.flag p "verbose" "v" "print each file's count")
      (call cli.option p "min" "m" "1" "skip words shorter than this")
      (call cli.positional p "file" "the file to read")
      (let args:(ptr cli.Args) (call cli.parse_or_exit p))
      (call cli.get_bool args "verbose")
      (try (call cli.get_i64 args "min"))

    Accepted: --name, --name value, --name=value; -n, -n value, -nvalue;
    flags together (-vq); "-" as an argument; and "--", after which
    everything is an argument. --help (or -h) prints the usage. An option
    not given has its default; a positional must be given, and rest takes
    any that follow.

struct cli.Parser [] and 5 private fields
    A program's declared options, built with flag, option, positional, and rest.

struct cli.Args [] and 6 private fields
    The values given: each spec's, by name (a flag "true" or ""), and
    what rest took.

fn cli.parser [a:alloc.Allocator name:str about:str] -> (ptr cli.Parser)
    A parser for the program `name`, which does `about`.

fn cli.flag [p:(ptr cli.Parser) name:str short:str help:str] -> void
    A flag, --name or -short (short "" for none): on when given.

fn cli.option [p:(ptr cli.Parser) name:str short:str default:str help:str] -> void
    An option with a value, --name VALUE or -short VALUE, `default` when
    not given.

fn cli.positional [p:(ptr cli.Parser) name:str help:str] -> void
    A positional argument, required, in the order declared.

fn cli.rest [p:(ptr cli.Parser) name:str help:str] -> void
    Any arguments after the positionals (none required).

fn cli.usage [p:(ptr cli.Parser)] -> str
    The usage text --help prints.

fn cli.parse_list [p:(ptr cli.Parser) argv:(ptr (vec.Vec str))] -> (result (ptr cli.Args) str)
    The arguments in argv (argument 1 on, as os.arg gives them), or the
    first that does not fit, with a hint; "help" when --help was asked.

fn cli.parse [p:(ptr cli.Parser)] -> (result (ptr cli.Args) str)
    parse_list over the program's own arguments.

fn cli.parse_or_exit [p:(ptr cli.Parser)] -> (ptr cli.Args)
    parse, printing the usage and exiting 0 for --help, or the error and
    a hint and exiting 2 for arguments that do not fit.

fn cli.get_str [args:(ptr cli.Args) name:str] -> str
    The value of the option, flag, or positional `name` ("" for a flag
    not given, or a name never declared).

fn cli.get_bool [args:(ptr cli.Args) name:str] -> bool
    Whether the flag `name` was given.

fn cli.get_i64 [args:(ptr cli.Args) name:str] -> (result i64 str)
    The value of `name` as an integer, or why it is not one.

fn cli.rest_args [args:(ptr cli.Args)] -> (ptr (vec.Vec str))
    What rest took, in order.
```
<!-- /api:cli -->

### std/cmp

The interfaces `(cmp.Eq T)` (`same`), `(cmp.Ord T)` (`compare`: -1, 0, or 1), and `(cmp.Hash T)` (`hash`, `same`), implemented for `i32`, `i64`, `bool` (Eq), `str` (bytewise), and `str.Bytes` (Hash, by contents); none for floats (section 4.H)

<!-- api:cmp -->
```text
module cmp
    std/cmp: equality, order, and hashing as interfaces, so a generic function
    can ask for them (docs/design/CONSTRAINTS_PLAN.md).

      (call (cmp.Eq.same i32) 3 3)                  ;; => true
      (call (cmp.Ord.compare str) "apple" "pear")   ;; => -1
      (call (cmp.Hash.hash i64) 42i64)

    Implemented here for the built-in types; a module implements them for its
    own types: (impl (cmp.Eq geo.Point) (fn same [a:geo.Point b:geo.Point] -> bool ...)).
    None for f32 or f64: NaN is not equal to itself, so float equality breaks
    what every hash map and sort relies on (same x x).

interface (cmp.Eq T)
    Equality for T: same is true for equal values (a hash map's keys, a
    search).
    (fn same [a:T b:T] -> bool)

interface (cmp.Ord T)
    An order for T, for sorting and searching.
    (fn compare [a:T b:T] -> i32 (ens (and (gte res -1) (lte res 1))))

interface (cmp.Hash T)
    Keys that are equal (same) must hash alike.
    (fn hash [k:T] -> i32)
    (fn same [a:T b:T] -> bool)

fn cmp.run_cmp_tests [] -> i32
    Pass count; test_suite.aipl expects 10.
```
<!-- /api:cmp -->

### std/csv

Comma-separated values, read as Python's `csv.reader` reads them with its default dialect (from its C source, non-strict: quoted fields with delimiters, doubled quotes, and line breaks; text after a closing quote kept; a line ending at `\n`, `\r\n`, or `\r`; an empty line a row of no fields; no field size limit), a row at a time over a `stream.Reader`: `reader [a r]`, `reader_with [a r delim]`, `open [a path] -> (result (ptr csv.Reader) io.Error)`, `of_bytes [a b]`, `close`;

`next_row [cr] -> bool`, `count`, `field [cr i] -> str.Bytes` (a view, good until the next row), `row` (1 the first), `read_error`; `header [cr] -> bool` then `column [cr name] -> i32` (-1 if none) and `get [cr name] -> (option.Option str.Bytes)`; `f64_at` and `i64_at [cr i] -> (result T (ptr csv.Error))`, `Error` holding `row`, `column`, and `what` (`row 3, column 1: x2 is not an integer`); `parse [a b]` (every row at once, as vectors of views).

Writing, as Python's `csv.writer` (quoting a field only if it holds the delimiter, a quote, or a line break; each quote doubled; rows ended by `\r\n`; a row of one empty field written `""`): `writer [a w]`, `writer_with [a w delim]`, `field_bytes`, `field_str`, `field_i64`, `field_f64`, `end_row`, `free`. Checked against Python on 3,600 generated cases (600 kept, tests/aipl/csv)

<!-- api:csv -->
```text
module csv
    std/csv: comma-separated values (RFC 4180, read as Python's csv module
    reads them), a row at a time from a stream, or a whole text at once;
    and written with quoting only where a field needs it.

      (let cr:(ptr csv.Reader) (try (call csv.open a "prices.csv")))
      (defer (call csv.close cr))
      (let _h:bool (call csv.header cr))
      (let price:i32 (call csv.column cr "price"))
      (while (call csv.next_row cr)
        (let p:f64 (try (call csv.f64_at cr price))) ...)

    A field is a view into the reader's buffer, good until the next row:
    copy what you keep. Reading follows Python's csv.reader with its default
    dialect, from its C source: a quoted field may hold delimiters, quotes
    written twice, and line breaks; text after a closing quote is kept as
    written (Python's non-strict mode), a quote inside an unquoted field is
    an ordinary character, an unclosed quote runs to the end, and a line
    ends at "\n", "\r\n", or "\r". An empty line is a row of no fields. Unlike
    Python, a field may be any size. Checked against Python on generated
    texts (tests/aipl/csv, tools/csv_vectors.py).

struct csv.Reader [] and 17 private fields
    data holds the row's fields one after another; spans has each field's
    start and end; views are the fields handed out.

struct csv.Error [(read row:i32) (read column:i32) (read what:str)]
    Where a value did not read: its row (1 the first) and column (1 the
    first), and why, as one line.

fn csv.reader [a:alloc.Allocator r:(ptr stream.Reader)] -> (ptr csv.Reader)
    Reads comma-separated rows from r (close r yourself).

fn csv.reader_with [a:alloc.Allocator r:(ptr stream.Reader) delim:i32] -> (ptr csv.Reader)
    reader with another delimiter byte (9 for tab-separated values).

fn csv.open [a:alloc.Allocator path:str] -> (result (ptr csv.Reader) io.Error)
    Opens a file of comma-separated rows; close closes it.

fn csv.of_bytes [a:alloc.Allocator b:str.Bytes] -> (ptr csv.Reader)
    Reads the text of b.

fn csv.close [cr:(ptr csv.Reader)] -> void
    Frees the reader, and closes the stream open or of_bytes made.

fn csv.next_row [cr:(ptr csv.Reader)] -> bool
    Moves to the next row; false at the end of the input (or after a read
    error: read_error).

fn csv.count [cr:(ptr csv.Reader)] -> i32
    The fields in the current row.

fn csv.field [cr:(ptr csv.Reader) i:i32] -> str.Bytes
    Field i (0 the first) of the current row, good until the next row.
    (req (and (gte i 0) (lt i (get cr Reader.n))))

fn csv.row [cr:(ptr csv.Reader)] -> i32
    The current row's number: 1 the first row read.

fn csv.read_error [cr:(ptr csv.Reader)] -> bool
    Whether a read from the stream failed.

fn csv.header [cr:(ptr csv.Reader)] -> bool
    Reads the next row as the column names (column finds them); false if
    there is none.

fn csv.column [cr:(ptr csv.Reader) name:str] -> i32
    The index of the column the header names `name`, or -1.

fn csv.get [cr:(ptr csv.Reader) name:str] -> (option.Option str.Bytes)
    The current row's field in the column named `name`, or none (no such
    column, or a row too short for it).

fn csv.f64_at [cr:(ptr csv.Reader) i:i32] -> (result f64 (ptr csv.Error))
    Field i as an f64 (text.parse_f64: exactly rounded), or where and why
    it is not one.

fn csv.i64_at [cr:(ptr csv.Reader) i:i32] -> (result i64 (ptr csv.Error))
    Field i as an i64 (str.parse_i64), or where and why it is not one.

fn csv.parse [a:alloc.Allocator b:str.Bytes] -> (ptr (vec.Vec (ptr (vec.Vec str.Bytes))))
    The whole text of b as rows of fields, the fields views into one
    buffer from a (each view is a small cell never freed: for a big input,
    read a row at a time).

struct csv.Writer [] and 6 private fields
    Writes rows to a stream: fields one at a time, then end_row.

fn csv.writer [a:alloc.Allocator w:(ptr stream.Writer)] -> (ptr csv.Writer)
    Writes comma-separated rows to w.

fn csv.writer_with [a:alloc.Allocator w:(ptr stream.Writer) delim:i32] -> (ptr csv.Writer)
    writer with another delimiter byte.

fn csv.free [cw:(ptr csv.Writer)] -> void
    Frees the writer (not its stream: flush or close that).

fn csv.field_bytes [cw:(ptr csv.Writer) b:str.Bytes] -> void
    Writes b as the row's next field, quoted if it holds the delimiter, a
    quote, or a line break (each quote written twice), as Python's
    QUOTE_MINIMAL.

fn csv.field_str [cw:(ptr csv.Writer) s:str] -> void
    field_bytes for a str.

fn csv.field_i64 [cw:(ptr csv.Writer) n:i64] -> void
    An integer field, in decimal.

fn csv.field_f64 [cw:(ptr csv.Writer) x:f64] -> void
    A number field, as Python's str() writes a float (0.1, 2.0, 1e+16).

fn csv.end_row [cw:(ptr csv.Writer)] -> void
    Ends the row with "\r\n" (Python's line terminator); a row of one empty
    field is written "" so it reads back as one.
```
<!-- /api:csv -->

### std/date

Dates and times in UTC (no time zones, no leap seconds), years 0000-9999: `(struct Date [year month day hour minute second nano] (value))` with read-only fields; `now [] -> Date`, `from_unix [secs:i64]`, `from_unix_ns [ns:i64]`, `to_unix [d] -> i64`, `to_unix_ns [d] -> i64`, `make [year month day hour minute second nano] -> Date` (checked by `valid [...] -> bool`), `weekday [d] -> i32` (1 Monday .. 7 Sunday), `day_of_year`, `is_leap [year]`, `days_in_month [year month]`, `add_seconds [d secs:i64]`, `add_days [d days]`;

ISO 8601: `push_iso [b d]` and `to_str [d] -> str` (`2026-10-16T09:41:07.25Z`), `push_iso_date [b d]`, `parse [b:str.Bytes] -> (result Date i32)` (RFC 3339: a date, or a date and time with `Z` or a `+HH:MM` offset, which is applied; the error is the index of the first byte that does not fit)

<!-- api:date -->
```text
module date
    std/date: dates and times in UTC, from the system clock or Unix time,
    written and read as ISO 8601 (the RFC 3339 profile).

      (let d:date.Date (call date.now))
      (call io.println (call date.to_str d))           ;; 2026-10-16T09:41:07.25Z
      (match_result (call date.parse (call str.from_str "2026-10-16T11:41:07+02:00"))
        (ok t (println "unix {(call date.to_unix t)}"))   ;; 1792143667
        (err at (println "bad byte {at}")))

    The calendar is the proleptic Gregorian one, years 0000 through 9999.
    There are no time zones (an offset in parsed text is applied, to give
    UTC) and no leap seconds (every day has 86400 seconds, as in Unix time).
    The day arithmetic is Howard Hinnant's days_from_civil / civil_from_days.

struct date.Date [(read year:i32) (read month:i32) (read day:i32) (read hour:i32) (read minute:i32) (read second:i32) (read nano:i32)] (value)
    One instant in UTC: nano is the nanoseconds past the second.

fn date.is_leap [year:i32] -> bool
    Whether year is a leap year (Gregorian).

fn date.days_in_month [year:i32 month:i32] -> i32
    The number of days in month (1-12) of year.
    (req (and (gte month 1) (lte month 12)))

fn date.valid [year:i32 month:i32 day:i32 hour:i32 minute:i32 second:i32 nano:i32] -> bool
    Whether the fields name a real instant in years 0000-9999.

fn date.make [year:i32 month:i32 day:i32 hour:i32 minute:i32 second:i32 nano:i32] -> date.Date
    The date and time given, which must be valid (see valid).
    (req (call valid year month day hour minute second nano))

fn date.from_unix [secs:i64] -> date.Date
    Unix time in seconds (in years 0000-9999).

fn date.from_unix_ns [ns:i64] -> date.Date
    Unix time in nanoseconds (any i64: years 1677-2262).

fn date.now [] -> date.Date
    Now, from the system clock.

fn date.to_unix [d:date.Date] -> i64
    Seconds since 1970-01-01T00:00:00Z (the nanoseconds dropped).

fn date.to_unix_ns [d:date.Date] -> i64
    Nanoseconds since 1970-01-01T00:00:00Z; d must be in 1677-09-21
    through 2262-04-11, the range of an i64 of nanoseconds.
    (req (and (gte (call to_unix d) -9223372036i64) (lte (call to_unix d) 9223372035i64)))

fn date.weekday [d:date.Date] -> i32
    The day of the week, ISO numbering: 1 Monday through 7 Sunday.

fn date.day_of_year [d:date.Date] -> i32
    The day of the year, 1 through 366.

fn date.add_seconds [d:date.Date secs:i64] -> date.Date
    d moved by `secs` seconds (negative: earlier); the result must stay in
    years 0000-9999.

fn date.add_days [d:date.Date days:i32] -> date.Date
    d moved by `days` days (negative goes back).

fn date.push_iso [b:(ptr buf.Buf) d:date.Date] -> void
    Appends d as YYYY-MM-DDTHH:MM:SSZ, with the fraction of the second
    before the Z when there is one (to the last nonzero digit, at most 9).

fn date.push_iso_date [b:(ptr buf.Buf) d:date.Date] -> void
    Appends d's date alone, YYYY-MM-DD.

fn date.to_str [d:date.Date] -> str
    d as ISO 8601 text (push_iso), a new str.

fn date.parse [t:str.Bytes] -> (result date.Date i32)
    Reads RFC 3339 text: YYYY-MM-DD alone (midnight UTC), or
    YYYY-MM-DDTHH:MM:SS, an optional fraction of the second (1-9 digits
    after a '.'), and the offset from UTC, Z or +HH:MM or -HH:MM, which is
    applied. 't', 'z', and a space for the T are accepted, as RFC 3339
    allows. A time without an offset is an error rather than a guess. The
    error is the index of the first byte that does not fit (the length,
    when the text ends too soon).

fn date.run_date_tests [] -> i32
    Pass count; test_suite.aipl expects 8.
```
<!-- /api:date -->

### std/deflate

DEFLATE (RFC 1951), raw and in zlib (RFC 1950) and gzip (RFC 1952) wrappers, both ways: `compress_to [a o:(ptr buf.Buf) b:str.Bytes level]`, `zlib_compress_to`, `gzip_compress_to` (levels 0, stored, to 9; LZ77 over a 32 KiB window with hash chains, lazy from level 4; each block stored, fixed, or dynamic, whichever is smallest; within 1% of zlib's size); `inflate_to [a o b limit] -> (result i32 (ptr deflate.Error))` and `zlib_decompress_to` (the input bytes the stream took; what follows is the caller's), `gzip_decompress_to` (every member to the end; zero padding allowed; an empty input is no members); `limit` caps the output; `Error.at` (input offset) and `Error.what`, `free_error [a e]`.

Refuses what zlib refuses (puff.c's rules), checks the Adler-32, CRC-32, and length, and refuses gzip's reserved header flags as zlib does

<!-- api:deflate -->
```text
module deflate
    std/deflate: DEFLATE compression (RFC 1951), raw and in the zlib (RFC
    1950) and gzip (RFC 1952) wrappers, both ways.

      (call deflate.gzip_compress_to a o data 6)          ;; levels 0 to 9
      (match_result (call deflate.gzip_decompress_to a o data 100000000)
        (ok used ...)                                     ;; input bytes read
        (err e ... (get e deflate.Error.what) ...))

    Decompression reads every block type and refuses what zlib refuses
    (its codes are puff.c's, zlib's reference decoder); the limit caps the
    output, so a small input cannot make a huge one. The raw and zlib
    decoders give how many input bytes the stream took and leave what
    follows to the caller; the gzip decoder reads members one after another
    to the end, as Python's gzip does. Compression is LZ77 over a 32 KiB
    window with hash chains (lazy matching from level 4), in blocks coded
    each way that is smallest: stored, fixed, or dynamic Huffman codes.
    Level 0 stores. Everything compressed here decompresses with Python's
    zlib and gzip to the same bytes, and the reverse (tests/aipl/deflate);
    the compressed bytes are not zlib's own.

struct deflate.Error [(read at:i32) (read what:str)]
    Why a stream was refused, and the input byte offset where.

fn deflate.free_error [a:alloc.Allocator e:(ptr deflate.Error)] -> void
    Frees an Error a decoder gave.

fn deflate.inflate_to [a:alloc.Allocator o:(ptr buf.Buf) input:str.Bytes limit:i32] -> (result i32 (ptr deflate.Error))
    Appends what the raw DEFLATE stream in stands for to o (at most limit
    bytes); how many bytes of in the stream took, or the error.

fn deflate.zlib_decompress_to [a:alloc.Allocator o:(ptr buf.Buf) input:str.Bytes limit:i32] -> (result i32 (ptr deflate.Error))
    Appends what the zlib stream in stands for to o (at most limit bytes);
    how many bytes of in the stream took, or the error (a bad header, a
    preset dictionary, or a wrong Adler-32 among them).

fn deflate.gzip_decompress_to [a:alloc.Allocator o:(ptr buf.Buf) input:str.Bytes limit:i32] -> (result i32 (ptr deflate.Error))
    Appends what the gzip data in stands for to o (at most limit bytes):
    each member in turn to the end of in (zero bytes may pad the end, as
    tape archives do; an empty in is no members); in's length, or the
    error (a bad header, a wrong CRC-32 or length among them).

fn deflate.compress_to [a:alloc.Allocator o:(ptr buf.Buf) b:str.Bytes level:i32] -> void
    Appends the raw DEFLATE stream of b at level (0: stored, 1 fastest to
    9 smallest) to o.
    (req (and (gte level 0) (lte level 9)))

fn deflate.zlib_compress_to [a:alloc.Allocator o:(ptr buf.Buf) b:str.Bytes level:i32] -> void
    Appends the zlib stream of b at level to o (a header, the DEFLATE
    stream, and b's Adler-32).
    (req (and (gte level 0) (lte level 9)))

fn deflate.gzip_compress_to [a:alloc.Allocator o:(ptr buf.Buf) b:str.Bytes level:i32] -> void
    Appends the gzip data of b at level to o: one member with no name
    and no time, then b's CRC-32 and length.
    (req (and (gte level 0) (lte level 9)))
```
<!-- /api:deflate -->

### std/encode

Bytes as text and back, decoding strictly (it accepts what Python's `binascii` with `strict_mode` and `urllib.parse` accept; the error is the index of the first byte it refuses, and nothing is appended then): `base64_to [o:(ptr buf.Buf) b:str.Bytes alphabet:encode.Alphabet padding:encode.Padding]` (`Alphabet.standard` or `url_safe`, `Padding.padded` or `unpadded`), `base64_decode_to [o b alphabet padding] -> (result i32 i32)` (bytes appended, or the index); `hex_to [o b upper]`, `hex_decode_to [o b]`, `hex_value [c] -> i32`;

`percent_encode_to [o b keep:str plus:bool]` (keeps A-Z a-z 0-9 `_.-~` and keep's ASCII, `%XX` for the rest, with plus a space as `+`: Python's `quote` and `quote_plus`), `percent_decode_to [o b plus]` (a `%` without two hex digits is an error); and from str to str: `base64`, `base64_decode -> (result str i32)`, `hex`, `hex_decode`, `percent_encode [s keep]`, `percent_decode`

<!-- api:encode -->
```text
module encode
    std/encode: bytes as text and back: base64 (RFC 4648, standard and
    URL-safe, padded or not), hex, and percent-encoding (as URLs use it).

      (call encode.base64 "hello")              ;; "aGVsbG8="
      (call encode.base64_decode "aGVsbG8=")    ;; (ok "hello")
      (call encode.hex "AB")                    ;; "4142"
      (call encode.percent_encode "a b/c" "/")  ;; "a%20b/c"

    Each encoding comes as an `_to` function that appends to a buf.Buf (for
    any bytes) and as one from str to str. Decoding is strict: it returns
    the index of the first byte it cannot accept as the error, and appends
    nothing then. What it accepts is what Python's binascii (strict_mode)
    and urllib.parse accept (tests/aipl/encode, checked against Python).

enum encode.Alphabet [standard url_safe]
    Which 64 characters base64 uses: `+` and `/` for 62 and 63
    (standard), or `-` and `_` (url_safe, safe in URLs and file names).

enum encode.Padding [padded unpadded]
    Whether base64 text ends in `=` to a multiple of 4 characters.

fn encode.base64_to [o:(ptr buf.Buf) b:str.Bytes alphabet:encode.Alphabet padding:encode.Padding] -> void
    Appends the base64 text of b to o.

fn encode.base64_decode_to [o:(ptr buf.Buf) b:str.Bytes alphabet:encode.Alphabet padding:encode.Padding] -> (result i32 i32)
    Appends the bytes the base64 text b stands for; how many, or where b
    goes wrong (with nothing appended). Bits left over in the last
    character are ignored, as Python does.

fn encode.base64 [s:str] -> str
    The standard, padded base64 text of s.

fn encode.base64_decode [s:str] -> (result str i32)
    The bytes the standard, padded base64 text s stands for, or the index
    where s goes wrong.

fn encode.hex_value [c:i32] -> i32
    The value of the hex digit c (either case), or -1.

fn encode.hex_to [o:(ptr buf.Buf) b:str.Bytes upper:bool] -> void
    Appends two hex digits for each byte of b (A-F if upper, else a-f).

fn encode.hex_decode_to [o:(ptr buf.Buf) b:str.Bytes] -> (result i32 i32)
    Appends the bytes whose hex digits (either case, no spaces) are b; how
    many, or where b goes wrong (its length if that is odd).

fn encode.hex [s:str] -> str
    The lowercase hex digits of s's bytes.

fn encode.hex_decode [s:str] -> (result str i32)
    The bytes whose hex digits are s, or where s goes wrong.

fn encode.percent_encode_to [o:(ptr buf.Buf) b:str.Bytes keep:str plus:bool] -> void
    Appends b with each byte other than the unreserved ones and the ASCII
    characters of `keep` written as `%` and two uppercase hex digits; with
    plus, a space is written `+` (as HTML forms send it), as Python's
    urllib.parse.quote(b, safe=keep) and quote_plus do.

fn encode.percent_decode_to [o:(ptr buf.Buf) b:str.Bytes plus:bool] -> (result i32 i32)
    Appends the bytes b stands for, each `%` and two hex digits (either
    case) becoming that byte and, with plus, each `+` a space; how many,
    or the index of a `%` without two hex digits after it. (Python's
    unquote leaves such a `%` as it is; this stops instead.)

fn encode.percent_encode [s:str keep:str] -> str
    s percent-encoded, keeping the unreserved characters and those of
    keep: (call encode.percent_encode "a b/c" "/") is "a%20b/c".

fn encode.percent_decode [s:str] -> (result str i32)
    The bytes the percent-encoded s stands for (`+` stays `+`), or the
    index of a `%` without two hex digits after it.
```
<!-- /api:encode -->

### std/float

`from_decimal [addr len] -> f64`: decimal text (an optional sign, digits with one `.`, optionally `e`/`E` and a signed exponent) to the nearest `f64`, ties to even, as Rust's `parse::<f64>`; too large is an infinity, too small a zero. The self-hosted compiler converts its float literals with it; `tests/test_std.rs` compares it with Rust on 400,000 literals

<!-- api:float -->
```text
module float
    std/float: decimal text to the nearest f64, rounding ties to even, as
    Rust's str::parse::<f64> (and so the Rust toolchain's float literals)
    does. The self-hosted compiler converts its float literals with it.

      (call float.from_decimal (str.ptr "0.1") (str.len "0.1"))   ;; 0.1

    Most literals take the fast path: when the digits form an integer m of at
    most 2^53 and the power of ten p has |p| <= 22, both m and 10^|p| are
    exact doubles and one IEEE multiplication or division rounds correctly.
    Anything else is computed exactly with std/bigint: the value is N / D
    for integers N and D; doubling one of them until 1 <= N / D < 2 gives
    the binary exponent, binary long division gives the significand's bits,
    and the remainder decides the rounding.

fn float.from_decimal [addr:i32 len:i32] -> f64 (unsafe)
    The f64 nearest the decimal number at (addr, len): an optional sign,
    digits with one '.' and at least one digit, then optionally 'e' or
    'E', an optional sign, and digits (compiler.is_float_literal's form).
    Too large is an infinity and too small a zero, with the sign. The fast
    path allocates nothing; the exact one, two numbers sized up front.

fn float.run_float_tests [] -> i32
    Pass count; test_suite.aipl expects 14.
```
<!-- /api:float -->

### std/fmt

`uint_to_bytes [n out] -> i32` (n read as unsigned), `int_to_bytes` (leading `-`), `hex_to_bytes` (lowercase, no prefix), and for `i64`: `uint64_to_bytes`, `int64_to_bytes`: each writes ASCII at `out` and returns the count (at most 10, 11, 8, 20, and 21 bytes); `f64_fixed [x digits out]` writes `x` with `digits` decimals exactly as C's `printf("%.*f")` (the exact decimal value of the double, rounded half to even; `inf`, `nan`, a `-` for any negative sign bit; at most 312 + digits bytes)

<!-- api:fmt -->
```text
module fmt
    std/fmt: numbers to text. Each function writes ASCII bytes at `out` (which
    must have room: 10 bytes for uint/hex, 11 for int, 20 for uint64, 21 for
    int64, 312 + digits for f64_fixed) and returns the count.

fn fmt.uint_to_bytes [n:i32 out:i32] -> i32 (unsafe)
    Decimal digits of n read as unsigned, so -1 is 4294967295.
    (ens (and (gte res 1) (lte res 10)))

fn fmt.int_to_bytes [n:i32 out:i32] -> i32 (unsafe)
    Decimal digits of n, with a leading '-' when negative.
    (ens (and (gte res 1) (lte res 11)))

fn fmt.uint64_to_bytes [n:i64 out:i32] -> i32 (unsafe)
    Decimal digits of the i64 n read as unsigned (up to 20).
    (ens (and (gte res 1) (lte res 20)))

fn fmt.int64_to_bytes [n:i64 out:i32] -> i32 (unsafe)
    Decimal digits of the i64 n, with a leading '-' when negative.
    (ens (and (gte res 1) (lte res 21)))

fn fmt.f64_fixed [x:f64 digits:i32 out:i32] -> i32 (unsafe)
    Writes x with `digits` decimals (0-40) at `out`, exactly rounded with
    ties to even, as printf("%.*f") does; returns the bytes written. `out`
    must have room for 312 + digits bytes. Its three big integers come from
    the shared heap and go back to it, so a call keeps no memory.
    (req (and (gte digits 0) (lte digits 40)))

fn fmt.hex_to_bytes [n:i32 out:i32] -> i32 (unsafe)
    Lowercase hex digits of n read as unsigned, no prefix, no leading zeros.
    (ens (and (gte res 1) (lte res 8)))

fn fmt.run_fmt_tests [] -> i32
    Pass count; test_suite.aipl expects 14.
```
<!-- /api:fmt -->

### std/fs

Directories, file information, and permissions over `fs.opendir`, `fs.readdir`, `fs.stat`, and `fs.chmod`: `list [a:alloc.Allocator path:str] -> (option.Option (ptr (vec.Vec str.Bytes)))` (the names in a directory sorted bytewise, without `.` and `..`, so a listing is the same in every backend; `none` if it cannot be read), `info [path:str] -> (option.Option fs.Info)` with `(struct Info [(read kind:fs.Kind) (read size:i64) (read modified:i64)] (value))`, a value struct (section 4.E) (`fs.Kind.file`, `directory`, or `other`, after following symbolic links; `modified` in nanoseconds since 1970), and `list_path` / `info_path` for a path held as `str.Bytes`;

`chmod [path:str mode:i32] -> (result i32 io.Error)` (and `chmod_path`) sets the permission bits, 0 to 0o777, with `fs.FILE` (0o644) and `fs.PROGRAM` (0o755) for the two common modes, confined as the other file operations are, and needing no grant.

Starting other programs is std/proc (below), over `proc.spawn` and its operations (section 4.C), granted by `--allow-run`

<!-- api:fs -->
```text
module fs
    std/fs: directories and file information (AIPL_SPEC.md 4.C), over
    fs.opendir, fs.readdir, fs.stat, and fs.chmod:

      (match (call fs.list (call alloc.default) "data")
        ((option.Option (ptr (vec.Vec str.Bytes))) some [names] ...)
        ((option.Option (ptr (vec.Vec str.Bytes))) none ...))
      (match (call fs.info "data/a.txt")
        ((option.Option fs.Info) some [i] (get i fs.Info.size))
        ((option.Option fs.Info) none -1i64))
      (call fs.chmod "build/tool" fs.PROGRAM)    ;; (result i32 io.Error)

    A directory's names come sorted bytewise, without "." and "..", so a
    listing is the same in every backend and on every file system (the order
    a directory gives them in is the file system's). A path that cannot be
    read gives none.

enum fs.Kind [file directory other]
    What a path names, after following symbolic links.

struct fs.Info [(read kind:fs.Kind) (read size:i64) (read modified:i64)] (value)
    size in bytes, modified in nanoseconds since 1970 (UTC); a value
    (AIPL_SPEC.md 4.E), so nothing is allocated for it

fn fs.list [a:alloc.Allocator path:str] -> (option.Option (ptr (vec.Vec str.Bytes)))
    The names in directory `path`, sorted; the vector and the names come
    from `a`.

fn fs.list_path [a:alloc.Allocator path:str.Bytes] -> (option.Option (ptr (vec.Vec str.Bytes)))
    list for a path held as bytes.

fn fs.info [path:str] -> (option.Option fs.Info)
    What `path` names: its kind, size, and when it was last modified.

fn fs.info_path [path:str.Bytes] -> (option.Option fs.Info)
    info for a path held as bytes.

const fs.FILE:i32 = 420
    Mode bits for chmod: owner read and write, everyone else read (0o644), the
    mode of an ordinary file.

const fs.PROGRAM:i32 = 493
    Owner read, write, and execute; everyone else read and execute (0o755), the
    mode of a program.

fn fs.chmod [path:str mode:i32] -> (result i32 io.Error)
    Sets the permission bits of `path`: `mode` is 0 to 0o777 (setuid, setgid,
    and sticky are refused). Needs no grant: the path is one the program could
    open, confined as fs.open is (a relative path stays in the working
    directory; under --sandbox, nothing outside it). The result is 0, or why
    not (a missing path not_found, a path out of reach denied, a bad mode
    other).

fn fs.chmod_path [path:str.Bytes mode:i32] -> (result i32 io.Error)
    chmod for a path held as bytes.

fn fs.rename [from:str to:str] -> (result i32 io.Error)
    Moves `from` to `to`, replacing what is there: a file replaces a file, a
    directory replaces only an empty directory. A link is moved itself, not
    what it names. Needs no grant: both paths are ones the program could open,
    confined as fs.open is. The result is 0, or why not (a missing `from`
    not_found, a path out of reach denied, a directory onto a file or a file
    onto a directory other).

fn fs.rename_path [from:str.Bytes to:str.Bytes] -> (result i32 io.Error)
    rename for paths held as bytes.

fn fs.remove [path:str] -> (result i32 io.Error)
    Deletes the file `path` (a link is deleted itself, not what it names; a
    directory is refused, "is a directory"). Needs no grant: the path is one
    the program could open, confined as fs.open is. The result is 0, or why
    not (a missing path not_found, a path out of reach denied).

fn fs.remove_path [path:str.Bytes] -> (result i32 io.Error)
    remove for a path held as bytes.

fn fs.mkdir [path:str] -> (result i32 io.Error)
    Makes the directory `path`, with the mode the umask leaves (0777 less it).
    Its parent must exist: there is no `mkdir -p`. Needs no grant: the path is
    one the program could open, confined as fs.open is. The result is 0, or
    why not (a missing parent, or a file as the parent, not_found; a path out
    of reach denied; a path that exists, a file or a link too, exists).

fn fs.mkdir_path [path:str.Bytes] -> (result i32 io.Error)
    mkdir for a path held as bytes.

fn fs.rmdir [path:str] -> (result i32 io.Error)
    Removes the directory `path`, which must be empty. Needs no grant, and is
    confined as mkdir is. The result is 0, or why not (a missing path, or a
    file or a link, not_found: use remove for those; a path out of reach
    denied; a directory with anything in it not_empty).

fn fs.rmdir_path [path:str.Bytes] -> (result i32 io.Error)
    rmdir for a path held as bytes.

fn fs.run_fs_tests [] -> i32
    Pass count; test_suite.aipl expects 4. Run from the repository root.
```
<!-- /api:fs -->

### std/gen

Generations for allocators (GENERATIONS_PLAN.md): `fresh [] -> i32`, `(adopt T) [base] -> (ptr T)` and `(adopt_array T) [base n] -> (arr T)` (a zeroed block of 8 + the object's bytes becomes a checked object with a fresh generation), `(retire T) [p] -> i32` and `(retire_array T) [a]` (fails if already freed; marks it freed; returns the block), `kill [base]`; all unsafe to call

<!-- api:gen -->
```text
module gen
    std/gen: generations for the objects allocators hand out and take back
    (docs/design/GENERATIONS_PLAN.md). An object an allocator gives has an
    8-byte prefix: its generation word, then (for an array) its length. A
    pointer carries the generation it was made with, and every access
    compares the two, so once the object is freed (its word changed), a
    pointer kept from before stops the program with "Use after free", even
    after the memory holds another object.

      (let p:(ptr Point) (call (gen.adopt Point) (call heap.raw h (+ (sizeof Point) 8))))
      (call heap.free_raw h (call (gen.retire Point) p) (+ (sizeof Point) 8))

    std/heap, std/arena, and std/alloc do this; programs use them.

fn gen.fresh [] -> i32
    A generation no object has had since the program started (until 2^32
    of them): the counter's next value, scrambled (an odd multiplier is
    one to one) so stale data is unlikely to equal it by accident. Never 0
    (objects from new), -1 (ptr.cast), or DEAD.

fn (gen.forever T) [] -> (ptr T) (where (struct T))
    A zeroed T that is never freed, from the bump cursor (mem.alloc):
    what alloc.forever hands out, here for the allocators std/alloc is
    made of, which cannot import it. Its generation is fresh, so a pointer
    kept past alloc.destroy still stops the program.

fn (gen.forever_array T) [n:i32] -> (arr T)
    n zeroed Ts that are never freed, as forever.
    (req (gte n 0))

fn (gen.adopt T) [base:i32] -> (ptr T) (where (struct T)) (unsafe)
    A T in the zeroed block at `base` (sizeof T + 8 bytes).

fn (gen.adopt_array T) [base:i32 n:i32] -> (arr T) (unsafe)
    n Ts in the zeroed block at `base` (8 + n * sizeof T bytes), laid out
    like (arr.new T n).

fn (gen.retire T) [p:(ptr T)] -> i32 (where (struct T)) (unsafe)
    Ends p's object, which must not be freed already (freeing it twice is
    a use after free); returns its block, for the allocator to take back.
    (req (call not_freed_already (ptr.addr p) (ptr.gen p)))

fn (gen.retire_array T) [a:(arr T)] -> i32 (unsafe)
    Ends a's array, which must not be freed already; returns its block,
    for the allocator to take back.
    (req (call not_freed_already (arr.addr a) (arr.gen a)))

fn gen.kill [base:i32] -> void (unsafe)
    Marks the object whose prefix starts at `base` freed (an arena reset).
```
<!-- /api:gen -->

### std/hash

Checksums and hashes: `hash.Algorithm` is `md5`, `sha1`, `sha256`, or `sha512`; `make [a alg] -> (ptr hash.Hasher)`, `update [x b:str.Bytes]`, `update_reader [x r:(ptr stream.Reader)] -> i64` (bytes read, or -1), `finish_to [x o:(ptr buf.Buf)]` (appends the digest; no updates after), `free`; `digest_to [o alg b]`, `hex [alg s] -> str` (Python's `hexdigest()`), `digest_size [alg]`, `block_size [alg]`; `hmac_to [o alg key msg]` and `hmac_hex [alg key:str msg:str]` (RFC 2104; a key longer than a block is hashed first); `crc32 [b] -> i32` and `crc32_update [crc b]` (zlib's, signed), `adler32` and `adler32_update [adler b]`; `constant_time_eq [a b] -> bool` (time independent of where they differ).

MD5 and SHA-1 are for checksums only (both broken for security); no encryption

<!-- api:hash -->
```text
module hash
    std/hash: checksums and hashes of bytes: CRC-32 (zlib's), Adler-32,
    MD5, SHA-1, SHA-256, SHA-512, and HMAC over any of the four.

      (call hash.hex hash.Algorithm.sha256 "abc")     ;; "ba7816bf8f01cfea..."
      (call hash.hmac_hex hash.Algorithm.sha256 "key" "message")
      (call hash.crc32 (call str.from_str "abc"))      ;; 891568578 (0x352441c2)

    For big inputs, a Hasher takes the bytes in pieces: make, update (or
    update_reader, for a whole stream), finish_to, free. MD5 and SHA-1 are
    for checksums and old formats only: both are broken for security
    (anyone can make two inputs with the same digest). There is no
    encryption here, by design. Digests compare with constant_time_eq,
    whose time does not depend on where they differ. Checked against the
    published test vectors and against Python's hashlib, hmac, and zlib
    (tests/aipl/hash).

enum hash.Algorithm [md5 sha1 sha256 sha512]
    Which hash a Hasher computes.

struct hash.Hasher [] and 19 private fields
    A hash in progress: the chaining words (h32 or h64), the round
    constants (k32 or k64), the message schedule (w32 or w64), and the
    bytes of the block not yet full (block, fill of them). total counts
    every byte given so far.

fn hash.digest_size [alg:hash.Algorithm] -> i32
    The digest's length in bytes: 16, 20, 32, or 64.

fn hash.block_size [alg:hash.Algorithm] -> i32
    The block length in bytes: 64, or 128 for SHA-512 (what HMAC pads
    its key to).

fn hash.make [a:alloc.Allocator alg:hash.Algorithm] -> (ptr hash.Hasher)
    A Hasher for alg, from a; nothing given to it yet.

fn hash.free [x:(ptr hash.Hasher)] -> void
    Frees the Hasher.

fn hash.update [x:(ptr hash.Hasher) b:str.Bytes] -> void
    Gives b to the hash (not after finish_to).
    (req (not (get x Hasher.done)))

fn hash.update_reader [x:(ptr hash.Hasher) r:(ptr stream.Reader)] -> i64
    Gives everything r reads to the hash; how many bytes, or -1 after a
    read error.

fn hash.finish_to [x:(ptr hash.Hasher) o:(ptr buf.Buf)] -> void
    Appends the digest (digest_size bytes) to o. The Hasher takes no more
    bytes after this.
    (req (not (get x Hasher.done)))

fn hash.digest_to [o:(ptr buf.Buf) alg:hash.Algorithm b:str.Bytes] -> void
    Appends the digest of b to o.

fn hash.hex [alg:hash.Algorithm s:str] -> str
    The digest of s's bytes as lowercase hex (Python's hexdigest()).

fn hash.hmac_to [o:(ptr buf.Buf) alg:hash.Algorithm key:str.Bytes msg:str.Bytes] -> void
    Appends the HMAC of msg under key to o (digest_size bytes): a key
    longer than a block is hashed first, as the RFC says.

fn hash.hmac_hex [alg:hash.Algorithm key:str msg:str] -> str
    The HMAC of msg under key as lowercase hex (Python's
    hmac.new(key, msg, alg).hexdigest()).

fn hash.constant_time_eq [a:str.Bytes b:str.Bytes] -> bool
    Whether a and b hold the same bytes, taking the same time wherever
    they differ (for digests and tokens; their lengths are not secret).

fn hash.crc32_update [crc:i32 b:str.Bytes] -> i32
    crc (a CRC-32 so far, 0 to start) continued over b: zlib's crc32
    (the reflected polynomial 0xEDB88320, as PNG, gzip, and zip use).

fn hash.crc32 [b:str.Bytes] -> i32
    The CRC-32 of b (zlib.crc32(b), as a signed i32).

fn hash.adler32_update [adler:i32 b:str.Bytes] -> i32
    adler (an Adler-32 so far, 1 to start) continued over b: zlib's
    adler32.

fn hash.adler32 [b:str.Bytes] -> i32
    The Adler-32 of b (zlib.adler32(b)).
```
<!-- /api:hash -->

### std/heap

A general-purpose allocator with per-object free (docs/design/HEAP_PLAN.md): `make [] -> (ptr heap.Heap)`, `(call (heap.create T) h) -> (ptr T)` and `(call (heap.destroy T) h p)`, `(call (heap.array T) h n) -> (arr T)` and `(call (heap.free_array T) h a)`, `raw [h n] -> i32` and `free_raw [h p n]` (n zeroed bytes, 8-aligned), `live [h]` (blocks not freed), `live_bytes [h]`, `reserved [h]`.

A double free, a free of memory this heap did not allocate, a free with the wrong size or type, and a write after free (found when the block is reused) stop the program with a contract failure naming the check; freed memory reads as `0xDEADBEEF` words, and any pointer to a destroyed object fails with `Use after free` (generation checks)

<!-- api:heap -->
```text
module heap
    std/heap: a general-purpose allocator with per-object free, in the spirit
    of Zig's: nothing allocates from a heap unless it is handed one, and a
    free names what it frees (its type, or its size), so a wrong free is
    caught instead of corrupting memory.

      (let h:(ptr heap.Heap) (call heap.make))
      (let p:(ptr Point) (call (heap.create Point) h))     ;; zeroed, like (new Point)
      (call (heap.destroy Point) h p)
      (let xs:(arr i64) (call (heap.array i64) h 100))     ;; zeroed, like (arr.new i64 100)
      (call (heap.free_array i64) h xs)
      (call heap.live h)                                  ;; blocks not yet freed: 0

    Checked on every free, as contract failures that stop the program:
    freeing twice, freeing what this heap did not allocate, and freeing
    with a different size (or type) than was allocated. Freed memory is
    filled with a junk pattern, and checked when the block is handed out
    again: a write after free is reported then. Reading freed memory is not
    detected, but reads the pattern (-559038737 as an i32), not old data.

    How: a request is rounded up to a size class (8, 16, 24, then four per
    doubling: 32 40 48 56 64 80 96 112 128 ..., so at most a quarter is
    spare). A freed block goes on its class's free list and is reused for
    the next request of that class. New blocks are cut from chunks of at
    least 64 KiB taken with mem.alloc, which never gives memory back to the
    system: a heap's memory is reused, not returned. Each block has an
    8-byte header: which heap and state (live or freed) in one word, the
    size asked for in the other. Allocating and freeing take the heap's
    lock, so threads can share a heap.

struct heap.Heap [] and 8 private fields
    lock: held while the heap changes (first, so its address is the
    heap's); heads: each size class's first free block (0 for none); new
    blocks come from [cur, end).

fn heap.make [] -> (ptr heap.Heap)
    A new, empty heap.

fn heap.raw [h:(ptr heap.Heap) n:i32] -> i32
    n zeroed bytes, 8-aligned.
    (no_leak): the heap grows only when no freed block of the size is
    left to reuse.

fn heap.free_raw [h:(ptr heap.Heap) p:i32 n:i32] -> void (unsafe)
    Frees the n bytes at p, which raw returned for n bytes.
    (req (call not_freed_already h p))
    (req (call allocated_by h p))
    (req (call size_matches p n))

fn (heap.create T) [h:(ptr heap.Heap)] -> (ptr T) (where (struct T))
    A zeroed T.

fn (heap.destroy T) [h:(ptr heap.Heap) p:(ptr T)] -> void (where (struct T))
    Destroying p ends its generation (std/gen): p, and every copy of it,
    stops the program with "Use after free" if used again.

fn (heap.array T) [h:(ptr heap.Heap) n:i32] -> (arr T)
    n zeroed Ts, laid out like (arr.new T n): the generation word, then the
    length, before the first element.
    (req (gte n 0))

fn (heap.free_array T) [h:(ptr heap.Heap) a:(arr T)] -> void
    Frees a, which array made from h; any copy of a used afterwards stops
    the program with "Use after free".

fn heap.live [h:(ptr heap.Heap)] -> i32
    Blocks allocated and not yet freed (a test can require 0: no leaks).

fn heap.live_bytes [h:(ptr heap.Heap)] -> i32
    Bytes asked for in those blocks.

fn heap.reserved [h:(ptr heap.Heap)] -> i32
    Bytes the heap holds from mem.alloc.

fn heap.run_heap_tests [] -> i32
    Pass count; test_suite.aipl expects 8.
```
<!-- /api:heap -->

### std/html

`escape_to [o b quote:bool]` and `escape [s] -> str` (`& < >`, and with quote `" '` as `&quot;` and `&#x27;`: Python's `html.escape`); `unescape_to [o b]` and `unescape [s] -> str`: numeric references as Python maps them (the Windows-1252 range to its characters, surrogates and numbers past U+10FFFF to U+FFFD, controls and noncharacters to nothing), HTML 4's named ones with `&apos;` (259 names, each meaning what HTML5 gives it), and HTML5's legacy names without their `;`, the longest prefix winning (`&notit;` is `¬it;`); a name it does not know stays as it is (Python knows all 2,231 of HTML5's)

<!-- api:html -->
```text
module html
    std/html: text made safe to put in HTML, and character references read
    back, as Python's html module does.

      (call html.escape "<a href=\"x\">")   ;; "&lt;a href=&quot;x&quot;&gt;"
      (call html.unescape "caf&eacute; &#x263A;")

    unescape knows the numeric references (decimal and hex, with or
    without the ';', mapped as Python maps them: the Windows-1252 range to
    its characters, a surrogate or a number past U+10FFFF to U+FFFD) and
    HTML 4's named ones with &apos; (259 names); HTML5's legacy names
    (&amp, &copy, &eacute, ...) also without the ';', the longest such
    prefix of a longer name winning (&notit; is "¬it;"). A name it does not
    know stays as it is. Python knows all 2,231 of HTML5's names; the
    others are what differs (tests/aipl/html, checked against Python).

fn html.escape_to [o:(ptr buf.Buf) b:str.Bytes quote:bool] -> void
    Appends b with & < > written &amp; &lt; &gt;, and with quote also
    " and ' as &quot; and &#x27; (for attribute values), as Python's
    html.escape does.

fn html.escape [s:str] -> str
    s escaped for anywhere in HTML, quotes included.

fn html.unescape [s:str] -> str
    s with its character references replaced.

fn html.unescape_to [o:(ptr buf.Buf) b:str.Bytes] -> void
    Appends b with its character references replaced (b is UTF-8).
```
<!-- /api:html -->

### std/http

HTTP/1.1 messages (RFC 9112), and `fetch [a url:str limits] -> (result (ptr http.Message) (ptr http.Error))`: a GET of `http://HOST[:PORT][/path][?query]` or `https://...` (read by std/url; port 80 or 443 unless given; `Connection: close`, read to the end, redirects not followed) over the network, which the program must be granted (`--allow-connect`): plain http to an address anywhere, a name or `https://` in the VM, aipl-run, and bundles (a native executable says it has neither: `Error.no_host_support`);

`fetch_within [a url limits ms]` is `fetch` with the response to arrive within `ms` milliseconds of the request being sent (an `http.Error` with status 0 if it does not); a failure of the exchange itself (not granted, refused, a certificate not trusted, a response cut short, over the limits) is an `http.Error` with `status` 0 and its reason in `what`, and the `Message` holds the bytes it was read from (`free_message` gives them back).

Messages: `parse_request [a text limits] -> (result (ptr http.Message) (ptr http.Error))` and `parse_response` (`method`, `target`, `status`, `reason`, `version`, `headers` (a `(vec.Vec http.Header)` of `name` and `value` views, in order), `body`, `length`: the bytes the message took, as another may follow); `header [m name] -> (option.Option str.Bytes)` (ignoring case); `free_message`, `free_error`; an `http.Error` has `at`, `what`, `status` (the answer the refusal calls for: 400, 413 for a body over the limit, 431 for a head or header count over it), and `incomplete` (only cut short: more bytes could make a whole message); `default_limits` (64 KiB of head, 100 headers, 8 MiB of body) and `limits [head headers body]`.

Refused, as two readers could disagree about them: a space before a header's `:`, a folded header line, both Transfer-Encoding and Content-Length, two different Content-Lengths, a transfer coding other than a final `chunked`. Lines may end with CRLF or LF. A response with neither length header runs to the end of the text; 1xx, 204, and 304 have no body.

Writing: `start_request [o method target]`, `start_response [o status]`, `add_header [o name value]` (name a token, value without CR, LF, or NUL: contracts), `end_head [o body]` (Content-Length, the empty line, the body); `reason [status] -> str` (Python's `http.HTTPStatus` phrases), `mime_type [name] -> str` (by extension, from a fixed table of 31), `token`, `clean`

<!-- api:http -->
```text
module http
    std/http: HTTP/1.1 messages (RFC 9112): read a request or a response from
    bytes, and write one; and fetch a URL over a connection.

      (match_result (call http.parse_request a text (call http.default_limits))
        (ok m ... (get m http.Message.method) (call http.header m "host") ...)
        (err e ... (get e http.Error.what) ...))
      (call http.start_response o 200)
      (call http.add_header o "Content-Type" (call http.mime_type "page.html"))
      (call http.end_head o body)               ;; Content-Length, then body

    Reading is strict where leniency lets two readers disagree about where
    a message ends (request smuggling): a space before a header's ':', a
    folded header line, both Transfer-Encoding and Content-Length, two
    different Content-Lengths, or a transfer coding other than a final
    "chunked" is an error. Lines may end with CRLF or a bare LF (RFC 9112,
    2.2). Limits cap the head, the number of headers, and the body. A
    program can read a saved request, or one piped through stdin, and
    answer on stdout.

      (match_result (call http.fetch a "http://127.0.0.1:9000/stock?item=7" (call http.default_limits))
        (ok m ... (get m http.Message.status) (get m http.Message.body) ... (call http.free_message m))
        (err e ... (get e http.Error.what) ...))

    fetch is a GET over a connection (std/net), which the program must be granted
    (--allow-connect HOST:PORT): http:// to an address, or to a name and https:// where the host has
    a resolver and TLS (the VM, aipl-run, wasm bundles), or in a native executable through the helper
    aipl-net (std/net). It sends `Connection: close`, reads to the end, and does not follow
    redirects.

struct http.Error [(read at:i32) (read what:str) (read status:i32) (read incomplete:bool)]
    Why a message was refused, and the byte offset where; the response
    status the refusal calls for (400, or 413 for a body over the limit,
    431 for a head or header count over it); and whether the bytes were
    only cut short, so that more of them could make a whole message (a
    server reading a request a piece at a time reads on).

struct http.Header [(read name:str.Bytes) (read value:str.Bytes)] (value)
    A header as it came (name and value without the spaces around it).

struct http.Limits [(read head_bytes:i32) (read headers:i32) (read body_bytes:i32)] (value)
    The most a message may hold: bytes of head (the start line and the
    headers), headers, and bytes of body.

struct http.Message [(read is_request:bool) (read method:str.Bytes) (read target:str.Bytes) (read status:i32) (read reason:str.Bytes) (read version:str.Bytes) (read headers:(ptr (vec.Vec http.Header))) (read body:str.Bytes) (read length:i32)] and 5 private fields
    A message read: a request's method and target, or a response's status
    and reason; the version ("HTTP/1.1"); the headers in order; the body
    (chunked bodies put together); and length, the bytes it took (another
    message may follow). The views point into the text read, but a
    chunked body is the message's own (free_message frees it).

fn http.default_limits [] -> http.Limits
    64 KiB of head, 100 headers, 8 MiB of body.

fn http.limits [head_bytes:i32 headers:i32 body_bytes:i32] -> http.Limits
    Limits of your own.
    (req (and (gt head_bytes 0) (and (gte headers 0) (gte body_bytes 0))))

fn http.free_error [a:alloc.Allocator e:(ptr http.Error)] -> void
    Frees an Error parse_request or parse_response gave.

fn http.free_message [m:(ptr http.Message)] -> void
    Frees the message (its header list and any chunked body).

fn http.fetch [a:alloc.Allocator url:str lim:http.Limits] -> (result (ptr http.Message) (ptr http.Error))
    The response to a GET of `url` (http://HOST[:PORT][/path][?query] or https://..., read by std/url; the
    port 80 or 443 unless given), or why there is none: an Error with status 0 for a failure before or
    after the exchange (a URL that is not one this reads, a connection not granted, refused, or not
    supported here, a certificate not trusted, a response that is cut short, over the limits, or not
    HTTP) and the message's own refusal otherwise. The message holds the bytes it was read from:
    free_message gives them back.

fn http.fetch_within [a:alloc.Allocator url:str lim:http.Limits ms:i32] -> (result (ptr http.Message) (ptr http.Error))
    fetch, but the response must be read within `ms` milliseconds of the request being sent (-1: no limit); an
    Error otherwise (status 0, "the response did not arrive in time"). Connecting has no limit of its own.

fn http.parse_request [a:alloc.Allocator text:str.Bytes lim:http.Limits] -> (result (ptr http.Message) (ptr http.Error))
    The request in text (from its start), or why it is not one.

fn http.parse_response [a:alloc.Allocator text:str.Bytes lim:http.Limits] -> (result (ptr http.Message) (ptr http.Error))
    The response in text (from its start), or why it is not one. The body
    of a response with neither Content-Length nor Transfer-Encoding is the
    rest of text (it ends when the connection closes); a 1xx, 204, or 304
    response has none.

fn http.header [m:(ptr http.Message) name:str] -> (option.Option str.Bytes)
    The value of the first header named name (ignoring case), if any.

fn http.clean [s:str] -> bool
    Whether s can go in a start line or header as it is: no CR, LF, or NUL.

fn http.token [s:str] -> bool
    Whether s is a token (RFC 9110, 5.6.2): what header names and
    methods must be.

fn http.start_request [o:(ptr buf.Buf) method:str target:str] -> void
    Appends a request line, "METHOD target HTTP/1.1".
    (req (and (call token method) (call clean target)))

fn http.start_response [o:(ptr buf.Buf) status:i32] -> void
    Appends a status line, "HTTP/1.1 200 OK", with the status's reason.
    (req (and (gte status 100) (lte status 999)))

fn http.reason [status:i32] -> str
    The reason phrase for status (as Python's http.HTTPStatus has them),
    or "Unknown".
    @tables (tools/http_tables.py)

fn http.add_header [o:(ptr buf.Buf) name:str value:str] -> void
    Appends a header line, "Name: value".
    (req (and (call token name) (call clean value)))

fn http.end_head [o:(ptr buf.Buf) body:str.Bytes] -> void
    Appends Content-Length for body, the empty line, and body.

fn http.mime_type [name:str] -> str
    The content type for a file name, by its extension (ignoring case),
    from a fixed table of the common ones (text types with charset=utf-8);
    application/octet-stream for others.
```
<!-- /api:http -->

### std/io

`read_stdin [] -> (result str.Bytes io.Error)` (all of stdin), `write_str [fd s]`, `println [s]`, `eprintln [s]` (stderr), `read_file [path:str] -> (result str.Bytes io.Error)` (the whole file, in a buffer that is never freed), `read_fd [fd] -> (result str.Bytes io.Error)` (reads an open descriptor to its end, freeing each buffer it outgrows), `write_file [path:str b:str.Bytes] -> (result i32 io.Error)` (the bytes written), and `read_path` / `write_path`, the same for a path held as `str.Bytes`; `read_stdin` also gives a result (empty input is an empty view, not an error).

A failure is an `io.Error`, a value struct `(struct Error [kind:Kind errno:i32 path:str.Bytes] (value))` whose `kind` is `io.Kind` (`(enum Kind [not_found denied is_directory other exists not_empty])`), whose `errno` is the WASI errno and whose `path` is the path it happened to (empty for standard input; a view of the caller's path, which must outlive the error), with `message [e] -> str` for the kind's words (`"no such file or directory"`, `"permission denied"`, `"is a directory"`, `"file exists"`, `"directory not empty"`) `describe [e] -> str` for all of it (`"notes.txt: no such file or directory"`), and `error_of [code path] -> io.Error` for an `fs.*` op's failed result (section 4.C).

A program branches on the kind, `(eq (get e io.Error.kind) io.Kind.not_found)`, and a function that returns another error type converts with `(try r (ref io.describe))`.

<!-- api:io -->
```text
module io
    std/io: plain text out, and whole-file reads/writes over fs.*. fds 1 and
    2 are stdout and stderr in both the VM and under WASI. Numbers and values
    are printed with the format forms, (println "total {n}") (AIPL_SPEC.md 4.K). A read or write that
    fails gives an io.Error (a kind: not_found, denied, is_directory, other,
    exists, not_empty; the errno; the path), with io.message for the kind's
    words and io.describe for all of it.

fn io.write_str [fd:i32 s:str] -> void
    Writes s to file descriptor fd (1 is stdout, 2 stderr).

fn io.println [s:str] -> void
    Writes s and a newline to standard output.

fn io.eprintln [s:str] -> void
    Writes s and a newline to standard error.

enum io.Kind [not_found denied is_directory other exists not_empty]
    What went wrong, in one word.

struct io.Error [(read kind:io.Kind) (read errno:i32) (read path:str.Bytes)] (value)
    What went wrong reading or writing a file: the kind, the system's errno
    (a WASI errno, as AIPL_SPEC.md 4.C gives it; 0 when there is none), and
    the path it happened to (empty for standard input). The path is a view of
    the caller's, which must outlive the error. io.message says the kind in
    words; io.describe says all of it.

fn io.message [e:io.Error] -> str
    The kind of e in words: "no such file or directory".

fn io.describe [e:io.Error] -> str
    All of e, for a message: "notes.txt: no such file or directory".

fn io.error_of [code:i32 path:str.Bytes] -> io.Error
    The Error for a failed fs.* operation's result on `path`: minus a WASI
    errno (AIPL_SPEC.md 4.C), as every backend gives it. A missing file or
    directory on the path (ENOENT 44, ENOTDIR 54) is not_found; no access
    (EACCES 2, EPERM 63, outside what the program may reach ENOTCAPABLE 76)
    denied; EISDIR (31) is_directory; EEXIST (20) exists; ENOTEMPTY (55)
    not_empty; anything else other.

fn io.read_file [path:str] -> (result str.Bytes io.Error)
    The whole file, in a buffer that is never freed (read_fd), or why not.

fn io.read_path [path:str.Bytes] -> (result str.Bytes io.Error)
    read_file for a path held as bytes (e.g. one built at run time).

fn io.read_stdin [] -> (result str.Bytes io.Error)
    Everything on standard input, until end of input (empty input is an
    empty view, not an error), or why it could not be read.

fn io.read_fd [fd:i32] -> (result str.Bytes io.Error)
    Reads fd to its end into a buffer from the shared heap that doubles
    as it fills (each outgrown one freed); the view is checked. The last
    buffer is never freed: for a program that reads as it goes for a long
    time, use std/stream with an allocator of its own.

fn io.write_file [path:str b:str.Bytes] -> (result i32 io.Error)
    Creates or truncates the file and writes b: the bytes written, or why
    not.

fn io.write_path [path:str.Bytes b:str.Bytes] -> (result i32 io.Error)
    write_file for a path held as bytes.

fn io.run_io_tests [] -> i32
    Pass count; test_suite.aipl expects 2. Uses a scratch file in the cwd.
```
<!-- /api:io -->

### std/ip

IP and socket addresses from text, read as Rust's standard library reads them (tests/test_web.rs compares the two on generated texts), allocating nothing: `parse [t:str.Bytes] -> (option.Option ip.Addr)` (IPv4 as four numbers 0-255 without leading zeros; IPv6 with `::` and an IPv4 tail), `parse_endpoint [t] -> (option.Option ip.Endpoint)` (`a.b.c.d:port`, `[v6]:port`; a zone index such as `%eth0` refused), `len [a] -> i32` (4 or 16), `byte_at [a i] -> i32` (network order); `Endpoint.addr`, `Endpoint.port`, `Addr.v6`

<!-- api:ip -->
```text
module ip
    std/ip: IP addresses and socket addresses from text, as Rust's standard
    library reads them (core::net::parser, step for step; tests/test_web.rs
    checks the two agree on generated texts), so a grant on the command line
    (`--listen 127.0.0.1:8080`) means the same to every backend.

      (call ip.parse (call str.from_str "::1"))                 ;; => some, v6
      (call ip.parse_endpoint (call str.from_str "10.0.0.7:8080"))
      (call ip.byte_at a 0)                                       ;; => 10

    IPv4 is four decimal numbers of 0-255 without leading zeros (01 is
    refused, since some readers take it as octal); IPv6 is up to eight
    groups of one to four hex digits, one `::` standing for one or more
    groups of zeros, and an IPv4 address in the last two groups. An
    endpoint is `ADDR:PORT`, with an IPv6 address in brackets
    (`[::1]:8080`). A zone index (`%eth0`, `%3`) is refused. Nothing here
    allocates.

struct ip.Addr [(read v6:bool) (read hi:i64) (read lo:i64)] (value)
    An address: IPv4 (v6 false, its 32 bits the low half of lo) or IPv6,
    128 bits in network order (hi the first 8 bytes, lo the last 8).

struct ip.Endpoint [(read addr:ip.Addr) (read port:i32)] (value)
    An address and a port (0-65535).

fn ip.endpoint [a:ip.Addr port:i32] -> ip.Endpoint
    The endpoint for address a and a port (0-65535).

fn ip.len [a:ip.Addr] -> i32
    How many bytes the address has: 4 or 16.

fn ip.byte_at [a:ip.Addr i:i32] -> i32
    Byte i of the address, in network order.

fn ip.is_host_name [t:str.Bytes] -> bool
    Whether t is a host name as a program may give one (src/net.rs is_host_name): letters, digits, and -, in
    labels of 1 to 63 characters between dots (a final dot allowed), at most 253 bytes, no label starting or
    ending with -, and a last label that is not all digits.

fn ip.is_name_grant [g:str.Bytes] -> bool
    Whether `g` is HOST:PORT or HOST:* with a host name (not an address): a grant for the hosts that have a
    resolver, which a native executable has not.

fn ip.parse [t:str.Bytes] -> (option.Option ip.Addr)
    The address `t` is, all of it: IPv4 or IPv6 (Rust's IpAddr::from_str).

fn ip.parse_endpoint [t:str.Bytes] -> (option.Option ip.Endpoint)
    The endpoint `t` is, all of it: `a.b.c.d:port` or `[v6]:port` (Rust's
    SocketAddr::from_str, less zone indexes).

fn ip.run_ip_tests [] -> i32
    Pass count; test_suite.aipl expects 5.
```
<!-- /api:ip -->

### std/json

JSON (RFC 8259) to and from a `json.Value`, `(union Value [(null) (boolean b:bool) (number n:f64) (text s:str.Bytes) (list items:(ptr (vec.Vec Value))) (object fields:(ptr (vec.Vec (ptr json.Field))))])`. `parse [a:alloc.Allocator text:str.Bytes] -> (result json.Value (ptr json.Error))` (its error from a; `free_error [a e]`) is strict: valid UTF-8, no duplicate keys, nesting at most 512 deep, numbers that fit an `f64`, and an `Error` with the byte offset `at` and `what` went wrong.

`get [v key:str] -> (option.Option json.Value)` (an object's member), `at [v i]` (a list's element), `count [v]` (elements or members, 0 for other values); `get_path [v path:str] -> (option.Option Value)` (`"a.b[2].c"`: members after `.`, elements in brackets; `""` is v; path must pass `valid_path [path] -> bool`, and a key cannot hold `.` or `[`); `field [a key value] -> (ptr json.Field)` makes a member for building an object (`(read key)`, `(read value)`).

`write [b:(ptr buf.Buf) v]` writes the text JavaScript's `JSON.stringify` writes: no spaces, keys in their order, numbers as the shortest digits that read back as the same `f64` (`0.1`, `1e+21`, `5e-324`); writing an infinity or NaN fails its contract. `tests/test_std.rs` compares the numbers with Rust's shortest digits on 20,000 doubles

<!-- api:json -->
```text
module json
    std/json: JSON text to values and back (RFC 8259).

      (match_result (call json.parse (call alloc.default) text)
        (ok v (match (call json.get v "name") ...))
        (err e (println "bad JSON at {(get e json.Error.at)}")))
      (call json.write out v)                  ;; compact text into a buf.Buf

    Parsing is strict: UTF-8 text, the RFC's grammar exactly (no trailing
    commas, leading zeros, or comments), no key twice in one object, and no
    number too large for an f64; anything else is an Error with the byte
    offset where it was found. Writing gives what JavaScript's JSON.stringify
    gives: the shortest digits that read back as the same f64, laid out as
    JavaScript lays out numbers (0.1, 1e+21, 5e-324), strings escaped the same
    way. A number that is not finite has no JSON text: writing one stops the
    program (a contract), as JSON.stringify's null would hide the mistake.
    Values are union cells, which are never freed; their lists and text come
    from the allocator given to parse.

union json.Value [(null) (boolean b:bool) (number n:f64) (text s:str.Bytes) (list items:(ptr (vec.Vec json.Value))) (object fields:(ptr (vec.Vec (ptr json.Field))))]
    A JSON value: null, a boolean, a number, text, a list, or an object
    (its members in the order written).

struct json.Field [(read key:str.Bytes) (read value:json.Value)]
    A member of an object, in the order written.

struct json.Error [(read at:i32) (read what:str)]
    Where parsing stopped (a byte offset in the text) and why.

fn json.field [a:alloc.Allocator key:str.Bytes value:json.Value] -> (ptr json.Field)
    A member for building an object: key and value.

fn json.get [v:json.Value key:str] -> (option.Option json.Value)
    The value of member `key` of an object; none for any other value.

fn json.at [v:json.Value i:i32] -> (option.Option json.Value)
    Element i of a list; none past its end or for any other value.

fn json.valid_path [path:str] -> bool
    Whether path is one get_path reads: empty, or steps, each a key (any
    bytes but '.' and '[', after a '.' unless it is the first step) or an
    index in brackets: "a.b[2].c", "[0].name", "".

fn json.get_path [v:json.Value path:str] -> (option.Option json.Value)
    The value at path in v ("a.b[2].c": member a, then its member b, then
    that list's element 2, then its member c), or none if a step finds
    nothing; "" is v itself. A key cannot hold '.' or '[' (use get for
    those). path must be valid (valid_path).

fn json.count [v:json.Value] -> i32
    A list's elements or an object's members; 0 for any other value.

fn json.parse [a:alloc.Allocator text:str.Bytes] -> (result json.Value (ptr json.Error))
    Parses text as JSON (strict RFC 8259), or says where and why it is
    not JSON.
    An error, and the parser's own state, come from a (free an error with
    free_error).

fn json.free_error [a:alloc.Allocator e:(ptr json.Error)] -> void
    Frees an error parse returned, given the allocator parse was given.

fn json.write [b:(ptr buf.Buf) v:json.Value] -> void
    v as compact JSON text, appended to b.

fn json.run_json_tests [] -> i32
    Pass count; test_suite.aipl expects 6.
```
<!-- /api:json -->

### std/map

Generic hash map `(map.Map K V)` from keys of any type with a `cmp.Hash` implementation (`i32`, `i64`, `str`, and `str.Bytes` by its contents, from `std/cmp`; a module implements it for its own types): `(call (map.make K V) a capacity)` (an allocator, as for `vec`), `free`, `set [m k v]`, `get_or [m k default]`, `(map.add K) [m k d] -> i32` (for a `(Map K i32)`: adds `d` to k's count, 0 when absent, and returns the new count, in one lookup where `get_or` then `set` take two), `find [m k] -> (option.Option V)` (held by value when `V` fits, so a lookup allocates nothing), `has`, `remove -> bool`, `count`, each called as `(call (map.set K V) m k v)`; iterate with `(for i 0 (call (map.capacity K V) m) (if (call (map.slot_used K V) m i) ... (block)))` reading `slot_key` / `slot_val`.

A `str.Bytes` key is kept as the pointer, so its bytes must not change while it is stored

<!-- api:map -->
```text
module map
    std/map: hash map from keys of any type with a cmp.Hash implementation
    to values of any type (open addressing, linear probing, tombstones; grows
    at 75% load). Generic in both (AIPL_SPEC.md 4.H):
      (let m:(ptr (map.Map i32 str)) (call (map.make i32 str) (call alloc.default) 0))
      (call (map.set i32 str) m 7 "seven")
      (call (map.get_or i32 str) m 7 "")
      (call (map.add str) counts "word" 1)        ;; a count, in one lookup
      (match (call (map.find i32 i64) m 7)
        ((option.Option i64) some [v] ...) ((option.Option i64) none ...))
    Keys hash and compare through (cmp.Hash K): i32, i64, str, and
    str.Bytes (by contents) are implemented in std/cmp; a module
    implements it for its own key types. A str.Bytes key is kept as the
    pointer, so its bytes must not change while it is in the map.

struct (map.Map K V) [] and 9 private fields
    state per slot: 0 empty, 1 used, 2 deleted

fn (map.make K V) [a:alloc.Allocator capacity:i32] -> (ptr (map.Map K V)) (where (cmp.Hash K))
    An empty map with room for about `capacity` entries, from a.

fn (map.free K V) [m:(ptr (map.Map K V))] -> void (where (cmp.Hash K))
    Frees the table and the map itself; neither may be used afterwards.

fn (map.capacity K V) [m:(ptr (map.Map K V))] -> i32 (where (cmp.Hash K))
    The number of slots (for iterating with slot_used, slot_key, slot_val).

fn (map.count K V) [m:(ptr (map.Map K V))] -> i32 (where (cmp.Hash K))
    The number of entries.

fn (map.set K V) [m:(ptr (map.Map K V)) k:K v:V] -> void (where (cmp.Hash K))
    Adds k -> v, or replaces k's value.

fn (map.add K) [m:(ptr (map.Map K i32)) k:K d:i32] -> i32 (where (cmp.Hash K))
    Adds d to k's count (0 when k is absent) and returns the new count: one
    lookup, where get_or and then set take two (word counts, histograms).

fn (map.get_or K V) [m:(ptr (map.Map K V)) k:K default:V] -> V (where (cmp.Hash K))
    k's value, or `default` when k is absent.

fn (map.find K V) [m:(ptr (map.Map K V)) k:K] -> (option.Option V) (where (cmp.Hash K))
    The value for k, or none. An Option is a value when V fits in one
    (AIPL_SPEC.md 4.J: not a str, at most 7 scalars), so nothing is
    allocated for it.

fn (map.has K V) [m:(ptr (map.Map K V)) k:K] -> bool (where (cmp.Hash K))
    Whether k has an entry.

fn (map.remove K V) [m:(ptr (map.Map K V)) k:K] -> bool (where (cmp.Hash K))
    Removes k; false if it was absent.

fn (map.slot_used K V) [m:(ptr (map.Map K V)) i:i32] -> bool (where (cmp.Hash K))
    Iterate with (for i 0 (call (map.capacity K V) m) (if (call (map.slot_used K V) m i) ...)).

fn (map.slot_key K V) [m:(ptr (map.Map K V)) i:i32] -> K (where (cmp.Hash K))
    The key in slot i (slot_used must be true for it).

fn (map.slot_val K V) [m:(ptr (map.Map K V)) i:i32] -> V (where (cmp.Hash K))
    The value in slot i (slot_used must be true for it).

fn map.run_map_tests [] -> i32
    Pass count; test_suite.aipl expects 10.
```
<!-- /api:map -->

### std/math

The small arithmetic the language does not spell: `min`, `max`, `abs`, `sign`, `clamp`, `floor_div` and `floor_mod` (rounded toward negative infinity, as Python's `//` and `%`; the operators `/` and `%` round toward zero), `gcd` (never negative), `pow [x n]` (n not negative; stops the program on overflow, through `checked.mul`), `isqrt` (the largest r with r*r <= x), for `i32`, and the same with an `_i64` suffix (`pow_i64 [x:i64 n:i32]`);

For `f64`, `floor`, `ceil`, `trunc`, and `round` (halves away from zero, as C's round; a whole number, infinity, and not-a-number come back as they are), `min_f64`, `max_f64` (not a number when either is), `abs_f64`, `powi [x n:i32]` (by repeated multiplication, so within a few units of the last place of libm's pow, not always its last bit), `is_nan`; the constants `PI` and `TAU`.

`abs` of the smallest integer, a zero divisor, a negative `isqrt` argument, and `clamp` with lo above hi fail their contracts.

Also `exp`, `expm1`, `ln`, `log10`, `log2`, `log1p`, `pow_f64 [x y]` (a real exponent), `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2 [y x]`, `sinh`, `cosh`, `tanh`, `hypot`, `cbrt`, and `scalbn`: ports of fdlibm (FreeBSD's msun, as musl and Rust's `libm` crate carry it) written in AIPL with IEEE arithmetic, `f64.sqrt`, and bit operations, so every backend gives the same bits, and the same as `libm`'s on every generated input (a NaN is any NaN); within 1 ulp of glibc (2 for `tanh`); as in C a result that does not exist is NaN or an infinity, not a stop.

Checked against Python on generated cases in every backend (`tools/math_vectors.py`), and the transcendentals against `libm` bit for bit (`tests/test_math_float.rs`)

<!-- api:math -->
```text
module math
    std/math: the small arithmetic a program reaches for and the language does
    not spell: min, max, abs, clamp, the floor-rounding division and remainder
    of Python's // and %, gcd, integer powers and square roots, floor, ceil,
    trunc, and round for f64, and the transcendental functions (exp, ln,
    pow_f64, sin, cos, atan2, and the rest: ports of fdlibm). Integers: i32 as the plain names, i64 with an
    _i64 suffix; f64 with _f64 where the name would clash.

      (call math.max 3 7)                 ;; 7
      (call math.floor_div -7 2)          ;; -4 (the operator / gives -3)
      (call math.floor_mod -7 2)          ;; 1 (the operator % gives -1)
      (call math.gcd 12 18)               ;; 6
      (call math.isqrt 99)                ;; 9
      (call math.floor 2.7)               ;; 2.0
      (call math.ln 10.0)                 ;; 2.302585092994046
      (call math.pow_f64 2.0 0.5)         ;; 1.4142135623730951

    Where a result cannot be represented the program stops, as the rest of the
    language does: abs of the smallest integer, pow past the range, a division
    by zero (the f64 functions return NaN or an infinity, as C does). Not here:
    gcd or pow on big numbers (std/bigint).

const math.PI:f64 = 3.141592653589793
    The ratio of a circle's circumference to its diameter, as an f64.

const math.TAU:f64 = 6.283185307179586
    A full turn in radians, 2 * PI.

fn math.min [a:i32 b:i32] -> i32
    The smaller of two numbers.

fn math.max [a:i32 b:i32] -> i32
    The larger of two numbers.

fn math.abs [x:i32] -> i32
    x without its sign. The smallest i32 has no positive twin, so it stops
    the program.
    (req (neq x -2147483648))

fn math.sign [x:i32] -> i32
    -1, 0, or 1: the sign of x.

fn math.clamp [x:i32 lo:i32 hi:i32] -> i32
    x, held between lo and hi.
    (req (lte lo hi))

fn math.floor_div [a:i32 b:i32] -> i32
    a divided by b, rounded toward negative infinity, as Python's a // b:
    (floor_div -7 2) is -4, where (/ -7 2) is -3.
    (req (neq b 0))

fn math.floor_mod [a:i32 b:i32] -> i32
    What is left of a after floor_div: the sign of b (or 0), as Python's
    a % b: (floor_mod -7 2) is 1, where (% -7 2) is -1.
    (req (neq b 0))

fn math.gcd [a:i32 b:i32] -> i32
    The greatest common divisor, never negative; (gcd 0 0) is 0.

fn math.pow [x:i32 n:i32] -> i32
    x to the power n, n not negative: (pow 3 4) is 81, (pow x 0) is 1.
    Stops the program when the answer does not fit.
    (req (gte n 0))

fn math.isqrt [x:i32] -> i32
    The whole part of the square root of x, x not negative: the largest r
    with r * r <= x, so (isqrt 99) is 9 and (isqrt 100) is 10.
    (req (gte x 0))

fn math.min_i64 [a:i64 b:i64] -> i64
    The smaller of two numbers.

fn math.max_i64 [a:i64 b:i64] -> i64
    The larger of two numbers.

fn math.abs_i64 [x:i64] -> i64
    x without its sign; the smallest i64 stops the program.
    (req (neq x -9223372036854775808i64))

fn math.clamp_i64 [x:i64 lo:i64 hi:i64] -> i64
    x, held between lo and hi.
    (req (lte lo hi))

fn math.floor_div_i64 [a:i64 b:i64] -> i64
    a // b as Python writes it, for i64.
    (req (neq b 0i64))

fn math.floor_mod_i64 [a:i64 b:i64] -> i64
    a % b as Python writes it, for i64.
    (req (neq b 0i64))

fn math.gcd_i64 [a:i64 b:i64] -> i64
    The greatest common divisor, never negative.

fn math.pow_i64 [x:i64 n:i32] -> i64
    x to the power n, n not negative; stops the program when the answer
    does not fit.
    (req (gte n 0))

fn math.isqrt_i64 [x:i64] -> i64
    The whole part of the square root of x, x not negative.
    (req (gte x 0i64))

fn math.is_nan [x:f64] -> bool
    Whether x is not a number.

fn math.min_f64 [a:f64 b:f64] -> f64
    The smaller of two numbers; not a number when either is.

fn math.max_f64 [a:f64 b:f64] -> f64
    The larger of two numbers; not a number when either is.

fn math.abs_f64 [x:f64] -> f64
    x without its sign (including a negative zero's).

fn math.trunc [x:f64] -> f64
    x with its fraction dropped (rounded toward zero): (trunc -2.7) is -2.0.
    A zero result has no sign.

fn math.floor [x:f64] -> f64
    The largest whole number not above x: (floor 2.7) is 2.0, (floor -2.7)
    is -3.0.

fn math.ceil [x:f64] -> f64
    The smallest whole number not below x: (ceil 2.1) is 3.0, (ceil -2.1) is
    -2.0.

fn math.round [x:f64] -> f64
    x to the nearest whole number, halves away from zero as C's round:
    (round 2.5) is 3.0, (round -2.5) is -3.0, (round 0.4999999999999999) is 0.0.

fn math.powi [x:f64 n:i32] -> f64
    x to the power n, n a whole number, by repeated multiplication (a negative
    n divides): exact where every step is, otherwise within a few units of
    the last place, not always libm's last bit.
    (req (neq n -2147483648))

fn math.scalbn [x:f64 n0:i32] -> f64
    x times 2 to the n (exactly, if the result is a normal number; one rounding, if it is subnormal), as C's
    scalbn.

fn math.exp [x0:f64] -> f64
    e to the x (FreeBSD e_exp.c): x = k ln 2 + r, |r| <= ln 2 / 2, exp(r) from a degree-5 polynomial in r*r,
    scaled by 2^k. Under 1 ulp. exp(+inf) is +inf, exp(-inf) is 0, exp(NaN) is NaN.

fn math.ln [x:f64] -> f64
    The natural logarithm (FreeBSD e_log.c). ln(0) is -inf, ln of a negative number is NaN, ln(+inf) is +inf.

fn math.log2 [x:f64] -> f64
    The logarithm base 2 (FreeBSD e_log2.c), with the high and low parts of 1/ln 2 kept apart for accuracy.

fn math.log10 [x:f64] -> f64
    The logarithm base 10 (FreeBSD e_log10.c).

fn math.log1p [x:f64] -> f64
    log(1 + x), exact for small x (FreeBSD s_log1p.c). log1p(-1) is -inf; below -1 it is NaN.

fn math.expm1 [x0:f64] -> f64
    e^x - 1, exact for small x (FreeBSD s_expm1.c). expm1(-inf) is -1, expm1(+inf) is +inf.

fn math.sin [x:f64] -> f64
    The sine (FreeBSD s_sin.c).

fn math.cos [x:f64] -> f64
    The cosine (FreeBSD s_cos.c).

fn math.tan [x:f64] -> f64
    The tangent (FreeBSD s_tan.c).

fn math.atan [x0:f64] -> f64
    The arc tangent (FreeBSD s_atan.c): the argument brought into [-7/16, 7/16] by one of four identities,
    atan(x) = atanhi + atanlo + atan(reduced) from a degree-23 polynomial.

fn math.atan2 [y:f64 x:f64] -> f64
    The angle of the point (x, y) from the positive x axis, in (-pi, pi] (FreeBSD e_atan2.c): the quadrant by
    the signs, atan(|y / x|) inside it, and the answers C names for zeros, infinities, and NaN.

fn math.asin [x0:f64] -> f64
    The arc sine (FreeBSD e_asin.c); NaN outside [-1, 1].

fn math.acos [x:f64] -> f64
    The arc cosine (FreeBSD e_acos.c); NaN outside [-1, 1].

fn math.sinh [x:f64] -> f64
    The hyperbolic sine (FreeBSD e_sinh.c), from expm1.

fn math.cosh [x0:f64] -> f64
    The hyperbolic cosine (FreeBSD e_cosh.c).

fn math.tanh [x0:f64] -> f64
    The hyperbolic tangent (FreeBSD s_tanh.c).

fn math.hypot [x0:f64 y0:f64] -> f64
    sqrt(x*x + y*y) without overflow or underflow, and with an error under 1 ulp (musl's hypot): the
    squares as exact double sums, scaled by 2^700 where the arguments are very large or very small.

fn math.cbrt [x:f64] -> f64
    The cube root (FreeBSD s_cbrt.c): a bit-twiddled guess good to 5 bits, a polynomial to 23, one Newton
    step to 53; error under 0.667 ulp (not always the correctly rounded value).

fn math.pow_f64 [x:f64 y:f64] -> f64
    x to the power y (FreeBSD e_pow.c): log2(x) as a sum of two doubles, times y as another, then 2^ of
    that: error under 0.7 ulp. Every case C names: pow(x, 0) and pow(1, y) are 1 (NaN included), a negative
    x with a y that is not a whole number is NaN, the signs of zeros and infinities follow the sign of y and
    the parity of a whole y.
```
<!-- /api:math -->

### std/net

TCP sockets over `aipl_net_v1`, in every backend (docs/design/NET_PLAN.md): `listen [a at:str.Bytes] -> (result net.Socket net.Error)` (ADDR:PORT; only where `--listen` granted, else `Error.not_granted`), `accept [l] -> (result net.Socket net.Error)` (waits), `recv [c into:(ptr buf.Buf) max] -> i32` (added to `into`; 0 at the end, -1 on an error), `send [c b:str.Bytes] -> bool` (all of it), `connect [a at:str.Bytes] -> (result net.Socket net.Error)` and `connect_to [a e:ip.Endpoint]` (a connection to an address, only where `--allow-connect ADDR:PORT` granted it, else `Error.connect_not_granted`), `connect_host [a host:str.Bytes port]` (to a host name, or an address as text, granted by that name: `--allow-connect example.com:443`; `Error.no_such_host` for a name that does not resolve. A native executable has no resolver and no TLS: its `connect_host` answers -3 and std/net starts `aipl-net`, a helper installed beside `aipl` and found on `$PATH` (or at `$AIPL_NET`), which does both and relays the bytes; `Error.no_helper` if it is not found or the executable was not compiled with a name grant. The compiler puts the executable's `--allow-connect` grants after the program as a trailer, and `aipl-net` reads them from its parent's file, so a program that starts the helper itself reaches no host it was not granted), `connect_tls [a host port]` (`connect_host`, then TLS with the certificate checked against `host`: `Error.tls_failed`; the same hosts), `close [s]`, `reader [a c] -> (ptr stream.Reader)` and `writer [a c] -> (ptr stream.Writer)` (std/stream over the connection; closing them leaves it open), `message [e] -> str`.

Waiting a limited time (every call above otherwise blocks): `poll [s events ms] -> i32`, `readable [s ms] -> bool` (a `recv` or `accept` would not wait: data, the end, an error, a connection pending), `writable`, `accept_within [l ms] -> (result net.Socket net.Error)` (`Error.timed_out` if none came), `recv_within [c into max ms] -> i32` (-2 if nothing came); `connect` has no limit of its own. A `Socket`'s handle is private, so a socket never reaches std/fs. All of it keeps no memory (`(no_leak)`); one connection at a time for now

<!-- api:net -->
```text
module net
    std/net: TCP sockets (docs/design/NET_PLAN.md), over AIPL's own host
    interface, aipl_net_v1: the same in the VM, under aipl-run, and in
    native executables.

      (let l:net.Socket (try (call net.listen a (call str.from_str "127.0.0.1:8080"))))
      (let c:net.Socket (try (call net.accept l)))
      (let r:(ptr stream.Reader) (call net.reader a c))
      (let w:(ptr stream.Writer) (call net.writer a c))
      ... read lines from r, write to w ...
      (let _ok:bool (call stream.close_writer w))   ;; flushes
      (call stream.close_reader r)
      (call net.close c)

      (let c:net.Socket (try (call net.connect a (call str.from_str "127.0.0.1:9000"))))
      ... the same reader and writer, then (call net.close c) ...

    A program may listen only where it was granted: `aipl eval --listen
    127.0.0.1:8080 prog.aipl`, `aipl-run --listen ...`, or, fixed in a native
    executable, `aipl compile --exe --listen ...`; elsewhere listen gives
    Error.not_granted. It may connect only to a host granted with
    `--allow-connect HOST:PORT` (HOST:* for any port), the same way; HOST is an address, or, for
    connect_host and connect_tls, a name (granted by that name). Addresses are read by std/ip, as Rust reads them.
    A Socket's handle is private to this module, so a socket cannot be given
    to std/fs, or a file to it. Every call blocks until it is done; one
    connection at a time for now (a pool of threads later). Nothing here
    keeps memory: the address bytes listen needs come from its allocator
    and go back to it, and a reader's or writer's memory comes from the
    allocator it is made with.

struct net.Socket [] (value) and 2 private fields
    A listening socket or a connection. h is the host's handle, or -1 for a connection made by aipl-net (host
    names and TLS in a native executable, below), which `helper` is the program of.

enum net.Error [not_an_address not_granted refused connect_not_granted no_host_support tls_failed no_such_host timed_out no_helper]
    Why a call failed.

fn net.message [e:net.Error] -> str
    What went wrong, for a message.

fn net.listen [a:alloc.Allocator at:str.Bytes] -> (result net.Socket net.Error)
    A socket listening on `at` (ADDR:PORT), with SO_REUSEADDR and a
    backlog of 128.

fn net.connect [a:alloc.Allocator at:str.Bytes] -> (result net.Socket net.Error)
    A connection to `at` (ADDR:PORT), if the program was granted it (--allow-connect); waits
    for the system's answer.

fn net.connect_host [a:alloc.Allocator host:str.Bytes port:i32] -> (result net.Socket net.Error)
    A connection to the host `host` (a name, or an address as text) on `port`, if the program was granted
    it (`--allow-connect HOST:PORT`); the system's resolver finds the address. In a native executable, which
    has no resolver, aipl-net makes the connection (see helper_connect).

fn net.connect_tls [a:alloc.Allocator host:str.Bytes port:i32] -> (result net.Socket net.Error)
    A secure connection (TLS) to `host` on `port`: connect_host, then the handshake, in which the server's
    certificate must be trusted (the Mozilla roots, and the file AIPL_CA_FILE names) and carry the name.
    Everything sent and received after is plain bytes.

fn net.connect_to [a:alloc.Allocator e:ip.Endpoint] -> (result net.Socket net.Error)
    A connection to the endpoint e.

fn net.accept [l:net.Socket] -> (result net.Socket net.Error)
    The next connection on listening socket l (waiting for one).

fn net.recv [c:net.Socket into:(ptr buf.Buf) max:i32] -> i32
    Up to max bytes from connection c, added to `into`: how many, 0 at the
    end of the connection, or -1 on an error.

fn net.send [c:net.Socket b:str.Bytes] -> bool
    Sends all of b on connection c; false on an error (the peer gone, say).

fn net.poll [s:net.Socket events:i32 ms:i32] -> i32
    poll's answer for events 1 (read) or 2 (write): the events if ready, 0 if the time ran out, -1 on an error.

fn net.readable [s:net.Socket ms:i32] -> bool
    Whether s can be read within `ms` milliseconds.

fn net.writable [s:net.Socket ms:i32] -> bool
    Whether s can be written within `ms` milliseconds.

fn net.accept_within [l:net.Socket ms:i32] -> (result net.Socket net.Error)
    accept, but for at most `ms` milliseconds: Error.timed_out if no connection came.

fn net.recv_within [c:net.Socket into:(ptr buf.Buf) max:i32 ms:i32] -> i32
    recv, but for at most `ms` milliseconds: -2 if nothing came, else recv's answer (bytes, 0 at the end, -1).

fn net.close [s:net.Socket] -> void
    Closes a socket (a connection is shut down first).

fn net.reader [a:alloc.Allocator c:net.Socket] -> (ptr stream.Reader)
    A reader of connection c (std/stream: by lines, bytes, or all of it),
    its window from a. close_reader leaves c open.

fn net.writer [a:alloc.Allocator c:net.Socket] -> (ptr stream.Writer)
    A writer to connection c, holding what it is given until 8 KB, flush,
    or close_writer; close_writer leaves c open.
```
<!-- /api:net -->

### std/option

`(union (option.Option T) [(some value:T) (none)])`, a value that may be missing (section 4.J): `(make (option.Option i32) some 5)`, `(make (option.Option i32) none)`; `(call (option.is_some T) o)`, `is_none`, `(call (option.unwrap_or T) o fallback)`

<!-- api:option -->
```text
module option
    std/option: a value that may be missing, for any type T.

      (let o:(option.Option i32) (make (option.Option i32) some 5))
      (let n:(option.Option i32) (make (option.Option i32) none))
      (match o
        ((option.Option i32) some [v] v)
        ((option.Option i32) none 0))
      (call (option.unwrap_or i32) n 0)       ;; => 0

    Use it instead of a sentinel (-1, a null pointer) when "nothing" is a
    possible answer: the checker makes every match handle none.

union (option.Option T) [(some value:T) (none)]
    A value of type T (some), or nothing (none).

fn (option.is_some T) [o:(option.Option T)] -> bool
    Whether o holds a value.

fn (option.is_none T) [o:(option.Option T)] -> bool
    Whether o is none.

fn (option.unwrap_or T) [o:(option.Option T) fallback:T] -> T
    The value, or `fallback` for none.

fn option.run_option_tests [] -> i32
    Pass count; test_suite.aipl expects 7.
```
<!-- /api:option -->

### std/os

The command line and environment: `arg_count [] -> i32` (argv[0], the program, included), `arg [i] -> (option.Option str.Bytes)` (`none` past the end), `arg_int [i default] -> i32` (argument `i` as a number, or `default` if it is missing or not one: `(call os.arg_int 1 1000)`), `env [name:str] -> (option.Option str.Bytes)` (`none` if unset; an empty value is `some` of an empty view), each table read once and kept, so they allocate nothing after the first call and are `(no_leak)`; `random_i32 [] -> i32` from the OS generator

<!-- api:os -->
```text
module os
    std/os: the program's command line and environment, over the args.* and
    env.* ops (WASI args_get / environ_get layout: a table of u32 pointers to
    NUL-terminated strings). The same in the VM, where `aipl eval FILE -- a b`
    supplies the arguments, and compiled, where the WASI host does
    (`wasmtime run --invoke main prog.wasm a b`; environment variables only
    when forwarded, e.g. `--env AIPL_PATH`). Each table is read once, on
    first use, and kept (neither changes while the program runs), so arg
    and env allocate nothing after the first call: a (no_leak) function may
    use them.

fn os.arg_count [] -> i32
    Number of command-line arguments, argv[0] (the program) included.

fn os.arg [i:i32] -> (option.Option str.Bytes)
    Argument i (0 is the program), or none when there is no argument i.

fn os.env [name:str] -> (option.Option str.Bytes)
    The value of environment variable `name`, or none when it is not set.

fn os.arg_int [i:i32 default:i32] -> i32
    Argument i as a decimal i32, or `default` if there is no argument i or
    it is not a number: (call os.arg_int 1 1000).

fn os.random_i32 [] -> i32
    A random i32 from the operating system's generator (sys.random).

fn os.run_os_tests [] -> i32
    Pass count; expects 3. A variable that is surely unset, an argument
    index past the end, and arg_int's default (past the end, and argument
    0, the program's name, which is not a number).
```
<!-- /api:os -->

### std/par

Parallel work with no `unsafe` (GENERATIONS_PLAN.md, "Data races"): `(call (par.map T U C) out f ctx xs threads)` sets `out[i]` to `(call_ref f ctx xs' i)` for every `i` of `out` (`f:(fn [C (arr T) i32] -> U)`), split into `threads` contiguous parts, one on the calling thread and each other on its own thread, each with its own copy `xs'` of `xs`; returns when all are done.

`(call (par.reduce T U C) f join ctx xs n init threads) -> U` folds the indices `0 .. n-1` the same way: each part starts from `init` and sets `acc` to `(call_ref f ctx xs' i acc)` for each of its indices in order (`f:(fn [C (arr T) i32 U] -> U)`), and the parts' results are joined in order with `join:(fn [U U] -> U)`; with an associative `join` and `init` its unit, the answer is the sequential one (with floats it can depend on `threads`, never on timing). `T`, `U`, and `C` must be plain (`(where (plain T))`, section 4.H), so no worker can reach memory another writes.

A program using it is a threaded module

<!-- api:par -->
```text
module par
    std/par: running code in parallel with no unsafe threads
    (docs/design/GENERATIONS_PLAN.md, "Data races"). (par.map T U C) sets
    out[i] to (f ctx xs i) for every i, on several threads; (par.reduce T U
    C) folds every index into one result, part by part, and joins the parts
    in order. Nothing a worker can reach is written by another while it
    runs: the context and the results are plain ((where (plain T)): no
    pointer or array inside, so each worker has its own copy), each part
    gets its own copy of the input array (free for the worker to read fast,
    or even change), and this module writes each result to its own element
    of out (map) or its own part's record (reduce). So no data race is
    possible, and the caller writes no unsafe:

      (struct Ctx [n:i32 scale:f64] (value))
      (fn row [c:Ctx xs:(arr f64) i:i32] -> f64 ...)
      (call (par.map f64 f64 Ctx) out (ref row) (make Ctx n 2.0) xs 4)
      (fn add [c:Ctx xs:(arr f64) i:i32 acc:f64] -> f64 (+ acc (arr.get f64 xs i)))
      (fn plus [a:f64 b:f64] -> f64 (+ a b))
      (call (par.reduce f64 f64 Ctx) (ref add) (ref plus) (make Ctx n 2.0) xs n 0.0 4)

    Copying the input costs one pass over it per part, small beside work
    that reads the whole input for each result (a matrix-vector product),
    large beside work that reads one element (there, one thread is as
    fast). Workers may allocate (alloc.default is a heap with a lock; new
    and arr.new are atomic) and print. The caller waits until every part is
    done. A program that uses par is a threaded module (docs/IMPLEMENTATION.md 3):
    it runs in the VM, under aipl-run, and natively.

fn (par.map T U C) [out:(arr U) f:(fn [C (arr T) i32] -> U) ctx:C xs:(arr T) threads:i32] -> void (where (plain T)) (where (plain U)) (where (plain C))
    out[i] = (f ctx xs i) for every i of out, in `threads` parts (at least
    1, at most out's length): one on the calling thread, each other on a
    thread of its own, each with its own copy of xs. Returns when all are
    done; the copies are freed.

fn (par.reduce T U C) [f:(fn [C (arr T) i32 U] -> U) join:(fn [U U] -> U) ctx:C xs:(arr T) n:i32 init:U threads:i32] -> U (where (plain T)) (where (plain U)) (where (plain C))
    The indices 0 .. n-1 folded in `threads` parts (at least 1, at most n):
    each part starts from `init` and sets acc to (f ctx xs i acc) for each
    of its indices in order, each on its own thread with its own copy of
    xs; then the parts' results are joined in order, (join (join p0 p1)
    p2) .... So with `init` a unit of an associative `join` (0 and +), the
    answer is the sequential one; with floats, whose + rounds, it can
    depend on `threads` (never on timing: the same threads give the same
    answer).
    (req (gte n 0))

fn par.run_par_tests [] -> i32
    Pass count; test_suite.aipl expects 8.
```
<!-- /api:par -->

### std/path

Paths as text, as Python's `posixpath` treats them (no file system access): `join [a b]` (b if it is absolute), `parent` (`dirname`), `file_name` (`basename`), `stem` and `extension` (`splitext`: `".gz"` for `b.tar.gz`, none for `.bashrc`), `normalize` (`normpath`: `"."` and repeated slashes out, `".."` taking the part before it, `"."` for the empty path), `is_absolute`; each takes and gives `str`. Checked against Python on 1,600 generated paths (400 kept, tests/aipl/libs)

<!-- api:path -->
```text
module path
    std/path: file paths as text ('/'-separated), as Python's posixpath
    treats them; nothing here touches the file system.

      (call path.join "data" "in.csv")         ;; "data/in.csv"
      (call path.parent "data/in.csv")         ;; "data"
      (call path.extension "a/b.tar.gz")       ;; ".gz"
      (call path.normalize "a/./b/../c")       ;; "a/c"

    Results are new strs (never freed, as every run-time str). Checked
    against Python's posixpath on generated paths (tests/aipl/libs).

fn path.is_absolute [p:str] -> bool
    Whether p starts at the root ('/').

fn path.join [a:str b:str] -> str
    b after a: b itself if it is absolute (posixpath.join).

fn path.file_name [p:str] -> str
    What follows the last '/' (posixpath.basename): "" for "a/".

fn path.parent [p:str] -> str
    What comes before the last '/', without the slashes ending it unless
    it is all slashes (posixpath.dirname): "a" for "a/b", "/" for "/a",
    "" for "a".

fn path.extension [p:str] -> str
    The file name's extension with its dot, or "" (posixpath.splitext):
    ".gz" for "b.tar.gz", "" for ".bashrc".

fn path.stem [p:str] -> str
    The file name without its extension: "b.tar" for "a/b.tar.gz".

fn path.normalize [p:str] -> str
    p with "." parts and repeated slashes left out and each ".." taking
    the part before it (posixpath.normpath): "a/c" for "a/./b/../c", ".."
    for "a/../..", "/" for "/..", "." for "".
```
<!-- /api:path -->

### std/proc

Starting other programs, over `aipl_sys_v1` (docs/design/SYS_PLAN.md), in every backend; only what `--allow-run` granted (else `Error.not_granted`).

`command [a path:str] -> (ptr proc.Command)` (and `command_path` for bytes; the path is given to the system as written, and is the program's own name, unless `command_named [a path name]` gives another), `arg c "text"` and `arg_bytes` add arguments, `free_command`; `run [c input:str.Bytes out:(ptr buf.Buf)] -> (result i32 proc.Error)` runs it to the end (an empty input is the null device; everything it writes, its error too, is added to `out`; the result is its status);

`spawn [c] -> (result proc.Child proc.Error)` starts it with a pipe to its input and one from its output and error together, `spawn_with c input output error` with each stream a `proc.Stream` (`inherit`, `pipe`, `null`, or for the error `merged`); on a `Child`: `write ch b -> bool`, `close_input ch`, `read ch into max -> i32` (like `net.recv`) and `read_error`, `wait ch -> i32`, `close ch`; `exec [c] -> proc.Error` replaces this program (it returns only to say why not).

A status is the exit status 0-255, or 256 plus the signal (`exited s`, `signal s`). A program that does not end: `kill ch sig -> bool` (a signal to a program this one started and has not waited for; the portable numbers are `SIGHUP`, `SIGINT`, `SIGQUIT`, `SIGKILL`, `SIGTERM`), `terminate ch` (SIGTERM), `try_wait ch -> (option.Option i32)` (the status, or none while it runs), `wait_timeout ch ns -> (option.Option i32)`, `stop ch grace_ns -> i32` (SIGTERM, up to the grace period, then SIGKILL; the status); one thread may kill what another waits for, but two should not wait for one program.

Limited waits: `readable ch ms`, `readable_error ch ms`, `read_within ch into max ms -> i32` (-2 if nothing came), and `run_within [c input out ns] -> (result i32 proc.Error)`: `run` within a limit as a whole (the input sent a pipe's worth at a time, the output read, the end waited for), killing the program and giving `Error.timed_out` at the limit, with what it wrote kept in `out`. `find [name:str out:(ptr buf.Buf)] -> bool` looks a name up in the PATH. `message [e]` words an error; a program that writes more than a pipe holds before it reads can wait for this one while this one waits for it

<!-- api:proc -->
```text
module proc
    std/proc: starting other programs (docs/design/SYS_PLAN.md), over AIPL's
    own host interface, aipl_sys_v1: the same in the VM, under aipl-run, and
    in native executables.

      (let c:(ptr proc.Command) (call proc.command (call alloc.default) "/bin/sh"))
      (call proc.arg c "-c")
      (call proc.arg c "echo hello; exit 3")
      (let out:(ptr buf.Buf) (call buf.make (call alloc.default) 64))
      (let status:i32 (try (call proc.run c (call str.from_str "") out)))   ;; 3
      ;; (call buf.bytes out) is "hello\n"

    A program may start only what it was granted: `aipl eval --allow-run
    /bin/sh prog.aipl`, `aipl-run --allow-run ...`, or, fixed in an executable,
    `aipl compile --exe --allow-run ...` (`*` grants any program); elsewhere a
    start gives Error.not_granted. The path is given to the system exactly as
    written: use `find` to look a name up in the PATH. The program has this
    program's working directory and environment, and `--sandbox` confines this
    program's files, not the program it starts.

    run is the whole of it for a program whose output fits in memory. For a
    conversation, spawn gives a Child with a pipe each way: write to it, read
    what it says, close_input to tell it there is no more, wait for its end.
    A program that writes more than a pipe holds (64 KiB) before it reads what
    it was sent can wait for this one to read while this one waits for it to
    read: send it little, or read first. A Child's handles are private to this
    module, so they never reach std/fs or std/net.
    A program's status is its exit status, 0 to 255, or 256 plus the number of
    the signal that ended it (exited and signal tell them apart).

    A program that does not end is dealt with by kill (a signal; only a program this one started, and has not
    waited for), try_wait (its status if it has ended, else none, without waiting), and wait_timeout, which
    waits a limited time. stop does the usual thing: ask it to end (SIGTERM), give it a grace period, then
    make it (SIGKILL):

      (match (call proc.wait_timeout ch 5000000000i64)       ;; five seconds
        ((option.Option i32) some [status] ...)               ;; it ended
        ((option.Option i32) none (call proc.stop ch 1000000000i64)))

    Two threads should not wait for one program at once (one may take the other's status); a thread may kill
    what another is waiting for.

struct proc.Command [] and 4 private fields
    A program to start, and its arguments: the program's name (argv[0], the
    path too unless command_named gave another) first, each argument after
    it, each followed by a zero byte; and the path it is started from. Made by
    command, added to by arg and arg_bytes, started by run, spawn,
    spawn_with, or exec.

struct proc.Child [] (value) and 4 private fields
    A started program and the pipes to it (-1 where a stream is not a pipe).

fn proc.none [] -> proc.Child
    A Child that is no program: all its handles are -1 (a Socket that is not a helper's holds one).

enum proc.Stream [inherit pipe null merged]
    What a program's standard input, output, or error is: this program's own
    (inherit), a pipe to it (pipe), the null device (null), or, for the error
    only, the same as its output (merged).

enum proc.Error [not_granted not_found denied other timed_out]
    Why a call failed.

fn proc.message [e:proc.Error] -> str
    What went wrong, for a message.

fn proc.error_of [code:i32] -> proc.Error
    The Error for a failed call's result: minus a WASI errno, as every
    backend gives it (-76 not granted, -44 or -54 no such program, -2 or -63
    no access, anything else other).

fn proc.command [a:alloc.Allocator path:str] -> (ptr proc.Command)
    The program at `path` (given to the system as it is written), with no
    arguments yet; its memory comes from a.

fn proc.command_path [a:alloc.Allocator path:str.Bytes] -> (ptr proc.Command)
    command for a path held as bytes.

fn proc.command_named [a:alloc.Allocator path:str.Bytes name:str.Bytes] -> (ptr proc.Command)
    command for a program that is to be given another name than its path
    (argv[0]: what it says when it reports a failure, as `x.wasm: ...` for
    a program built from x.wasm).

fn proc.arg [c:(ptr proc.Command) s:str] -> void
    Adds an argument.

fn proc.arg_bytes [c:(ptr proc.Command) s:str.Bytes] -> void
    Adds an argument held as bytes (it must hold no zero byte).
    (req (lt (call (option.unwrap_or i32) (call str.find_byte s 0) -1) 0))

fn proc.free_command [c:(ptr proc.Command)] -> void
    Gives back what command, arg, and arg_bytes took from the allocator.

fn proc.spawn_with [c:(ptr proc.Command) input:proc.Stream output:proc.Stream error:proc.Stream] -> (result proc.Child proc.Error)
    Starts the program with the streams `input`, `output`, and `error`
    (Stream.merged is for the error only: for the others the system refuses
    it, as Error.other).

fn proc.spawn [c:(ptr proc.Command)] -> (result proc.Child proc.Error)
    Starts the program with a pipe to its input, and one from its output
    and its error together: the way to talk to it.

fn proc.exec [c:(ptr proc.Command)] -> proc.Error
    Replaces this program with the program: it does not return when that
    works, so what it gives is why it did not.

fn proc.write [ch:proc.Child b:str.Bytes] -> bool
    Sends all of b to the program's input; false on an error (the program
    gone, or its input not a pipe).

fn proc.close_input [ch:proc.Child] -> void
    Closes the pipe to the program's input, which tells it there is no more.

fn proc.read [ch:proc.Child into:(ptr buf.Buf) max:i32] -> i32
    Up to max bytes of what the program writes (its error too, when spawn
    merged it), added to `into`: how many (it waits for some), 0 at the end,
    or minus an errno on an error.

fn proc.read_error [ch:proc.Child into:(ptr buf.Buf) max:i32] -> i32
    read for the program's error, where spawn_with made it a pipe of its own.

fn proc.wait [ch:proc.Child] -> i32
    Waits for the program to end: its status (below), the same on a second wait.

fn proc.close [ch:proc.Child] -> void
    Closes the pipes and lets go of the program (which goes on running if it
    has not ended: wait first).

fn proc.exited [status:i32] -> bool
    Whether a status is an exit status (0 to 255), not a signal's.

fn proc.signal [status:i32] -> i32
    The signal that ended the program, or 0 if it exited by itself.

const proc.SIGHUP:i32 = 1
    The signals every system numbers alike (for others, the system's own number goes to kill).
    The terminal line was closed.

const proc.SIGINT:i32 = 2
    Interrupt, as ctrl-C.

const proc.SIGQUIT:i32 = 3
    Quit, as ctrl-backslash.

const proc.SIGKILL:i32 = 9
    End the program at once: it cannot catch or ignore this.

const proc.SIGTERM:i32 = 15
    Ask the program to end: it may clean up first, or ignore this.

fn proc.kill [ch:proc.Child sig:i32] -> bool
    Sends signal `sig` (1 to 64) to the program: true if it was sent. It is not sent to a program already
    waited for (its id may be another program's now), and the program must be one this program started.

fn proc.terminate [ch:proc.Child] -> bool
    Asks the program to end: SIGTERM (it may handle the signal and clean up, or ignore it).

fn proc.try_wait [ch:proc.Child] -> (option.Option i32)
    The program's status if it has ended (as wait gives it), else none: it does not wait.

fn proc.wait_timeout [ch:proc.Child ns:i64] -> (option.Option i32)
    Waits for the program to end, for at most `ns` nanoseconds: its status, or none if it is still running
    then (it is left running: kill or stop it). The program is looked at every millisecond at first, then
    less often, up to every 25 ms.

fn proc.stop [ch:proc.Child grace:i64] -> i32
    Ends the program: SIGTERM, up to `grace` nanoseconds for it to end, then SIGKILL; its status
    (256 + the signal, if the signal ended it), or -1 if it could not be waited for.

fn proc.readable [ch:proc.Child ms:i32] -> bool
    Whether the program's output can be read within `ms` milliseconds: data, or its end (the program gone
    or its output closed).

fn proc.readable_error [ch:proc.Child ms:i32] -> bool
    The same for its error, when that is a pipe of its own.

fn proc.read_within [ch:proc.Child into:(ptr buf.Buf) max:i32 ms:i32] -> i32
    read, for at most `ms` milliseconds: -2 if nothing came, else read's answer (bytes, 0 at the end).

fn proc.run_within [c:(ptr proc.Command) input:str.Bytes out:(ptr buf.Buf) ns:i64] -> (result i32 proc.Error)
    run, but the whole of it (the program's input sent, its output read, its end) within `ns` nanoseconds:
    the program is killed (SIGKILL) and the result is Error.timed_out if it is not over by then; `out` holds
    what it had written. A program that does not read its input, or does not end, cannot hold this one up.

fn proc.run [c:(ptr proc.Command) input:str.Bytes out:(ptr buf.Buf)] -> (result i32 proc.Error)
    Runs the program to its end: `input` sent to it (nothing for an empty
    one: its input is the null device), and everything it writes, its error
    too, added to `out`. Its status, or why it did not start.

fn proc.find [name:str out:(ptr buf.Buf)] -> bool
    Looks `name` up in the directories of the PATH, in order, and adds the
    first path to a file of that name to `out`: true if there was one. A name
    with a slash in it is a path already: true if a file is there. Whether it
    may be run is for the system to say when it is started.
```
<!-- /api:proc -->

### std/random

Random numbers from PCG-32 (pcg-c-basic's pcg32), so a seed gives the same numbers everywhere: `seeded [a seed:i64] -> (ptr random.Rng)` (stream 54, as pcg32's demo: seed 42 gives `a15c02b7 7b47f409 ba1d3330 ...`), `seeded_stream [a seed seq]`, `seeded_from_os [a]`, `free`; `next_u32 -> i32` (32 bits), `next_u64`, `below [r bound] -> i32` (0 to bound - 1, unbiased: pcg32_boundedrand), `range [r lo hi]` (hi left out), `f64` (in [0, 1), a multiple of 2^-53), `coin`, `normal [r mean sd]` (Marsaglia's polar method, over a port of fdlibm's log), `(shuffle T) [r xs]` (Fisher-Yates, in place), `(choice T) [r xs]`, `(sample T) [a r xs k]` (k without repeats).

Not for secrets

<!-- api:random -->
```text
module random
    std/random: random numbers from a seeded generator (PCG-32, O'Neill's
    pcg32 from pcg-c-basic), so a seed gives the same numbers in every
    backend, every run, on every machine.

      (let r:(ptr random.Rng) (call random.seeded a 42i64))
      (call random.range r 1 7)          ;; a die: 1 to 6
      (call random.f64 r)                ;; in [0, 1)
      (call (random.shuffle i32) r xs)

    seeded_from_os seeds from the system's random bytes instead. Not for
    secrets: PCG-32 is fast and well distributed, not cryptographic.

struct random.Rng [] and 3 private fields
    The generator's state and stream (pcg32_random_t).

fn random.seeded_stream [a:alloc.Allocator seed:i64 seq:i64] -> (ptr random.Rng)
    A generator from a seed, on the stream `seq` (pcg32_srandom_r).

fn random.seeded [a:alloc.Allocator seed:i64] -> (ptr random.Rng)
    A generator from a seed (stream 54, as pcg32's demo).

fn random.seeded_from_os [a:alloc.Allocator] -> (ptr random.Rng)
    A generator seeded from the system's random bytes.

fn random.free [r:(ptr random.Rng)] -> void
    Frees the generator.

fn random.next_u32 [r:(ptr random.Rng)] -> i32
    The next 32 random bits (pcg32_random_r), as an i32.

fn random.next_u64 [r:(ptr random.Rng)] -> i64
    The next 64 random bits: two draws, the first the high half.

fn random.below [r:(ptr random.Rng) bound:i32] -> i32
    A number from 0 to bound - 1 (bound an unsigned 32-bit count, not 0),
    every one equally likely (pcg32_boundedrand_r: draws again below the
    threshold rather than taking a biased remainder).
    (req (neq bound 0))

fn random.range [r:(ptr random.Rng) lo:i32 hi:i32] -> i32
    An i32 from lo up to hi, hi left out.
    (req (lt lo hi))

fn random.f64 [r:(ptr random.Rng)] -> f64
    An f64 in [0, 1): 53 random bits, each value a multiple of 2^-53.

fn random.coin [r:(ptr random.Rng)] -> bool
    true or false, equally likely.

fn random.normal [r:(ptr random.Rng) mean:f64 sd:f64] -> f64
    A normally distributed f64 (Marsaglia's polar method).

fn (random.shuffle T) [r:(ptr random.Rng) xs:(arr T)] -> void
    Shuffles xs in place, every order equally likely (Fisher-Yates).

fn (random.choice T) [r:(ptr random.Rng) xs:(arr T)] -> T
    One of xs, each equally likely.
    (req (gt (arr.len xs) 0))

fn (random.sample T) [a:alloc.Allocator r:(ptr random.Rng) xs:(arr T) k:i32] -> (arr T)
    k of xs, without repeats, in random order, as a new array from a.
    (req (and (gte k 0) (lte k (arr.len xs))))
```
<!-- /api:random -->

### std/regex

Regular expressions of the guaranteed-linear kind (a Pike VM: time in proportion to pattern size times text length, whatever the pattern): `compile [a pattern:str] -> (result (ptr regex.Regex) (ptr regex.Error))` (`Error.at`, a byte offset, and `Error.what`; `free_error [a e]`), `free`, `groups`, `group_index [re name] -> i32` (-1 if none);

Over text as `str.Bytes`, spans `regex.Span` (`start`, `end`, byte offsets): `is_match`, `find -> (option.Option regex.Span)`, `find_at [re text start]`, `captures [a re text] -> (option.Option (ptr regex.Captures))` with `group [c i] -> (option.Option regex.Span)` and `free_captures`, `find_all [a re text] -> (ptr (vec.Vec regex.Span))` (Python's `finditer` rule for empty matches), `replace_all [re text template] -> (result str (ptr regex.Error))` (`$1`, `${1}`, `$name`, `${name}`, `$0`, `$$`; a group the pattern lacks is an error), `split [a re text]` (the text between matches; unlike Python's, without the groups).

Syntax: literals, escapes, `.`, classes, `\d \w \s` and negations (ASCII), `^ $ \A \Z \b \B`, groups (capturing, `(?:...)`, `(?P<name>...)`), `|`, `* + ? {m} {m,} {,n} {m,n}` greedy and lazy, flags `(?i) (?m) (?s)` at the start. Matches are Python's `re` with `re.ASCII` (leftmost, as a backtracking matcher prefers; `$` also before a final newline); `.` and classes match UTF-8 characters. Lookaround, backreferences, possessive quantifiers, scoped flags, and counts over 1000 are errors. A Regex is for one thread at a time

<!-- api:regex -->
```text
module regex
    std/regex: regular expressions of the guaranteed-linear kind (as Go's
    and RE2's): a pattern compiles to a program for a Pike virtual machine,
    which follows every way of matching at once, so a search takes time in
    proportion to the pattern's size times the text's length, whatever the
    pattern. No backreferences and no lookaround: those are what make some
    patterns take exponential time elsewhere.

      (match_result (call regex.compile a "(?P<key>\\w+)=(\\d+)")
        (ok re ... (call regex.find re text) ...)
        (err e ... (get e regex.Error.at) (get e regex.Error.what) ...))

    Syntax: literals and escapes (\n \t \r \f \v \a \0 \xHH \uHHHH and any
    punctuation), `.`, classes `[a-z]` and `[^...]`, \d \w \s and \D \W \S,
    anchors ^ $ \A \Z \b \B, groups (capturing, `(?:...)`, `(?P<name>...)`),
    `|`, the quantifiers * + ? {m} {m,} {,n} {m,n} and their lazy forms
    (a `?` after), and flags at the start: (?i) (ASCII letters match either
    case), (?m) (^ and $ at every line), (?s) (`.` matches a newline too).
    Matches are what Python's re gives with re.ASCII: the leftmost match,
    and of those the one a backtracking matcher finds first; $ (without m)
    matches at the end and before a newline that ends the text, as in
    Python. \d \w \s \b are ASCII, as in RE2. The text is UTF-8: `.` and
    classes match a character (a byte that is not UTF-8 matches as U+FFFD),
    and spans are byte offsets. Checked against Python's re on generated
    patterns and texts (tests/aipl/regex).

    A Regex holds the machine's working lists, so one Regex searches on one
    thread at a time.

struct regex.Error [(read at:i32) (read what:str)]
    Why a pattern or a template was refused, and the byte offset where.

struct regex.Span [(read start:i32) (read end:i32)] (value)
    A match's or a group's bytes: from start up to end, end left out.

struct regex.Regex [] and 34 private fields
    A compiled pattern.

struct regex.Captures [] and 4 private fields
    A match's groups: spans[2i] and spans[2i + 1] are group i's start and
    end, -1 if it took no part (group 0 is the whole match).

fn regex.compile [a:alloc.Allocator pattern:str] -> (result (ptr regex.Regex) (ptr regex.Error))
    The program for pattern, from a, or why and where pattern is refused.

fn regex.free [re:(ptr regex.Regex)] -> void
    Frees the regex (and the Error of a failed compile: free_error).

fn regex.free_error [a:alloc.Allocator e:(ptr regex.Error)] -> void
    Frees an Error that compile or replace_all gave.

fn regex.groups [re:(ptr regex.Regex)] -> i32
    The number of capturing groups (not counting the whole match).

fn regex.group_index [re:(ptr regex.Regex) name:str] -> i32
    The number of the group named name, or -1 if there is none.

fn regex.is_match [re:(ptr regex.Regex) text:str.Bytes] -> bool
    Whether text has a match.

fn regex.find [re:(ptr regex.Regex) text:str.Bytes] -> (option.Option regex.Span)
    The leftmost match, if any.

fn regex.find_at [re:(ptr regex.Regex) text:str.Bytes start:i32] -> (option.Option regex.Span)
    The leftmost match starting at byte start or later (^, \b, and the
    like still see the text before start).
    (req (and (gte start 0) (lte start (get text str.Bytes.len))))

fn regex.captures [a:alloc.Allocator re:(ptr regex.Regex) text:str.Bytes] -> (option.Option (ptr regex.Captures))
    The leftmost match's groups, from a (free_captures), if there is a
    match.

fn regex.group [c:(ptr regex.Captures) i:i32] -> (option.Option regex.Span)
    Group i's span (0: the whole match), or none if it took no part.
    (req (and (gte i 0) (lt i (get c Captures.n))))

fn regex.free_captures [c:(ptr regex.Captures)] -> void
    Frees the captures.

fn regex.find_all [a:alloc.Allocator re:(ptr regex.Regex) text:str.Bytes] -> (ptr (vec.Vec regex.Span))
    Every match, left to right, not overlapping, as a new vector from a
    (Python's re.finditer: an empty match may follow a match, and the
    match after an empty one is not empty at the same place).

fn regex.split [a:alloc.Allocator re:(ptr regex.Regex) text:str.Bytes] -> (ptr (vec.Vec str.Bytes))
    The text between the matches (as find_all finds them), as slices of
    text in a new vector from a. (Python's re.split also gives each
    group's text between them; this does not.)

fn regex.replace_all [re:(ptr regex.Regex) text:str.Bytes template:str] -> (result str (ptr regex.Error))
    text with each match (as find_all finds them) replaced by template,
    in which $1 (or ${1}) is group 1's text, $name (or ${name}) a named
    group's, $0 the match's, and $$ a '$'; a group that took no part is
    empty. A template naming no group the pattern has is an error.
```
<!-- /api:regex -->

### std/serve

A long-running HTTP/1.1 server over `net` and `http` (docs/design/NET_PLAN.md, mode A): `run [l:net.Socket handler options] -> serve.Stats` with a handler `(fn [alloc.Allocator (ptr http.Message) (ptr buf.Buf)] -> void (no_leak))` given the request's arena, the request, and the buffer its whole response goes in (`http.start_response`, `add_header`, `end_head`); `(run_with C) [l ctx handler options]` for state kept between requests, the handler taking `ctx` first.

One connection at a time, one request each; each request on an arena reset after it (memory stays flat: tests/test_serve.rs serves 100,000 requests natively and under aipl-run, a million with AIPL_MILLION=1, with the heap where it was after the first thousand); a request std/http refuses answered with the error's status and reason; a view kept past its request stops the program with "Use after free" on its next use. `options` (http's default limits, 64 KiB arena chunks, no end), `requests [o n]` (return after n answers), `limits [o lim]`; `Stats.served`, `Stats.arena_bytes`.

A trap in a handler ends the server. Mode B, a fresh instance per request: `once [handler options] -> i32` reads one request from standard input and writes its answer to standard output (a refusal too), 0, or 1 if the input ended before a request began; `(once_with C) [ctx handler options]`. Run under `aipl serve --fresh` or as an executable built with `aipl compile --exe --serve ADDR:PORT`, where a trap answers 500 and the next request is served; an answer that cannot all be written gives 1; and a saved request is tried with `aipl eval prog.aipl < request.txt` in any backend

<!-- api:serve -->
```text
module serve
    std/serve: a long-running HTTP/1.1 server (docs/design/NET_PLAN.md,
    mode A) over std/net and std/http: it accepts a connection, reads one
    request, runs your handler, writes its response, and closes the
    connection, one at a time.

      (fn handle [a:alloc.Allocator req:(ptr http.Message) out:(ptr buf.Buf)] -> void (no_leak)
        (call http.start_response out 200)
        (call http.add_header out "Content-Type" "text/plain; charset=utf-8")
        (call http.end_head out (get req http.Message.target)))

      (match_result (call net.listen (call alloc.default) (call str.from_str "127.0.0.1:8080"))
        (ok l (let s:serve.Stats (call serve.run l (ref handle) (call serve.options))) ...)
        (err e ...))

    Each request is read into, and handled on, an arena that is reset after
    it: `a` is that arena, the request and every view of it point into it,
    and the handler writes its whole response to `out` (std/http's
    start_response, add_header, end_head). State kept between requests (a
    counter, a cache) goes in a context of the program's own, from its own
    allocator: (call (serve.run_with C) l ctx (ref handle) o), the handler
    taking ctx first, as AIPL's _with functions do (AIPL_SPEC.md 4.G). The handler's type promises
    (no_leak), and so does everything here on a request's path, which the
    checker proves: a server's memory stays flat however long it runs. A
    pointer kept past its request reads freed memory, which the generation
    checks stop ("Use after free") on its next use.

    Each call of run makes its own request arena, which the program keeps
    until it ends (a server calls run once).

    A request std/http refuses is answered with the status the refusal
    calls for (400, 413, or 431) and its reason; a connection that closes
    before a whole request is closed without an answer. A trap in a handler
    ends the server (mode A; NET_PLAN.md section 2).

struct serve.Options [(read limits:http.Limits) (read arena_bytes:i32) (read requests:i32)] (value)
    How a server runs: the limits on a request (std/http), the size of the
    request arena's chunks, and how many requests to serve before run
    returns (-1: no end).

fn serve.options [] -> serve.Options
    std/http's default limits, 64 KiB arena chunks, and no end.

fn serve.requests [o:serve.Options n:i32] -> serve.Options
    o, serving n requests and then returning.

fn serve.limits [o:serve.Options lim:http.Limits] -> serve.Options
    o with these request limits.

struct serve.Stats [(read served:i32) (read arena_bytes:i32)] (value)
    What run did: the requests answered (the handler's and the refusals),
    and the bytes the request arena holds at the end (its high-water mark:
    resetting it keeps its chunks for the next request).

fn serve.run [l:net.Socket handler:(fn [alloc.Allocator (ptr http.Message) (ptr buf.Buf)] -> void (no_leak)) o:serve.Options] -> serve.Stats
    Serves on listening socket l with handler until o's request count is
    reached (never, by default).

fn (serve.run_with C) [l:net.Socket ctx:C handler:(fn [C alloc.Allocator (ptr http.Message) (ptr buf.Buf)] -> void (no_leak)) o:serve.Options] -> serve.Stats
    run, with a context the handler is given first, for state kept
    between requests.

fn serve.once [handler:(fn [alloc.Allocator (ptr http.Message) (ptr buf.Buf)] -> void (no_leak)) o:serve.Options] -> i32
    One request, read from standard input and answered on standard
    output: the program a host starts afresh for each connection
    (aipl-run --serve-fresh, or an executable made with `aipl compile
    --exe --serve`), the connection as its input and the client getting
    its output. Nothing outlives the request, and a trap costs only it:
    the host answers 500. Also how a saved request is tried: `aipl eval
    prog.aipl < request.txt`. 0 if it answered (a refusal too); 1 if the
    input ended before a request began, or the answer could not all be
    written (so a host never sends part of one as if it were whole).

fn (serve.once_with C) [ctx:C handler:(fn [C alloc.Allocator (ptr http.Message) (ptr buf.Buf)] -> void (no_leak)) o:serve.Options] -> i32
    once, with a context the handler is given first.
```
<!-- /api:serve -->

### std/set

A hash set `(set.Set K)` of any type with a `cmp.Hash` implementation, over std/map: `(call (set.make K) a capacity)`, `free`, `add -> bool` (whether it was new), `has`, `remove -> bool`, `count`, `items [a s] -> (ptr (vec.Vec K))` (in slot order, the same in every backend), `union`, `intersection`, `difference` (each a new set from a), `is_subset`; each named with its type, `(call (set.add str) s "x")`

<!-- api:set -->
```text
module set
    std/set: a hash set of any type with a cmp.Hash implementation (i32,
    i64, str, and str.Bytes by its contents come with std/cmp).

      (let s:(ptr (set.Set str)) (call (set.make str) a 16))
      (let _new:bool (call (set.add str) s "apple"))
      (when (call (set.has str) s "apple") ...)

    Built on std/map; its memory comes from the allocator it is made with,
    and free gives it back. Items come out in the order of the slots they
    hash to: the same in every backend, but not the order added.

struct (set.Set K) [] and 3 private fields
    A set of K: a map from each item to true.

fn (set.make K) [a:alloc.Allocator capacity:i32] -> (ptr (set.Set K)) (where (cmp.Hash K))
    An empty set with room for about `capacity` items, from a.

fn (set.free K) [s:(ptr (set.Set K))] -> void (where (cmp.Hash K))
    Frees the set.

fn (set.add K) [s:(ptr (set.Set K)) k:K] -> bool (where (cmp.Hash K))
    Adds k; whether it was not there before.

fn (set.has K) [s:(ptr (set.Set K)) k:K] -> bool (where (cmp.Hash K))
    Whether k is in the set.

fn (set.remove K) [s:(ptr (set.Set K)) k:K] -> bool (where (cmp.Hash K))
    Takes k out; whether it was there.

fn (set.count K) [s:(ptr (set.Set K))] -> i32 (where (cmp.Hash K))
    The number of items.

fn (set.items K) [a:alloc.Allocator s:(ptr (set.Set K))] -> (ptr (vec.Vec K)) (where (cmp.Hash K))
    The items, in slot order, as a new vector from a.

fn (set.union K) [a:alloc.Allocator s:(ptr (set.Set K)) t:(ptr (set.Set K))] -> (ptr (set.Set K)) (where (cmp.Hash K))
    The items of s and of t, as a new set from a.

fn (set.intersection K) [a:alloc.Allocator s:(ptr (set.Set K)) t:(ptr (set.Set K))] -> (ptr (set.Set K)) (where (cmp.Hash K))
    The items in both s and t, as a new set from a.

fn (set.difference K) [a:alloc.Allocator s:(ptr (set.Set K)) t:(ptr (set.Set K))] -> (ptr (set.Set K)) (where (cmp.Hash K))
    The items of s not in t, as a new set from a.

fn (set.is_subset K) [s:(ptr (set.Set K)) t:(ptr (set.Set K))] -> bool (where (cmp.Hash K))
    Whether every item of s is in t.
```
<!-- /api:set -->

### std/show

Values as text, as the formatting forms (section 4.K) print them; each function takes a buffer and returns it, so calls chain: `start [] -> (ptr buf.Buf)`, `lit [b s:str]`, `put_i32 [b v spec]`, `put_i64`, `put_f32`, `put_f64` (the fewest digits that read back, as Python's repr; or exact decimals), `put_bool`, `put_str`, `put_bytes`, then `done [b] -> str`, `print [b]`, `println [b]`, `eprintln [b]` (each frees b), or `end_to [b]`; `debug_addr [b label addr]` writes a pointer or array inside a value shown by a generated printer (`null`, or `label @ addr`).

`spec [width decimals align zero base plus fill] -> i32` packs a format (the word's layout is in the module's header)

<!-- api:show -->
```text
module show
    std/show: values as text, for the formatting forms (AIPL_SPEC.md 4.K):
    (println "x = {x}") becomes calls to these, one per piece, each taking
    and returning the buffer, so they chain:

      (call show.println (call show.put_i32 (call show.lit (call show.start) "x = ") x 0))

    They are ordinary functions, usable directly. A spec word says how to
    write a value (the compiler packs it from the text after the colon in
    {x:spec}):

      bits 0-11   width, 0-4095 (0: as wide as the value)
      bits 12-17  decimals plus one, 0-41 (0: none given)
      bits 18-19  alignment: 0 the type's default (numbers right, the rest
                  left), 1 '<', 2 '>', 3 '^'
      bit 20      '0': pad numbers with zeros, after the sign
      bits 21-22  base: 0 decimal, 1 'x', 2 'X', 3 'b'
      bit 23      '+': a sign on numbers that are not negative
      bits 24-30  the fill byte (0: a space)

    Floats are written as Python's repr writes them: the fewest digits that
    read back as the same value (an f32's read back as AIPL reads an f32
    literal: to the nearest f64, then to the nearest f32), plainly when the
    point is within 16 places, else with an exponent (0.1, 100.0, 1e+16,
    1.5e-07, inf, nan); with decimals, exactly rounded to that many places,
    ties to even. Width counts UTF-8 characters, as Python does. Nothing here
    keeps memory except what a finished str needs (show.done).

fn show.start [] -> (ptr buf.Buf)
    A new buffer to format into (the start of (format ...) and (println ...)).

fn show.lit [b:(ptr buf.Buf) s:str] -> (ptr buf.Buf)
    Appends the text s to b; returns b.

fn show.done [b:(ptr buf.Buf)] -> str
    The text as a new str (never freed, as every run-time str); b is freed.

fn show.end_to [b:(ptr buf.Buf)] -> void
    The end of (format_to b "..."): the text stays in b.

fn show.debug_addr [b:(ptr buf.Buf) label:str addr:i32] -> (ptr buf.Buf)
    A pointer or array inside a value being shown (formats.rs
    debug_functions): null, or its type and address.

fn show.print [b:(ptr buf.Buf)] -> void
    Writes b to standard output and frees it.

fn show.println [b:(ptr buf.Buf)] -> void
    Writes b and a newline to standard output and frees it.

fn show.eprintln [b:(ptr buf.Buf)] -> void
    Writes b and a newline to standard error and frees it.

fn show.spec [width:i32 decimals:i32 align:i32 zero:bool base:i32 plus:bool fill:i32] -> i32
    A spec word from its parts (align 0-3, base 0-3, fill 0 for a space),
    for callers writing one by hand.
    (req (and (and (gte width 0) (lte width 4095)) (and (gte decimals -1) (lte decimals 40))))
    (req (and (and (gte align 0) (lte align 3)) (and (and (gte base 0) (lte base 3)) (and (gte fill 0) (lte fill 127)))))

fn show.put_str [b:(ptr buf.Buf) v:str s:i32] -> (ptr buf.Buf)
    Appends v under spec s (a word from spec); returns b.

fn show.put_bytes [b:(ptr buf.Buf) v:str.Bytes s:i32] -> (ptr buf.Buf)
    Appends the bytes of v under spec s; returns b.

fn show.put_bool [b:(ptr buf.Buf) v:bool s:i32] -> (ptr buf.Buf)
    Appends true or false under spec s; returns b.

fn show.put_i32 [b:(ptr buf.Buf) v:i32 s:i32] -> (ptr buf.Buf)
    Appends v in decimal (or the spec's base) under spec s; returns b.

fn show.put_i64 [b:(ptr buf.Buf) v:i64 s:i32] -> (ptr buf.Buf)
    Appends v in decimal (or the spec's base) under spec s; returns b.

fn show.put_f64 [b:(ptr buf.Buf) v:f64 s:i32] -> (ptr buf.Buf)
    Appends v, as Python's repr or with the spec's decimals; returns b.

fn show.put_f32 [b:(ptr buf.Buf) v:f32 s:i32] -> (ptr buf.Buf)
    Appends v, its shortest digits as an f32, or with the spec's
    decimals; returns b.

fn show.run_show_tests [] -> i32
    Pass count; test_suite.aipl expects 7.
```
<!-- /api:show -->

### std/stats

Statistics over `(arr f64)`, computed as Python's `statistics` module and `math.fsum` compute them, to the last bit: `sum` (`fsum`: the exact sum, rounded once), `fmean` (`fsum / n`), `mean` (the exact mean, rounded once), `median`, `quantiles [a xs n inclusive] -> (arr f64)` (the n - 1 cut points; Python's default `exclusive` method, or `inclusive`), `min`, `max`, `mode` (the most common; of equally common, the first to appear), `variance` and `pvariance` (the exact sample or population variance, rounded once), `stdev` and `pstdev` (the square root of the exact one, rounded once), `covariance`, `correlation`, and `linear_fit [xs ys] -> stats.Fit` (`slope`, `intercept`, as `statistics.linear_regression`, and `r2`), over `fsum` and `math.sumprod`'s triple-length accumulation;

Plus `zscore [x mean sd]` and `histogram [a xs lo hi bins] -> (arr i32)` (equal intervals, `hi` in the last).

The exact values are held as expansions (Shewchuk's partials, Dekker's products) and the roundings found by exact sign tests, so a square beyond about 1e300 overflows where Python's fractions would not. Too little data fails the function's contract, where Python raises `StatisticsError`. Checked against Python on about 44,000 values (300 data sets kept, tests/aipl/stats)

<!-- api:stats -->
```text
module stats
    std/stats: statistics over (arr f64), computed as Python's statistics
    module computes them, so the answers are Python's to the last bit:

      (call stats.mean xs)        ;; statistics.mean: the exact mean, rounded once
      (call stats.stdev xs)       ;; statistics.stdev: the exact one, rounded once
      (call stats.median xs)      (call stats.quantiles xs 4 false)

    sum is math.fsum (the exact sum, rounded once) and fmean statistics.fmean
    (fsum / n). mean, variance, pvariance, stdev, and pstdev round an exact
    rational once, as Python's do with fractions; here the exact values are
    held as expansions (sums of f64s that are exact, Shewchuk's partials) and
    the rounding found by exact sign tests, so for |x| beyond about 1e150 a
    square can overflow. covariance, correlation, and linear_fit follow
    Python's formulas over fsum and math.sumprod (triple-length
    accumulation, an exact product standing in for fma). Too little data
    (an empty array; one value for a sample statistic) fails the function's
    contract, as Python raises StatisticsError. Checked against Python on
    generated data (tests/aipl/stats, tools/stats_vectors.py).

fn stats.sum [xs:(arr f64)] -> f64
    math.fsum: the exact sum, rounded once.

fn stats.fmean [xs:(arr f64)] -> f64
    statistics.fmean: fsum / n.
    (req (gt (arr.len xs) 0))

fn stats.mean [xs:(arr f64)] -> f64
    statistics.mean: the exact mean, rounded once.
    (req (gt (arr.len xs) 0))

fn stats.variance [xs:(arr f64)] -> f64
    statistics.variance: the sample variance, exact then rounded once.
    (req (gt (arr.len xs) 1))

fn stats.pvariance [xs:(arr f64)] -> f64
    statistics.pvariance: the population variance.
    (req (gt (arr.len xs) 0))

fn stats.stdev [xs:(arr f64)] -> f64
    statistics.stdev: the square root of the exact sample variance,
    rounded once.
    (req (gt (arr.len xs) 1))

fn stats.pstdev [xs:(arr f64)] -> f64
    statistics.pstdev: the population standard deviation.
    (req (gt (arr.len xs) 0))

fn stats.median [xs:(arr f64)] -> f64
    statistics.median: the middle value, or the mean of the two middle ones.
    (req (gt (arr.len xs) 0))

fn stats.quantiles [a:alloc.Allocator xs:(arr f64) n:i32 inclusive:bool] -> (arr f64)
    statistics.quantiles: the n - 1 cut points dividing xs into n groups,
    by Python's 'exclusive' method (its default), or 'inclusive'.
    (req (and (gt n 0) (gt (arr.len xs) 0)))

fn stats.min [xs:(arr f64)] -> f64
    The least value (the first of equal ones), as Python's min.
    (req (gt (arr.len xs) 0))

fn stats.max [xs:(arr f64)] -> f64
    The greatest value (the first of equal ones), as Python's max.
    (req (gt (arr.len xs) 0))

fn stats.mode [xs:(arr f64)] -> f64
    statistics.mode: the most common value; of equally common ones, the
    first to appear (as written there first).
    (req (gt (arr.len xs) 0))

fn stats.covariance [xs:(arr f64) ys:(arr f64)] -> f64
    statistics.covariance: the sample covariance of xs and ys.
    (req (and (eq (arr.len xs) (arr.len ys)) (gt (arr.len xs) 1)))

fn stats.correlation [xs:(arr f64) ys:(arr f64)] -> f64
    statistics.correlation: Pearson's coefficient of xs and ys.
    (req (and (eq (arr.len xs) (arr.len ys)) (gt (arr.len xs) 1)))

struct stats.Fit [(read slope:f64) (read intercept:f64) (read r2:f64)] (value)
    A least-squares line, y = slope * x + intercept (statistics.
    linear_regression), and r2, the correlation squared.

fn stats.linear_fit [xs:(arr f64) ys:(arr f64)] -> stats.Fit
    The least-squares line through (xs[k], ys[k]).
    (req (and (eq (arr.len xs) (arr.len ys)) (gt (arr.len xs) 1)))

fn stats.zscore [x:f64 mean_:f64 sd:f64] -> f64
    How far x is from the mean, in standard deviations.
    (req (gt sd 0.0))

fn stats.histogram [a:alloc.Allocator xs:(arr f64) lo:f64 hi:f64 bins:i32] -> (arr i32)
    How many of xs fall in each of `bins` equal intervals from lo to hi
    (hi in the last; values outside left out).
    (req (and (gt bins 0) (lt lo hi)))
```
<!-- /api:stats -->

### std/str

`(struct Bytes [data:(arr u8) start:i32 len:i32] (value))`, a byte slice (`len` -1 marks a failed read): a checked view reads `data` from `start`, so a freed array stops the program when read; `bytes [addr len] -> Bytes` (unsafe to call) makes an unchecked view of raw memory (no array: for memory that is never freed, or that the caller vouches for); `of_array [a start len]` a checked one; `addr [b] -> i32` (its first byte's address, for `mem.*`, after checking the array), `checked [b] -> bool`, `absent [] -> Bytes` and `present [b] -> bool` (the `len -1` view where text may be missing);

The fields are read-only outside `str`, so no other module can make a view;

`from_str [s:str] -> Bytes` (a literal or run-time `str`, viewed in place), `is_at [b i s:str] -> bool` (whether b holds s at index i; allocates nothing), `byte_at`, `is_space [c] -> bool` (space and `\t \n \v \f \r`), `bytes_eq [a b] -> bool`, `find_byte [b c] -> (option.Option i32)` (first index, or none), `count_byte`, `count_lines` (newlines plus an unterminated last line), `count_words` (runs of non-space bytes), `parse_int [b] -> (result i32 i32)` (`(ok n)`, or `(err i)` with the index of the first bad byte; optional leading `-`), `parse_int_or [b default] -> i32` (the number, or `default`), `valid_utf8 [b] -> bool`;

Searching and trimming, allocating nothing (a slice is a value) (linear time: needles over 64 bytes by the Two-Way algorithm of Crochemore and Perrin, as musl's memmem): `starts_with [b p]`, `ends_with`, `contains`, `find [b p] -> (option.Option i32)` (first index, or none), `find_from [b p from]` and `rfind` (the same),  `count` (not overlapping), `trim`, `trim_start`, `trim_end` (ASCII whitespace), `split_once [b sep] -> (option.Option str.Halves)` (`before`, `after`), `eq_ignore_case` (ASCII), `cmp [a b] -> i32` (byte order), `parse_i64 [b] -> (result i64 i32)`, `parse_bool`;

Making a `str` (a copy, never freed; section 4.B): `same [a:str b:str] -> bool` (the same bytes; `eq` on `str` is an error), `concat [a:str b:str] -> str`, `from_bytes [b] -> str` (`b` must be UTF-8)

<!-- api:str -->
```text
module str
    std/str: byte slices and byte-level text functions. Every function is plain
    AIPL over mem.* and str.*, so it behaves the same in the VM and compiled.

struct str.Bytes [(read data:(arr u8)) (read start:i32) (read len:i32)] (value)
    A view of len bytes, a value (nothing is allocated to make one, and
    nothing to free). A checked view reads bytes start .. start + len - 1
    of the array data, so a view of memory that has been freed stops the
    program ("Use after free") as a pointer would. A view made by the
    unsafe bytes has no array (data is null) and reads the memory at the
    address start, unchecked: for memory that is never freed (literals,
    mem.alloc) or that the caller vouches for. A failed read has len -1.

fn str.bytes [addr:i32 len:i32] -> str.Bytes (unsafe)
    A view of len bytes at the address addr, unchecked. Unsafe to call:
    the memory is the caller's word.

fn str.of_array [a:(arr u8) start:i32 len:i32] -> str.Bytes
    A checked view of bytes start .. start + len - 1 of a.
    (req (and (gte start 0) (and (gte len 0) (lte (+ start len) (arr.len a)))))

fn str.checked [b:str.Bytes] -> bool
    Whether b is checked (it has an array; see Bytes).

fn str.addr [b:str.Bytes] -> i32
    The address of b's first byte, for mem.* in unsafe code. A checked
    view's array is checked here (a freed one stops the program), so a
    loop that frees nothing may read through the address after.

fn str.absent [] -> str.Bytes
    The view that stands for no text where one may be missing (len -1, as
    a failed read is).

fn str.present [b:str.Bytes] -> bool
    Whether b is text (empty or not), not absent.

fn str.from_str [s:str] -> str.Bytes
    The bytes of a string, viewed in place (a str is never freed).

fn str.byte_at [b:str.Bytes i:i32] -> i32
    Byte i of b (0-255); i must be in range.
    (req (and (gte i 0) (lt i (get b Bytes.len))))

fn str.slice [b:str.Bytes start:i32 len:i32] -> str.Bytes
    Bytes start .. start + len - 1 of b, as a view of the same memory
    (a value: nothing is allocated).
    (req (and (gte start 0) (and (gte len 0) (lte (+ start len) (get b Bytes.len)))))

fn str.set_byte [b:str.Bytes i:i32 c:i32] -> void
    Sets byte i of b to c (its low 8 bits).
    (req (and (gte i 0) (lt i (get b Bytes.len))))

fn str.is_space [c:i32] -> bool
    ASCII whitespace: space, \t \n \v \f \r.

fn str.bytes_eq [a:str.Bytes b:str.Bytes] -> bool
    Whether a and b hold the same bytes.

fn str.is [b:str.Bytes s:str] -> bool
    Whether b spells the literal s, without allocating.

fn str.is_at [b:str.Bytes i:i32 s:str] -> bool
    Whether b holds the bytes of s at index i (false if they would run
    past its end), without allocating.

fn str.starts_with [b:str.Bytes p:str.Bytes] -> bool
    Whether b begins with p.

fn str.ends_with [b:str.Bytes p:str.Bytes] -> bool
    Whether b ends with p.

fn str.find_from [b:str.Bytes p:str.Bytes from:i32] -> (option.Option i32)
    The first index at or after `from` where p occurs in b, or none (an
    empty p occurs at `from`).
    (req (and (gte from 0) (lte from (get b Bytes.len))))

fn str.find [b:str.Bytes p:str.Bytes] -> (option.Option i32)
    The first index where p occurs in b, or none.

fn str.contains [b:str.Bytes p:str.Bytes] -> bool
    Whether p occurs in b.

fn str.rfind [b:str.Bytes p:str.Bytes] -> (option.Option i32)
    The last index where p occurs in b, or none (an empty p: b's length).

fn str.count [b:str.Bytes p:str.Bytes] -> i32
    How many times p occurs in b, not overlapping, from the left.
    (req (gt (get p Bytes.len) 0))

fn str.trim_start [b:str.Bytes] -> str.Bytes
    b without its leading, trailing, or both runs of ASCII whitespace.

fn str.trim_end [b:str.Bytes] -> str.Bytes
    b without trailing ASCII whitespace (a view).

fn str.trim [b:str.Bytes] -> str.Bytes
    b without leading or trailing ASCII whitespace (a view).

struct str.Halves [(read before:str.Bytes) (read after:str.Bytes)] (value)
    The text before and after the first sep.

fn str.split_once [b:str.Bytes sep:str.Bytes] -> (option.Option str.Halves)
    The text before and after the first sep in b, or none if sep does
    not occur.
    (req (gt (get sep Bytes.len) 0))

fn str.eq_ignore_case [a:str.Bytes b:str.Bytes] -> bool
    Whether a and b are equal but for the case of ASCII letters.

fn str.cmp [a:str.Bytes b:str.Bytes] -> i32
    Byte order: -1, 0, or 1 (a prefix first).

fn str.parse_i64 [b:str.Bytes] -> (result i64 i32)
    b as a decimal i64: an optional sign, then digits; or (err i), the
    index of the first byte that is not a digit, or of the digit past which
    the value no longer fits.

fn str.parse_bool [b:str.Bytes] -> (result bool i32)
    "true" or "false"; else (err 0).

fn str.same [a:str b:str] -> bool
    Whether a and b hold the same bytes. (eq on str is an error: it would
    compare addresses compiled and contents in the VM.)

fn str.from_bytes [b:str.Bytes] -> str
    A str holding a copy of b's bytes, which must be UTF-8 (check untrusted
    input with valid_utf8 first).
    (req (call valid_utf8 b))

fn str.concat [a:str b:str] -> str
    a followed by b, as a new str.

fn str.find_byte [b:str.Bytes c:i32] -> (option.Option i32)
    Index of the first byte equal to c, or none.

fn str.count_byte [b:str.Bytes c:i32] -> i32
    The number of bytes in b equal to c.

fn str.count_lines [b:str.Bytes] -> i32
    Newlines, plus one for a final line without a newline.

fn str.count_words [b:str.Bytes] -> i32
    Runs of non-whitespace bytes.

fn str.parse_int [b:str.Bytes] -> (result i32 i32)
    Decimal integer with an optional leading '-': (ok:i32 n), or (err:i32 i) where i
    is the index of the first byte that is not part of a number (0 for an
    empty input or a lone '-'). Values outside i32 wrap.

fn str.parse_int_or [b:str.Bytes default:i32] -> i32
    b as a decimal i32 (parse_int), or `default` if it is not one.

fn str.valid_utf8 [b:str.Bytes] -> bool
    Whether b is valid UTF-8 (as Rust's str::from_utf8 decides: no
    overlong forms, no surrogates, nothing past U+10FFFF).

fn str.run_str_tests [] -> i32
    Pass count; test_suite.aipl expects 10.
```
<!-- /api:str -->

### std/stream

Reading and writing as you go (a big input a piece at a time), over a file, standard input and output, bytes in memory, a `buf.Buf`, or functions of your own, each from the allocator passed in.

Readers: `open [a path:str] -> (result (ptr stream.Reader) io.Error)` (std/io's error: `not_found`, `denied`, ...), `open_path`, `stdin [a]`, `of_fd [a fd]`, `of_bytes [a b]` and `of_str [a s]` (read in place), `(reader_of C) [a state fill_fn]` with `fill_fn:(fn [C (ptr buf.Buf)] -> i32 (no_leak))` (a function that keeps no memory, section 4.G) appending some bytes and returning how many (0 at the end, negative on an error);

`next_line [r] -> bool` then `line [r] -> str.Bytes`, or `read_line [r] -> (option.Option str.Bytes)` (a line ends at `\n` or `\r\n`, which it leaves out; a last line with no `\n` keeps a `\r`, as Rust's `str::lines`), `read_byte`, `peek_byte` (-1 at the end), `read [r into max]` and `read_all [r into]` (append to a buffer), `read_error [r] -> bool`, `close_reader [r]` (closes what `open` opened).

What a reader hands out points into its window, good until the next read from it. Writers: `create [a path] -> (result (ptr stream.Writer) io.Error)`, `create_path`, `stdout [a]`, `stderr [a]`, `to_fd [a fd]`, `to_buf [a b]` (appends to b, holding nothing back), `(writer_of C) [a state write_fn]` with `write_fn:(fn [C str.Bytes] -> bool (no_leak))`; `write_byte`, `write_bytes`, `write_str`, `(format_to w ...)`, `buffer [w]` and `wrote [w]` (add to the held bytes directly), `flush [w] -> bool`, `write_error [w] -> bool`, `close_writer [w] -> bool` (flushes; closes what `create` opened).

A writer holds what it is given until it has 8 KB, `flush`, or `close_writer`: a program that ends without either loses its last writes, and between a buffered `stdout` writer and `println` (which writes at once) output can reorder unless you flush. Every OS error is -1 (section 4.C), so a stream records only that one happened; after a read error a reader is at its end, and after a write error a writer drops what it is given

<!-- api:stream -->
```text
module stream
    std/stream: reading and writing as you go, over a file, standard input
    and output, bytes in memory, a buf.Buf, or functions of your own, so a
    program (and csv, and later deflate and http) can take a big input a
    piece at a time instead of holding all of it.

      (let r:(ptr stream.Reader) (try (call stream.open a "data.txt")))
      (while (call stream.next_line r)
        (let line:str.Bytes (call stream.line r)) ...)
      (call stream.close_reader r)

      (let w:(ptr stream.Writer) (call stream.stdout a))
      (format_to w "{n} lines\n")
      (let _ok:bool (call stream.close_writer w))     ;; flushes

    A Reader holds a window of what it has read and not yet handed out
    (memory from its allocator, grown for a line longer than the window).
    What it hands out (a line, the view from `line`) points into that
    window, so it is good until the next read from that reader: copy what
    you keep.

    A Writer keeps what it is given in a buffer and writes it out when the
    buffer reaches 8 KB, at flush, and at close_writer: a program that ends
    without either loses its last writes. std/io and println write at once,
    so between them and a buffered stdout writer, flush.

    Every OS error is -1 (AIPL_SPEC.md 4.C), so a stream records only that
    one happened: read_error and write_error. After a read error a reader
    is at its end; after a write error a writer drops what it is given.

struct stream.Reader [] and 11 private fields
    The unread bytes are win's start .. end - 1. win views store, the
    window the reader owns (freed when it grows and by close_reader, so a
    line kept past them stops the program when read), or, for a reader
    of memory, the bytes it reads (store is null).

fn stream.of_fd [a:alloc.Allocator fd:i32] -> (ptr stream.Reader)
    Reads file descriptor fd; close_reader leaves fd open.

fn stream.stdin [a:alloc.Allocator] -> (ptr stream.Reader)
    Reads standard input.

fn stream.open [a:alloc.Allocator path:str] -> (result (ptr stream.Reader) io.Error)
    Opens a file to read, or says why not (io.message gives the words);
    close_reader closes it.

fn stream.open_path [a:alloc.Allocator path:str.Bytes] -> (result (ptr stream.Reader) io.Error)
    open for a path held as bytes.

fn stream.of_bytes [a:alloc.Allocator b:str.Bytes] -> (ptr stream.Reader)
    Reads b's bytes where they are, so b must not change or be freed
    while the reader is in use.
    (req (gte (get b str.Bytes.len) 0))

fn stream.of_str [a:alloc.Allocator s:str] -> (ptr stream.Reader)
    Reads the bytes of s.

fn (stream.reader_of C) [a:alloc.Allocator state:C fill_fn:(fn [C (ptr buf.Buf)] -> i32 (no_leak))] -> (ptr stream.Reader)
    A reader over fill_fn, called with `state` whenever the reader needs
    more bytes.

fn stream.read_byte [r:(ptr stream.Reader)] -> i32
    The next byte (0 .. 255), or -1 at the end.

fn stream.peek_byte [r:(ptr stream.Reader)] -> i32
    The next byte without taking it, or -1 at the end.

fn stream.next_line [r:(ptr stream.Reader)] -> bool
    Moves to the next line; false at the end. A line ends at "\n" or
    "\r\n", which `line` leaves out, or at the end of the input (a last
    line with no "\n" keeps a "\r", as Rust's str::lines and text.lines do).

fn stream.line [r:(ptr stream.Reader)] -> str.Bytes
    The line next_line moved to, good until the next read from r.

fn stream.read_line [r:(ptr stream.Reader)] -> (option.Option str.Bytes)
    The next line (as next_line and line), or none at the end.

fn stream.read [r:(ptr stream.Reader) into:(ptr buf.Buf) max:i32] -> i32
    Appends up to max bytes (max > 0) to `into`: how many, 0 at the end,
    or -1 after a read error.
    (req (gt max 0))

fn stream.read_all [r:(ptr stream.Reader) into:(ptr buf.Buf)] -> i32
    Appends everything left to `into`: how many bytes, or -1 after a read
    error (what came before it is appended).

fn stream.read_error [r:(ptr stream.Reader)] -> bool
    Whether a read from the source failed.

fn stream.close_reader [r:(ptr stream.Reader)] -> void
    Frees the reader and its window, and closes the file open opened.

struct stream.Writer [] and 6 private fields
    out holds what is not written yet; for a writer to a buf.Buf, out is
    that buffer and nothing is held back.

fn stream.to_fd [a:alloc.Allocator fd:i32] -> (ptr stream.Writer)
    Writes to file descriptor fd; close_writer leaves fd open.

fn stream.stdout [a:alloc.Allocator] -> (ptr stream.Writer)
    Writes to standard output (held until 8 KB, flush, or close_writer).

fn stream.stderr [a:alloc.Allocator] -> (ptr stream.Writer)
    Writes to standard error (held until 8 KB, flush, or close_writer).

fn stream.create [a:alloc.Allocator path:str] -> (result (ptr stream.Writer) io.Error)
    Creates (or empties) a file to write, or says why not; close_writer
    closes it.

fn stream.create_path [a:alloc.Allocator path:str.Bytes] -> (result (ptr stream.Writer) io.Error)
    create for a path held as bytes.

fn stream.to_buf [a:alloc.Allocator b:(ptr buf.Buf)] -> (ptr stream.Writer)
    Appends to b as it is written to; close_writer leaves b.

fn (stream.writer_of C) [a:alloc.Allocator state:C write_fn:(fn [C str.Bytes] -> bool (no_leak))] -> (ptr stream.Writer)
    A writer into write_fn, called with `state` and the bytes whenever the
    writer writes out.

fn stream.write_byte [w:(ptr stream.Writer) c:i32] -> void
    Writes the byte c (its low 8 bits).

fn stream.write_bytes [w:(ptr stream.Writer) b:str.Bytes] -> void
    Writes the bytes of b.

fn stream.write_str [w:(ptr stream.Writer) s:str] -> void
    Writes the text s.

fn stream.buffer [w:(ptr stream.Writer)] -> (ptr buf.Buf)
    The bytes not yet written out, to add to directly (buf.push_int, ...):
    call wrote after, so the writer writes out when it has 8 KB.

fn stream.wrote [w:(ptr stream.Writer)] -> void
    Writes out what is held once it reaches 8 KB.

fn stream.format_end [w:(ptr stream.Writer) b:(ptr buf.Buf)] -> void
    The end of (format_to w "..."): b is w's buffer.

fn stream.flush [w:(ptr stream.Writer)] -> bool
    Writes out everything held; false if a write has failed.

fn stream.write_error [w:(ptr stream.Writer)] -> bool
    Whether a write to the sink failed.

fn stream.close_writer [w:(ptr stream.Writer)] -> bool
    Flushes, frees the writer, and closes the file create opened; false
    if a write failed.

fn stream.run_stream_tests [] -> i32
    Pass count; test_suite.aipl expects 7.
```
<!-- /api:stream -->

### std/testing

What a test's `expect` calls (section 4.L): `fail [label:str]`, `fail_cmp [label left right]` (each counts one failure and writes to standard error), `failures [] -> i32`, `status [n] -> i32` (n, at most 125)

<!-- api:testing -->
```text
module testing
    std/testing: what a (test ...)'s (expect ...) calls (AIPL_SPEC.md 4.L,
    docs/design/FEEDBACK_TOOLS_PLAN.md F4). A failed expect writes where it
    is and what it checked to standard error, with both sides for a
    comparison, and counts one failure; the test goes on. A test build's
    _start runs the test its first argument names and exits with the count.
    AIPL has no globals, so the count is in runtime cell 24 (AIPL_SPEC.md
    7.9); each test runs in a fresh instance, so it starts at 0.

fn testing.failures [] -> i32
    The failures in the test running.

fn testing.fail [label:str] -> void
    A failed (expect c): its place and text.

fn testing.fail_cmp [label:str left:str right:str] -> void
    A failed comparison: its place and text, then both sides.

fn testing.status [n:i32] -> i32
    The exit status for n failures: 0 to 125 (a WASI command's range).
```
<!-- /api:testing -->

### std/text

Text that builds something, from the allocator passed in: `split [a b sep] -> (ptr (vec.Vec str.Bytes))` (Python's `str.split(sep)`: `"a,,b"` gives three pieces), `fields [a b]` (runs of non-whitespace, Python's `str.split()`), `lines [a b]` (Rust's `str::lines`: `\r\n` or `\n`, no empty last line), `join [a parts sep] -> (ptr buf.Buf)`, `replace [a b from to]` (left to right, not overlapping), `repeat [a b n]`, `to_upper`, `to_lower` (ASCII), `parse_f64 [b] -> (result f64 i32)` (exactly rounded; `12`, `-0.5`, `1e-3`; the error is the index of the first byte that does not fit).

Checked against Python on 800 generated cases (tests/aipl/text)

<!-- api:text -->
```text
module text
    std/text: the text functions that build something, over byte slices
    (str.Bytes), with memory from the allocator passed in (free a
    vector with vec.free, a buffer with buf.free; the slices in a vector are
    views of the input). The ones that only look are in std/str.

      (let parts:(ptr (vec.Vec str.Bytes)) (call text.split a line (call str.from_str ",")))
      (let b:(ptr buf.Buf) (call text.replace a s (call str.from_str "cat") (call str.from_str "dog")))

    Splitting works as Python's str.split(sep), str.split(), and Rust's
    str::lines do; replace as Python's str.replace.

fn text.split [a:alloc.Allocator b:str.Bytes sep:str.Bytes] -> (ptr (vec.Vec str.Bytes))
    The pieces of b between each sep: "a,,b" gives "a", "", "b"; "" gives
    one empty piece.

fn text.fields [a:alloc.Allocator b:str.Bytes] -> (ptr (vec.Vec str.Bytes))
    The runs of non-whitespace in b (ASCII whitespace): "  a b\n" gives "a", "b".

fn text.lines [a:alloc.Allocator b:str.Bytes] -> (ptr (vec.Vec str.Bytes))
    The lines of b: split at each \n, a \r before it dropped, and no empty
    line after a final \n ("a\r\nb\n" gives "a", "b"; "" gives none), as
    Rust's str::lines.

fn text.join [a:alloc.Allocator parts:(ptr (vec.Vec str.Bytes)) sep:str.Bytes] -> (ptr buf.Buf)
    The parts with sep between each.

fn text.replace [a:alloc.Allocator b:str.Bytes from:str.Bytes to:str.Bytes] -> (ptr buf.Buf)
    b with every `from` replaced by `to`, from the left, not overlapping.

fn text.repeat [a:alloc.Allocator b:str.Bytes n:i32] -> (ptr buf.Buf)
    b, n times.

fn text.to_upper [a:alloc.Allocator b:str.Bytes] -> (ptr buf.Buf)
    b with ASCII letters in upper or lower case (other bytes as they are).

fn text.to_lower [a:alloc.Allocator b:str.Bytes] -> (ptr buf.Buf)
    b with ASCII letters in lower case (other bytes as they are).

fn text.parse_f64 [b:str.Bytes] -> (result f64 i32)
    b as an f64, exactly rounded: an optional sign, digits with at most
    one '.', at least one digit, then optionally e or E, a sign, and
    digits ("12", "-0.5", "1e-3", ".5"); or (err i), the index of the
    first byte that does not fit (the length when b ends too soon).

fn text.run_text_tests [] -> i32
    Pass count; test_suite.aipl expects 7.
```
<!-- /api:text -->

### std/thread

`(spawn_with C) [worker:(fn [C] -> i32) ctx:C] -> i32`: runs `worker` on `ctx` on a new thread (unsafe to call, as `thread.spawn` is); `(thread.join h)` gives the result. The worker's context keeps its type, where `thread.spawn` passes an `i32`

<!-- api:thread -->
```text
module thread
    std/thread: starting a thread with a typed context (AIPL_SPEC.md 4.D).
    thread.spawn hands its worker one i32; spawn_with hands it a value of any
    type, typically a pointer to the struct the worker reads and writes:

      (fn sum_part [p:(ptr Part)] -> i32 ...)
      (let h:i32 (unsafe (call (thread.spawn_with (ptr Part)) (ref sum_part) part)))
      (let total:i32 (unsafe (thread.join h)))

    AIPL has no closures: the context argument is how a worker sees data of
    its own. Threads share memory and nothing checks for data races, so
    starting one is unsafe (AIPL_SPEC.md 3); guard shared data with atomics or
    a lock.

fn (thread.spawn_with C) [worker:(fn [C] -> i32) ctx:C] -> i32 (unsafe)
    Runs (worker ctx) on a new thread; returns its handle for thread.join,
    which gives the worker's result.

fn thread.run_thread_tests [] -> i32
    Pass count; test_suite.aipl expects 2.
```
<!-- /api:thread -->

### std/time

Timing code with the monotonic clock: `now [] -> i64` (nanoseconds), `since [start] -> i64`, `push_duration [b ns]` ("850 ns", "12.345 us", "3.071 ms", "4.200 s": three decimals in the largest fitting unit, integer arithmetic), `report [label start]` ("label: 12.345 ms" on stderr, so a program's output stays clean), `ms_until [deadline:i64] -> i32` (the milliseconds to a `now` value, rounded up, 0 once past: what `poll` takes)

<!-- api:time -->
```text
module time
    std/time: measuring how long code takes, from the monotonic clock
    (sys.monotonic: nanoseconds, never going backwards).

      (let t:i64 (call time.now))
      ... work ...
      (call time.report "sort" t)          ;; stderr: "sort: 12.345 ms"
      (call time.sleep 250000000i64)       ;; waits a quarter of a second

    Durations print with three decimals in the largest fitting unit (ns, us,
    ms, s), from integer arithmetic.

fn time.now [] -> i64
    Now, in nanoseconds since an arbitrary start (only differences matter).

fn time.sleep [ns:i64] -> void
    Waits `ns` nanoseconds (nothing for 0 or less), by sys.poll: WASI's
    poll_oneoff with one relative clock subscription, which every backend
    gives. A wait a signal cuts short ends early.

fn time.since [start:i64] -> i64
    Nanoseconds since `start` (a value from now).

fn time.ms_until [deadline:i64] -> i32
    The milliseconds from now until `deadline` (a value of now), rounded up: 0 once it has passed, and at most
    2147483647 (what poll takes as a timeout).

fn time.push_duration [b:(ptr buf.Buf) ns:i64] -> void
    Appends a duration: "850 ns", "12.345 us", "3.071 ms", "4.200 s".

fn time.report [label:str start:i64] -> void
    Writes "label: <duration since start>" and a newline to stderr (so a
    program's own output stays clean).

fn time.run_time_tests [] -> i32
    Pass count; test_suite.aipl expects 7.
```
<!-- /api:time -->

### std/url

URLs (RFC 3986): `parse [a text:str.Bytes] -> (result (ptr url.Url) (ptr url.Error))`, the parts as views in the text as written (`scheme` empty if none; `has_authority`, `authority`, `has_user`, `user`, `has_password`, `password`, `host` (an IPv6 address without its brackets), `port` (-1 if none), `path`, `has_query`, `query`, `has_fragment`, `fragment`), split as Python's `urlsplit` and its properties split them; a port that is not a number up to 65535 or an unpaired bracket is an error; `free [u]`; `write [o u]`, `to_str [u]`;

`resolve [a base ref] -> (ptr url.Url)` (RFC 3986 5.2, strict: "http:g" stays; Python's `urljoin` differs from the RFC in three ways: it keeps dot segments in a network-path reference, drops empty segments, and gives an empty reference back as the whole base), `resolve_str [a base ref] -> (result str (ptr url.Error))`, `remove_dots [path]`;

`query_pairs [a q] -> (result (ptr (vec.Vec str)) (ptr url.Error))` (names and values in turn, + as space, %XX decoded; a bad % or bytes that are not UTF-8 are an error, where Python passes them through), `query_get [a q name] -> (result (option.Option str) (ptr url.Error))`;

These three give `str`s, never freed, and each has an `_into` form that writes into a buffer of yours and keeps no memory, for a request's path: `remove_dots_into [o path]`, `query_pairs_into [a o q] -> (result (ptr (vec.Vec str.Bytes)) (ptr url.Error))` and `query_get_into [a o q name] -> (result (option.Option str.Bytes) (ptr url.Error))` (views of `o`, good until it changes).

`parse`, `resolve` (whose path the new Url owns: `free` frees it), `write`, and the `_into` forms are `(no_leak)`, as are std/json's and std/text's functions; `query_build [pairs] -> str` (Python's `urlencode`); `free_error [a e]`

<!-- api:url -->
```text
module url
    std/url: URLs (RFC 3986) taken apart, put back together, and resolved
    against a base; and query strings both ways.

      (match_result (call url.parse a (call str.from_str "https://x.org/a/b?q=1#top"))
        (ok u ... (get u url.Url.host) (get u url.Url.path) ...)
        (err e ... (get e url.Error.what) ...))
      (call url.resolve_str a "http://a/b/c/d;p?q" "../g")   ;; (ok "http://a/b/g")

    A Url's parts are views into the text it was parsed from, as written
    (scheme and host are not lowercased: compare them ignoring case), with
    whether it has an authority, a query, and a fragment ("http://a?" has
    an empty query, "http://a" none). The parts are split as Python's
    urllib.parse.urlsplit splits them, with username, password, hostname
    (without the brackets of an IPv6 address), and port as its properties
    give them; a port that is not a number up to 65535, or a bracket
    without its pair, is an error. resolve is RFC 3986's (section 5.2, the
    strict form: "http:g" against an http base stays "http:g"), as Python's
    urljoin is apart from that. Checked against Python (tests/aipl/url).

struct url.Error [(read at:i32) (read what:str)]
    Why a URL or a query was refused, and the byte offset where.

struct url.Url [(read scheme:str.Bytes) (read has_authority:bool) (read authority:str.Bytes) (read has_user:bool) (read user:str.Bytes) (read has_password:bool) (read password:str.Bytes) (read host:str.Bytes) (read port:i32) (read path:str.Bytes) (read has_query:bool) (read query:str.Bytes) (read has_fragment:bool) (read fragment:str.Bytes)] and 3 private fields
    A URL's parts (port -1 if none); scheme is empty if there is none;
    authority is all of it as written (user info, host, and port). From
    the allocator it was made with; free gives it back, with the path
    resolve made for it (owned).

fn url.free_error [a:alloc.Allocator e:(ptr url.Error)] -> void
    Frees an Error parse or a query function gave.

fn url.parse [a:alloc.Allocator text:str.Bytes] -> (result (ptr url.Url) (ptr url.Error))
    text's parts, from a, or why it is not a URL.

fn url.free [u:(ptr url.Url)] -> void
    Frees the Url, and the path resolve made for it (its other parts are
    views of the text it came from).

fn url.write [o:(ptr buf.Buf) u:(ptr url.Url)] -> void
    Appends u as text (RFC 3986, 5.3): an IPv6 host in brackets, the port
    if there is one.

fn url.to_str [u:(ptr url.Url)] -> str
    u as a new str.

fn url.remove_dots_into [o:(ptr buf.Buf) path:str.Bytes] -> void
    path with its "." and ".." segments taken out (5.2.4), written to o,
    which must be empty: nothing is allocated but o's growth.

fn url.remove_dots [path:str.Bytes] -> str.Bytes
    path with its "." and ".." segments taken out (5.2.4), as new bytes,
    never freed (a str's): remove_dots_into keeps no memory.

fn url.resolve [a:alloc.Allocator base:(ptr url.Url) ref:(ptr url.Url)] -> (ptr url.Url)
    ref resolved against base (RFC 3986, 5.2.2, strict), as a new Url
    from a.

fn url.resolve_str [a:alloc.Allocator base:str ref:str] -> (result str (ptr url.Error))
    ref resolved against base, as text, or why one is not a URL.

fn url.query_pairs [a:alloc.Allocator q:str.Bytes] -> (result (ptr (vec.Vec str)) (ptr url.Error))
    The names and values of the query q ("a=1&b=two+words"), in turn
    (name 0, value 0, name 1, ...), each with + as a space and %XX
    decoded, in a new vector from a; parts are split at '&', empty ones
    skipped, a part without '=' a name with an empty value (as Python's
    parse_qsl with keep_blank_values). A % without two hex digits, or
    bytes that are not UTF-8, is an error (Python passes them through).
    Each is a str, never freed: query_pairs_into keeps no memory.

fn url.query_pairs_into [a:alloc.Allocator o:(ptr buf.Buf) q:str.Bytes] -> (result (ptr (vec.Vec str.Bytes)) (ptr url.Error))
    query_pairs, the names and values decoded into o (after what it
    holds) and given as views of it, in a new vector from a: no str is
    made, so nothing is kept. The views are good until o changes.

fn url.query_get [a:alloc.Allocator q:str.Bytes name:str] -> (result (option.Option str) (ptr url.Error))
    The first value for name in the query q, decoded as query_pairs
    decodes, if there is one: a str, never freed (query_get_into keeps
    no memory).

fn url.query_get_into [a:alloc.Allocator o:(ptr buf.Buf) q:str.Bytes name:str] -> (result (option.Option str.Bytes) (ptr url.Error))
    query_get, the value decoded into o and given as a view of it, good
    until o changes: nothing is kept.

fn url.query_build [pairs:(ptr (vec.Vec str))] -> str
    The query for the names and values in turn (as query_pairs gives
    them): each name=value joined with '&', with a space as + and all but
    A-Z a-z 0-9 _ . - ~ as %XX (Python's urlencode).
    (req (eq (% (call (vec.len str) pairs) 2) 0))
```
<!-- /api:url -->

### std/utf8

One character (scalar value) at a time: `decode [b i] -> utf8.Char` (`cp`, and `len` 1-4, or 0 where the bytes are not UTF-8), `step [ch] -> i32` (the bytes to move past it), `encode [buf cp]`, `count [b]` (characters), `scalar [cp]`, `is_space [cp]` (Unicode's White_Space property), `is_ascii_letter`, `is_ascii_digit`

<!-- api:utf8 -->
```text
module utf8
    std/utf8: text one character (Unicode scalar value) at a time, over byte
    slices. A str is UTF-8 by construction; a str.Bytes from a file may
    not be (str.valid_utf8 checks).

      (let i:i32 0)
      (while (lt i (get b str.Bytes.len))
        (let ch:utf8.Char (call utf8.decode b i))
        ... (get ch utf8.Char.cp) ...
        (set! i (+ i (call utf8.step ch))))

struct utf8.Char [(read cp:i32) (read len:i32)] (value)
    The character at a byte index: its code point and its length in bytes
    (1-4); len 0 when the bytes there are not UTF-8 (cp is the byte).

fn utf8.step [ch:utf8.Char] -> i32
    The bytes to move past ch: its length, or 1 past a byte that is not UTF-8.

fn utf8.decode [b:str.Bytes i:i32] -> utf8.Char
    The character starting at byte i of b (as str.valid_utf8 decides what
    is UTF-8: no overlong forms, no surrogates, nothing past U+10FFFF).
    (req (and (gte i 0) (lt i (get b str.Bytes.len))))

fn utf8.scalar [cp:i32] -> bool
    Whether cp is a Unicode scalar value (0 .. 0x10FFFF, not a surrogate).

fn utf8.encode [out:(ptr buf.Buf) cp:i32] -> void
    Appends cp's UTF-8 bytes.
    (req (call scalar cp))

fn utf8.count [b:str.Bytes] -> i32
    The characters in b (a byte that is not UTF-8 counts as one).

fn utf8.is_space [cp:i32] -> bool
    Unicode's White_Space property: \t \n \v \f \r, space, U+0085, U+00A0,
    U+1680, U+2000-U+200A, U+2028, U+2029, U+202F, U+205F, U+3000.

fn utf8.is_ascii_letter [cp:i32] -> bool
    Whether cp is an ASCII letter (A-Z, a-z).

fn utf8.is_ascii_digit [cp:i32] -> bool
    Whether cp is an ASCII digit (0-9).

fn utf8.run_utf8_tests [] -> i32
    Pass count; test_suite.aipl expects 4.
```
<!-- /api:utf8 -->

### std/uuid

Version 4 UUIDs as 36 lowercase characters, 8-4-4-4-12: `v4 [] -> str` (the system's random bytes), `v4_from [r:(ptr random.Rng)]` (the same for the same seed), `valid [s] -> bool`

<!-- api:uuid -->
```text
module uuid
    std/uuid: random UUIDs (RFC 9562 version 4), as 36 characters,
    8-4-4-4-12 lowercase hex digits: "3f2b8c1e-7d4a-4c9b-9e21-5a6b7c8d9e0f".

      (call uuid.v4)                 ;; from the system's random bytes
      (call uuid.v4_from r)          ;; from a random.Rng: the same each run

fn uuid.v4 [] -> str
    A version 4 UUID from the system's random bytes.

fn uuid.v4_from [r:(ptr random.Rng)] -> str
    A version 4 UUID from r: the same UUIDs for the same seed.

fn uuid.valid [s:str] -> bool
    Whether s is a UUID's text: 8-4-4-4-12 hex digits (either case).
```
<!-- /api:uuid -->

### std/vec

Generic growable list `(vec.Vec T)`: `(call (vec.make T) a capacity)` with `a` an `alloc.Allocator` (growing frees the old storage through it), `(call (vec.free T) v)`, `push`, `pop`, `at [v i]`, `set [v i x]`, `len`, `truncate [v n]`, `clear`, `index_of [v x] -> (option.Option i32)`, `(sort T) [v]` (ascending by `T`'s `cmp.Ord`, stable), `sort_by [v cmp:(fn [T T] -> i32 (no_leak))]` (stable merge sort; `cmp` negative puts the first argument first; any comparison that keeps no memory: section 4.G), `(sort_by_with T C) [v ctx:C cmp:(fn [C T T] -> i32 (no_leak))]` (the same, `cmp` given `ctx` first), each called as `(call (vec.push T) v x)`; plus `sort_i32 [v:(ptr (vec.Vec i32))]` and `cmp_i32`

<!-- api:vec -->
```text
module vec
    std/vec: a growable list of any element type (storage doubles when full).
    Generic (AIPL_SPEC.md 4.H): every use names the element type. Its memory
    comes from the allocator it is made with (std/alloc); growing frees the
    old storage, and free releases it all.
      (let v:(ptr (vec.Vec i32)) (call (vec.make i32) (call alloc.default) 0))
      (call (vec.push i32) v 7)
      (call (vec.at i32) v 0)
      (call (vec.free i32) v)

struct (vec.Vec T) [(read len:i32)] and 3 private fields
    A growable list of T; len is how many it holds.

fn (vec.make T) [a:alloc.Allocator capacity:i32] -> (ptr (vec.Vec T))
    An empty list with room for `capacity` items, from a.
    (req (gte capacity 0))

fn (vec.free T) [v:(ptr (vec.Vec T))] -> void
    Frees the storage and the vec itself; neither may be used afterwards.

fn (vec.len T) [v:(ptr (vec.Vec T))] -> i32
    The number of items in v.

fn (vec.at T) [v:(ptr (vec.Vec T)) i:i32] -> T
    Element i.
    (req (and (gte i 0) (lt i (get v (vec.Vec T) len))))

fn (vec.set T) [v:(ptr (vec.Vec T)) i:i32 x:T] -> void
    Replaces item i of v (0 <= i < len) with x.
    (req (and (gte i 0) (lt i (get v (vec.Vec T) len))))

fn (vec.push T) [v:(ptr (vec.Vec T)) x:T] -> void
    Appends x, doubling the storage when full.

fn (vec.pop T) [v:(ptr (vec.Vec T))] -> T
    Removes and returns the last element.
    (req (gt (get v (vec.Vec T) len) 0))

fn (vec.truncate T) [v:(ptr (vec.Vec T)) n:i32] -> void
    Keeps the first n elements.
    (req (and (gte n 0) (lte n (get v (vec.Vec T) len))))

fn (vec.clear T) [v:(ptr (vec.Vec T))] -> void
    Empties v, keeping its storage.

fn (vec.index_of T) [v:(ptr (vec.Vec T)) x:T] -> (option.Option i32) (where (cmp.Eq T))
    Index of the first element equal to x (eq: same value, or same address
    for pointers), or none.

fn vec.cmp_i32 [a:i32 b:i32] -> i32
    -1, 0, 1 as a < b, a = b, a > b (no subtraction, so no overflow).

fn (vec.sort_by T) [v:(ptr (vec.Vec T)) cmp:(fn [T T] -> i32 (no_leak))] -> void
    Stable sort by cmp: a negative result puts a before b, positive puts b
    before a, zero keeps their order. Bottom-up merge sort, O(n log n).

fn (vec.sort_by_with T C) [v:(ptr (vec.Vec T)) ctx:C cmp:(fn [C T T] -> i32 (no_leak))] -> void
    sort_by, with a context passed to every comparison: (cmp ctx a b). AIPL
    has no closures, so this is how a comparison sees data of its own, a
    map of counts, say (AIPL_SPEC.md 4.G). The same merge sort as sort_by,
    kept as its own copy: sort_by on top of this would cost an extra
    indirect call per comparison (9% on a million i32s).

fn (vec.sort T) [v:(ptr (vec.Vec T))] -> void (where (cmp.Ord T))
    Ascending by T's order (std/cmp's Ord: i32, i64, str, or a type that
    implements it), stable: (call (vec.sort str) names). The same merge
    sort as sort_by, calling T's compare directly: no indirect call, and
    any implementation of Ord sorts (sort_by's comparator must keep no
    memory; here a compare that keeps some makes only this sort may-leak).

fn vec.sort_i32 [v:(ptr (vec.Vec i32))] -> void
    Ascending.

fn vec.run_vec_tests [] -> i32
    Pass count; test_suite.aipl expects 8.
```
<!-- /api:vec -->
