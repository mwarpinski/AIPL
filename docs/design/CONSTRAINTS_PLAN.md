# Interfaces, constraints, and checking templates once

**Status: done (IC1–IC5, 2026-10-14).** Kept as the design record; the spec (AIPL_SPEC.md 4.H) describes the result. Two details settled while building it: a template's `where` clauses come right after its signature, and an implementation's functions must use the interface's parameter names (its contracts refer to them).

## The two problems

**A template is only checked when someone uses it.** Generics are expanded
before type checking (AIPL_SPEC.md 4.H): the checker sees `index_of<i32>`,
`index_of<f64>`, and so on, never `(fn (index_of T) ...)` itself. So a
template's signature can promise more than its body keeps:

```lisp
(fn (index_of T) [v:(ptr (Vec T)) x:T] -> i32
  (loop i 0 (- (get v (Vec T) len) 1) 1
    (if (eq (arr.get T (get v (Vec T) items) i) x) (return i) (block)))
  -1)
```

The header says "any T", but the body uses `eq`, which unions do not have.
`(call (vec.index_of (option.Option i32)) v o)` fails with an error inside
`std/vec.aipl`, a file the caller never wrote. C++ templates and Zig work
this way; C++20 concepts check only the call, not the body.

**A template cannot ask for an operation on its type.** `std/map` is keyed
by `i32` and `std/strmap` (since merged into `std/map`) by byte strings, two copies of one hash map,
because a template cannot say "K can be hashed and compared".

## The design

Rust's traits, Haskell's type classes, and Swift's protocols, with AIPL's
habit of writing everything out:

1. **A template is checked once, with its type parameters sealed.** Inside
   the body a type parameter supports only what every type of its kind
   supports, plus what its `where` clauses grant. The checker proves the
   body for every type that meets the clauses; an error in a template is
   its author's, found when it is written.
2. **An interface names operations; an implementation provides them for one
   type.** A `where` clause asks for an interface; a use of the template
   names types, and each must have the implementations asked for.
3. **One meaning per type (coherence).** A type has at most one
   implementation of an interface in a program, so two maps keyed by the
   same type cannot hash it two ways.
4. **Nothing is inferred.** A call to an interface's function names the
   interface and the type, as generic calls name their type arguments.

Instances are still made per type, as now: the code generators, the VM,
and the native backend see the same concrete code. No run-time cost, and
byte parity between the toolchains works as it does today.

### Interfaces

```lisp
(interface (Hash K)
  (fn hash [k:K] -> i32)
  (fn same [a:K b:K] -> bool))

(interface (Ord T)
  ;; negative, zero, or positive
  (fn compare [a:T b:T] -> i32
    (ens (and (gte res -1) (lte res 1)))))
```

- One type parameter. Several (`(Convert A B)`) can come later; nothing
  needs them yet.
- Each function is a signature: a name, parameters, a return type, and
  optionally `req`/`ens` contracts and no body. Every implementation gets
  the interface's contracts in front of its own, so an interface can state
  what its functions promise, and each implementation is checked against it
  on every call.
- An interface is named like a struct: `Hash` in its module, `cmp.Hash`
  from another, through an alias `c.Hash`.

### Implementations

```lisp
(impl (Hash i32)
  (fn hash [k:i32] -> i32 (call mix k))
  (fn same [a:i32 b:i32] -> bool (eq a b)))
```

- An implementation defines exactly the interface's functions, with the
  interface's signatures after putting the type in for the parameter.
  Missing, extra, or differently typed functions are errors that name the
  difference.
- **Coherence.** At most one `(impl (Hash t) ...)` per type `t` in the
  whole program: `two implementations of (Hash i32): a.aipl:3:3 and
  b.aipl:9:3`.
- **Where an implementation may live (the orphan rule).** In the
  interface's module, or in the module that defines the type: for
  `(ptr geo.Point)`, `geo`. An implementation for a built-in type (`i32`,
  `str`) can only be in the interface's module. Two libraries can then
  never define clashing implementations, which matters once packages exist
  (ROADMAP.md).
- Implementations are for concrete types only. A generic implementation
  (`(impl (Hash (ptr T)) ...)` for every struct) can come later. It needs
  a rule against overlap first: a "hash any pointer by address"
  implementation would clash with "hash a `str.Bytes` by its
  contents", and the correct answer is to forbid the overlap, not to pick
  a winner.
- An implementation expands to ordinary functions named
  `Interface<type>.function` (`cmp.Hash<i32>.hash`), which is how they
  appear in messages, call chains, and the wasm name section.

### Constraints and calls

```lisp
(fn (get K V) [m:(ptr (Map K V)) k:K fallback:V] -> V
  (where (cmp.Hash K))
  ... (call (cmp.Hash.hash K) k) ...
  ... (call (cmp.Hash.same K) a k) ...)
```

- `(where (Interface T))` goes in a generic function's header beside its
  contracts, one clause per requirement. A generic struct or union can
  have them too: `(struct (Map K V) [...])` with `(where (cmp.Hash K))`,
  so a `(Map Point i32)` without `(Hash Point)` is an error where it is
  written.
- `(call (Interface.function t) args...)` and `(ref (Interface.function t))`
  call, or refer to, the implementation for `t`. Inside a template, `t` is
  usually a type parameter with a matching `where`; outside, a concrete
  type with an implementation.
- **Using a template checks its clauses**: every argument has the
  implementations asked for (`(map.Map Point i32) needs (cmp.Hash Point):
  write (impl (cmp.Hash Point) ...) in the module of Point or of
  cmp.Hash`). Inside a template, using another template passes clauses
  along: `get` may call `(vec.index_of K)` only if its own `where` gives
  `(cmp.Eq K)`.

