# Frequently asked questions

### Why S-expressions?

Because there is exactly one way to read them. `(+ a (* b c))` has no
precedence rules, no optional parentheses, no significant whitespace, and no
statement-versus-expression distinction to get wrong. Every construct has
one written form, so a model writing AIPL has fewer decisions to make and
fewer of them to get wrong, and a tool can read its structure with about a
hundred lines of code. A missing or extra parenthesis is reported with its line and
column.

The cost is that it looks unfamiliar to people who do not write Lisp, and
it is longer than the infix equivalent. AIPL is meant to be written mostly
by models, which do not mind.

### Why is it so verbose? `(call (vec.push i32) v x)` to push onto a list?

On purpose. Each part says something a reader would otherwise have to work
out: `call` marks a user function (not a built-in operation), `vec.push`
names where it comes from, and `i32` says which version of the generic
function this is. Types are written on every parameter, local, and field.
Nothing is inferred, so nothing is inferred wrongly. A model produces the
extra words at no cost, and a human reviewing the code can see what each
line does without tracing types across the file.

### Why not just use Rust, Zig, Go, or Python?

They are good languages, and models write them well. AIPL is a bet that a
language designed around how models fail can do better on correctness:
smaller, more regular, with contracts and bounds checks everywhere, and
errors that point at the exact expression. The bet is not proven. The next
major step is an experiment where agents write the same set of tasks in
AIPL and in other languages, measured on how often the result works
([ROADMAP.md](../ROADMAP.md)). If AIPL loses, the write-up will say so.

### Why compile to WebAssembly?

WebAssembly has a precise, published semantics, so it serves as the
definition of what a program means: the interpreter, the WebAssembly
backend, and the native backend are all tested against it, and must agree.
It is also a simple, sandboxed, portable target. Native executables are
made by translating the WebAssembly to x86-64 machine code.

### Why not compile through LLVM or C?

To keep the toolchain small and written in AIPL. The native backend is
about 5,800 lines of AIPL with no dependencies: a single-pass translator
that keeps values in registers, in the manner of V8's Liftoff, whose code
takes 0.8 to 2.0 times as long as wasmtime's (Cranelift's) on the eight
benchmarks. LLVM would make the code faster still and the toolchain far
larger, and could not be self-hosted.

### Is it really self-hosted?

Yes, with one caveat. The import resolver, type checker, code generator,
and native backend are written in AIPL. They produce the same bytes and the
same error messages as the Rust implementation, and the compiler, built as a
native executable, compiles itself to the same bytes in about half a
second. On Linux x86-64 it builds from a checked-in native seed with no
Rust at all (`sh bootstrap/build.sh`, about 3 seconds), and every test run
checks that the seed's build is byte for byte the Rust toolchain's, so the
seed cannot carry anything the source does not say (bootstrap/README.md).
The caveat: the `aipl` command most people use is still the Rust one,
since the interpreter and the WebAssembly runner are Rust, and other
platforms have no seed yet.

### Was it written by AI?

The language is designed by Matt Warpinski and implemented with AI coding
agents; the commits say so. That is also why the project leans so hard on
checking: two independent compilers that must produce identical bytes, an
interpreter and two backends that must agree on every program, fuzzers
that generate and mutate programs on every push, and tests that are
themselves checked by planting bugs to see that they fail. The README lists
these.

### Is it memory-safe?

Outside `(unsafe ...)`, yes, with limits stated below. Array indexes are
bounds-checked, pointers are typed and checked for null, and every pointer
carries its object's generation, a number the object's allocator changes
when it frees it. Every access compares the two, so a pointer kept after
its object is freed (with `std/heap`), or its arena reset (`std/arena`),
stops the program with `Use after free` and the line, the same in the
interpreter and in compiled code, whether or not the memory holds a new
object by then. Freeing twice is caught the same way. The operations
nothing checks (raw `mem.load32`/`mem.store32`, casts to pointers,
threads, atomics) are rejected outside an `(unsafe ...)` block, as in
Rust, and module visibility keeps the standard library's raw addresses in
fields other modules may only read, so safe code cannot forge one.

The limits:

- **Data races.** AIPL does not check them, so threads and atomics need
  `unsafe`. `std/par` runs work in parallel without `unsafe`: its workers
  share only plain values (no pointers or arrays inside) and their own
  copies of the input, so there is nothing to race on, and
  `examples/spectralnorm_par.aipl` runs 3.9 times as fast on 8 threads.
- **Arenas, after their memory is reused.** A new object can start where
  an old one's middle was, so the word an old pointer checks may be the
  new object's data; the pointer is caught unless that word happens to
  equal its old generation, a scrambled 32-bit number. The heap reuses a
  block only for a block of the same size, so it has no such case.
