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
takes 0.8 to 1.9 times as long as wasmtime's (Cranelift's) on the eight
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

Not yet. What holds now: array indexes are bounds-checked, pointers are
typed and checked for null, contracts run on every call, and the standard
heap stops the program on a double free, a free of the wrong size or type,
or a write to freed memory (when that memory is handed out again). The
operations nothing checks (raw `mem.load32`/`mem.store32`, casts to
pointers, threads, atomics) are rejected outside an `(unsafe ...)` block,
as in Rust, so a program without `unsafe` cannot reach memory through a
raw address.

Module visibility closes a second door: the standard library keeps its raw
addresses in fields other modules may only read, such as `str.Bytes`'s
address and length, so safe code cannot forge one.

The gap is use after free. A pointer kept after its object is freed (with
`std/heap`) or its arena is reset (`std/arena`) still works. While the
memory is free, the heap has filled it with a junk pattern; but once it
holds another object, the old pointer reads and writes that object, and
writing an integer where the new object keeps a pointer gives code without
`unsafe` a pointer to any address in the program's memory. It never
reaches outside that memory, in any backend (WebAssembly's sandbox, a
guarded region natively), but inside it nothing is protected. Generation
checks (each object and each pointer carries a number, compared on every
access) are planned
([docs/design/GENERATIONS_PLAN.md](design/GENERATIONS_PLAN.md)).

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

Between C and Python. On seven of eight benchmarks, native executables
take 1.2 to 14 times as long as C compiled with `gcc -O2`, and
WebAssembly run by wasmtime 1.4 to 12 times as long. The slowest are
pidigits (C uses GMP's hand-tuned big integers) and k-nucleotide (C packs
each key into one 64-bit integer where AIPL hashes byte strings). On the
eighth, binarytrees, AIPL takes 0.6 times C's time because it frees whole
trees from an arena while C frees every node, so that one is not a fair
race. The native backend
is a single-pass translator, within 0.8 to 1.9 times wasmtime's optimising
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