### Kinds: what a sealed type parameter supports

Today a type parameter can be any type, and the allocators rely on two
kinds that cannot coexist: `(heap.create T)` returns `(ptr T)`, so `T` must
be a struct; `(heap.array T)` returns `(arr T)`, so `T` must be a value that
can be stored. The two are disjoint, because a struct exists only behind a
pointer (AIPL_SPEC.md 4.E). So a type parameter has one of two kinds:

| Kind | Written | Types | A sealed parameter supports |
|---|---|---|---|
| value (the default) | nothing | anything a struct field can hold: numbers, `bool`, `str`, pointers, arrays, function references, enums, unions | `let`, parameters, results, struct and variant fields, `(arr T)`, `(sizeof T)`, passing to and returning from functions, `arr.cast` (in `unsafe`) |
| struct | `(where (struct T))` | a struct | `(ptr T)`, `(new T)`, `(sizeof T)`, `(ptr.null T)`, `ptr.cast` (in `unsafe`); no fields: `(get p T f)` is an error |

Nothing else works on a sealed parameter: no `eq`, no arithmetic, no
`match`, no printing. Those come from interfaces, or from making the type
concrete. Neither kind admits `void` or `result` (which cannot be stored),
so `(Option void)` is rejected where it is written instead of deep inside
`std/option`.

### Checking once: archetypes

The checker does not need to learn templates. The expander makes one extra
instance of each template, at **archetypes**: every type parameter becomes
a sealed placeholder type (a new type `T`, of its kind, that the checker
knows supports only the operations above), and every `where` clause
becomes a set of declared-only functions with the interface's signatures
(`Hash<K>.hash [k:K] -> i32`). The checker checks these archetype instances
like any code; nothing is generated from them. Swift checks generics the
same way.

So the pipeline becomes: resolve, then expand at archetypes and check
(templates once), then expand at the real types and check and compile as
today. Messages from the archetype check name the parameter as written
(`eq on T: a type parameter has no eq; add (where (cmp.Eq T))`) and point at
the template's own line. The second check should never find an error
inside a template body; if it does, that is a bug in the sealing rules,
reported as such.

What can still fail where a template is used, and correctly so: a type
argument without the asked-for implementation, a type of the wrong kind,
and an instance that grows without bound (the existing 1024-character
rule).

### Standard library

A new `std/cmp` holds the interfaces and the implementations for built-in
types:

- `(cmp.Eq T)`: `same`. Implemented for `i32`, `i64`, `bool`, and `str`.
- `(cmp.Ord T)`: `compare`, with the contract above. Implemented for `i32`,
  `i64`, and `str` (bytewise).
- `(cmp.Hash T)`: `hash` and `same`. Implemented for `i32`, `i64`, `str`,
  and `str.Bytes` (by contents, as `strmap` hashes today).
- **Not for `f32`/`f64`.** NaN is not equal to itself, so equality on
  floats breaks the law every hash map relies on (`same x x`). Rust makes
  the same choice. A program that needs float keys writes a wrapper type
  and decides what NaN means.
- Enums, structs, and unions implement what they need themselves, in their
  own module. Nothing is derived automatically.

Then:

- `std/map` becomes `(map.Map K V)` with `(where (cmp.Hash K))`, replacing
  both `map` and `strmap`. `examples/word_freq.aipl` and the knucleotide
  benchmark move to it.
- `vec.index_of` gets `(where (cmp.Eq T))`; `vec.sort_by` keeps its
  comparator argument (a sort by a key other than the type's order is
  normal), and a `vec.sort` with `(where (cmp.Ord T))` joins it (built
  2026-10-20).
- `alloc`, `heap`, and `arena`: `create`, `destroy`, and `alloc` get
  `(where (struct T))`.

## Stages

Each stage lands in both toolchains, byte for byte, with tests and mutation
checks, and is merged on its own.

1. **IC1: interfaces and implementations.** Parsing (`interface`, `impl`),
   expansion of implementations into functions, the call form, coherence,
   the orphan rule, signature matching, contracts carried into
   implementations. `std/cmp`. Templates are still checked per instance.
2. **IC2: `where` clauses.** On functions, structs, and unions; checked
   wherever a template is used, with the message naming the missing
   implementation and where it may be written.
3. **IC3: kinds and archetypes.** The sealed placeholder type in both
   checkers, the archetype expansion, `(where (struct T))`. Every template
   in the repository then has to check sealed: the std changes above that
   need only a `where` clause (`index_of`, the allocators) land here.
4. **IC4: one map.** `(map.Map K V)` over `cmp.Hash`, `strmap` folded in,
   programs and benchmarks moved, benchmark times rechecked (the hash and
   equality become direct calls to the implementation; no slower than
   today is the bar).
5. **IC5: documentation.** Spec (a section after 4.H), the prompt guide
   (a rule and a checklist item), the tour, the pitfalls table, and
   LAUNCH_CHECKLIST.md 6.

Later, each when something needs it: generic implementations with a rule
against overlap; interfaces with several type parameters; default
functions in an interface.

## Alternatives considered

- **Passing the functions explicitly** (`(call (map.make K V) a cap (ref
  hash) (ref same))`): possible today with no language change, but a body
  is still checked only per instance, two maps of one key type can hash it
  two ways, and every lookup makes an indirect call.
- **Parameterised modules** (ML functors): as sound as interfaces, but
  "one meaning per type" becomes each programmer's job, and a module
  system is a larger change to the resolver than interfaces are.
- **Finding implementations implicitly** (Rust and Haskell infer which
  implementation a call means): shorter to write, but the reader, often an
  agent, has to work out which function runs. Here the call says it.