- **Fast arenas.** `arena.Fast`, which a program chooses by name for
  speed, marks nothing at reset: a stale pointer is caught once its memory
  is reused, and until then reads the old object (wrong, but not unsafe).
- **`ptr.cast`.** A pointer made from an address inside `unsafe` is not
  checked.
- **The trusted base.** The compilers, the standard library's own
  `unsafe` code, and the runtime are trusted; two independent compilers
  that must agree byte for byte, differential testing, and fuzzing check
  them, which is evidence, not proof.

Unlike Rust, which rejects a use after free before the program runs,
AIPL stops the program when it happens: no borrow checker, at the cost of
a check on each pointer access (one per loop where nothing in the loop
can free). It never reaches outside the program's memory in any case
(WebAssembly's sandbox, a guarded region natively). The costs are in
docs/BENCHMARKS.md; the design is in
[docs/design/GENERATIONS_PLAN.md](design/GENERATIONS_PLAN.md).

### Does it have closures, or lambdas?

No, by design. A function sees only its parameters: nothing captures the
variables around it, and there are no global variables. When a callback
needs data, such as a comparison that sorts by counts kept in a map, the
data is passed to it as a typed *context*, its first parameter:
`(call (vec.sort_by_with str (ptr Counts)) words counts (ref by_count))`.
A closure would have to allocate its captured variables behind the
program's back, where AIPL allocates only through allocators the program
names; it would make every function value bigger and every call through
one slower; and it would add rules about what a capture copies or shares.
Passing the context costs a parameter, and the reader sees exactly what the
callback uses. (AIPL_SPEC.md 4.G.)

### Are structs values or references?

Either, but each struct is one or the other for good. A plain struct lives
behind a pointer, `(ptr Node)`, and is changed in place with `put`. A struct
declared `(value)`, such as `(struct Vec3 [x:f64 y:f64 z:f64] (value))`, is
a value like a number: copied where it goes, held inline in arrays and in
other structs, allocating nothing, and never changed (`(with v Vec3.x 1.0)`
is a copy with one field changed). Since nothing converts one kind into the
other, the classic bug of changing a copy while meaning the original cannot
happen. A value struct holds at most eight numbers; larger records belong
behind pointers. Writing the n-body benchmark with a `Body` value struct
made it 1.9 times faster under wasmtime (AIPL_SPEC.md 4.E,
[docs/design/VALUES_PLAN.md](design/VALUES_PLAN.md)). Unions, which cannot
be changed either, are held by value without being declared so whenever
they fit (they do not contain themselves and need at most eight numbers),
so returning an `(option.Option i32)` allocates nothing.

### How fast is it?

Between C and Python. On the eight benchmarks, native executables take
1.2 to 17 times as long as C compiled with `gcc -O2`, and WebAssembly run
by wasmtime 1.0 to 16 times as long. The slowest are k-nucleotide (C packs
each key into one 64-bit integer where AIPL hashes byte strings) and
pidigits (C uses GMP's hand-tuned big integers). binarytrees is not a fair
race: AIPL frees whole trees from an arena while C frees every node.
Generation checks (use after free) cost little on six of them, 1.3 times
on k-nucleotide, and twice on binarytrees. The native backend is a
single-pass translator, within 0.8 to 2.0 times wasmtime's optimising
compiler. [docs/BENCHMARKS.md](BENCHMARKS.md) has every measurement and
what each benchmark showed.

### Can people write it?

Yes; it is a small language with no surprises. The
[ten-minute tour](TOUR.md) walks through it. It is wordier than what people
usually choose to write by hand, which is the trade it makes.

### What platforms does it run on?

Linux x86-64 is the one that is tested, and the only one with native
executables. Elsewhere, programs compile to WebAssembly and run under the
`aipl-run` launcher (built on wasmtime), which should work on macOS and
Windows but is not tested there.

### Is there a package manager?

Not yet. Modules import each other by name from the importing file's
directory, the entry file's directory, and the standard library. A
registry with versioned packages, dependency resolution, and lock files is
planned for after the first release.

### Is there editor support?

Not yet: no syntax highlighting package and no language server. Any editor
that understands Lisp parentheses gets you most of the way.

### Is it ready for real use?

No. It is version 0.1.0, nothing about it is stable, and the language will
change. It is ready to read, run, and experiment with.

### How do I report a problem or contribute?

Open an issue on GitHub. [DEVELOPING.md](../DEVELOPING.md) explains how to
build and test everything, how the repository is laid out, and the
conventions changes follow.
