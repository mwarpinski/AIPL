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
about 4,400 lines of AIPL with no dependencies. LLVM would make the code
much faster and the toolchain far larger, and could not be self-hosted. A
faster native translator is on the roadmap instead.

### Is it really self-hosted?

Yes, with one caveat. The import resolver, type checker, code generator,
and native backend are written in AIPL. They produce the same bytes and the
same error messages as the Rust implementation, and the compiler, built as a
native executable, compiles itself to the same bytes in about half a
second. The caveat is the first build: today it still takes the Rust
toolchain. Rebuilding from a checked-in compiled seed, so that Rust is only
needed for the WebAssembly launcher, is on the roadmap.

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
heap stops the program on a double free or a write after free. The
operations nothing checks (raw `mem.load32`/`mem.store32`, casts to
pointers, threads, atomics) are rejected outside an `(unsafe ...)` block,
as in Rust, so a program without `unsafe` cannot reach memory through a
raw address.

Two gaps remain. A pointer kept after its object is freed can still read
the freed memory; generation checks (each object and each pointer carries a
number, compared on every access) are planned
([docs/design/GENERATIONS_PLAN.md](design/GENERATIONS_PLAN.md)). And the
standard library's structs, such as `str.Bytes`, keep raw addresses in
fields that any module can write; module visibility will close that.

### How fast is it?

Not fast yet. On eight benchmarks, native executables take 1.7 to 40 times
as long as C compiled with `gcc -O2`, and WebAssembly run by wasmtime 0.8 to
14 times as long. The native backend translates one instruction at a time
and keeps most values in memory. [docs/BENCHMARKS.md](BENCHMARKS.md) has
every measurement and what each benchmark showed.

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
