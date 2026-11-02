# Small structs by value

**Status: approved 2026-10-14; V1-V4 done 2026-10-15 (every backend
runs value structs; nbody, std, and the docs use them); V5 done
2026-10-15: unions, `Option`, and results by value (below).** Every decision at the end was taken
as recommended. Built after the native rewrite
(docs/design/NATIVE_SPEED_PLAN.md), per decision 6.

## The gap

A struct exists only behind a pointer (AIPL_SPEC.md 4.E): `(new Point)`
takes 8 bytes from `mem.alloc`, never freed, and a `Point` is used through
a `(ptr Point)`. Unions, results, and `Option`s are the same: every `make`,
`ok`, and `err` allocates a heap cell that is never freed (4.F, 4.J).

That costs three things:

- **Memory that only grows.** A function that returns two numbers
  allocates a cell per call. `std/map` has `get_or` and no `get` returning
  an `(option.Option V)`, because each lookup would leak a cell; `std/json`
  leaks a cell per value it parses.
- **Arrays of records are arrays of pointers.** 1,000 points are an
  `(arr (ptr Point))` of 4-byte addresses plus 1,000 separate 8-byte cells:
  50% more memory, a pointer chase per element, and nothing contiguous
  for the cache. `benchmarks/nbody` keeps seven parallel `(arr f64)`s
  rather than an array of bodies for this reason.
- **Code that says less than it means.** A point that is never shared or
  changed still has an address, a possible null, and a lifetime; a reader
  (or an agent) cannot tell from `(ptr Point)` that it is just two numbers.

C, Rust, Go, Swift, and C# all have structs by value. The plan below takes
the part that does not bring their pitfalls with it.

## The design

**A value struct is declared as one, and is immutable.**

```lisp
(struct Vec3 [x:f64 y:f64 z:f64] (value) (pub))

(fn add [a:Vec3 b:Vec3] -> Vec3
  (make Vec3 (+ (get a Vec3.x) (get b Vec3.x))
             (+ (get a Vec3.y) (get b Vec3.y))
             (+ (get a Vec3.z) (get b Vec3.z))))
```

| Form | Meaning |
|---|---|
| `(struct Name [fields] (value))` | a value struct. Its fields are scalars, enums, `(ptr S)`, `(arr T)`, function references, and other value structs; it may not contain itself, directly or through another value struct |
| `Name` as a type | the value itself, everywhere a type goes: parameters, results, `let`, struct and variant fields, `(arr Name)`, type arguments. `(ptr Name)` is an error: a value has no address |
| `(make Name v ...)` | a new value, one operand per field in declaration order (as `make` builds a union variant) |
| `(get v Name.f)` | a field of the value |
| `(with v Name.f x)` | a copy of `v` with field `f` replaced by `x` |
| a value-struct field of a struct, a variant, or an array element | stored inline, laid out as a struct inside the outer one (aligned to its largest field) |
| `(get p Outer.pos Vec3.x)`, `(put p Outer.pos Vec3.x 1.0)` | a field path: reads or writes one field of a value stored inline, without copying the whole value |
| `(arr.get Body bs i Body.vx)`, `(arr.set Body bs i Body.vx v)` | the same for an element of an array of value structs |

Why each choice:

1. **Declared, not "any struct by value".** In C, Go, and Rust the same
   struct can be a value in one place and behind a pointer in another, and
   the classic bug is changing a copy while meaning the original (Go's
   `for _, p := range points { p.x = 1 }` changes nothing). Here a type is
   one or the other, forever: `Vec3` is a value, `(ptr Node)` is a
   reference, and nothing converts between them.
2. **Immutable.** A value cannot be changed, only replaced (`with`), so it
   does not matter whether the compiler copies it or shares it, and there
   is no "did I change the copy?" question to get wrong. Where a value
   sits in memory that can change (a field, an array element), a field
   path writes one field in place: what numeric code needs, without making
   the value itself mutable.
3. **Small.** A value struct holds at most 8 scalars once nested value
   structs are flattened (64 bytes at most). Every copy is then a few
   register or memory moves, and a function can return one in registers.
   Larger records stay behind pointers, where a copy cannot be hidden.
4. **No built-in equality.** As with unions, `eq` does not apply. A type
   that needs it implements `cmp.Eq` (AIPL_SPEC.md 4.H), which says what
   equal means (`0.0` and `-0.0`, NaN fields) instead of comparing bytes.

Generic code works unchanged: `(vec.Vec Vec3)` stores its elements
inline, `(sizeof Vec3)` is 24, and a value struct is a type argument of the
value kind. `(where (struct T))` still means a pointer struct.

### How it runs

Nothing about values is visible at run time except speed and memory, since
they cannot be changed or compared by address. The backends:

- **wasm:** a value is its fields, flattened: a `Vec3` local is three wasm
  locals, a `Vec3` parameter three parameters, and a `Vec3` result three
  results (wasm's multi-value, standard since wasm 2.0 and on by default in
  wasmtime). A block never yields one: an `if`, `cond`, `match`, or
  `match_result` that yields a value writes it into scratch locals from
  each branch and pushes them after its end, so only calls have several
  results, which is all the native backend has to support. `with`, and a
  field read from a value that is computed rather than held (a call's
  result), go through scratch locals too. In memory a value is laid out
  inline as above, and moved one scalar at a time; a store first puts the
  value in scratch locals, since a wasm store needs its address just
  under its value.
- **VM:** a value is a `Value::Record` of its fields.
- **Native:** the translator gains functions with several results. One
  result still comes back in rax; several go through a results area of
  8 slots in the thread's FS block (after its stack limit): the callee
  stores them there just before it returns, and the caller loads them
  into value registers right after the call. Registers would be faster
  but run out at 8 results (rax and the other registers a call may
  change are 7); V4's measurements say whether it matters. This is the part that depends on the
  native rewrite (docs/design/NATIVE_SPEED_PLAN.md): built into the new
  translator once, rather than into the old one and again later.
- **The self-hosted toolchain** does all of it too, byte for byte, as for
  every feature.

### Then: unions and results by value

Unions and results are already immutable and opaque (no `eq`, no address,
no null), so they can also be values without changing what any program
means. A union that does not contain itself and fits in 8 scalars (its tag
plus every variant's fields) would become a value: `(option.Option i32)` is
two numbers instead of a heap cell, `(result f64 (ptr Error))` three. A
union that contains itself (a list, a tree) stays a cell, as it must. That
removes the leak behind `map.get_or`, makes `json.Value` (which contains
itself) the main remaining cell, and lets `std/map` offer
`get -> (option.Option V)`. It is a later stage because it changes the
memory layout that the differential tests check address by address.

## Stages

| Stage | What | Done when |
|---|---|---|
| V1 | Parser, checker, and VM: `(value)`, `make`, `get`, `with`, inline fields and arrays, field paths, the size and recursion rules, with their messages; both toolchains | the checker tests (both toolchains agree on every message) and VM tests pass (done 2026-10-14) |
| V2 | wasm backend: flattened locals, parameters, multi-value results, scratch locals for branches that yield a value, inline loads and stores; `codegen.aipl` byte for byte | test_selfhost, test_differential, test_values (done 2026-10-15) |
| V3 | Native: several results per function, in the new translator | test_native, `tools/run_fuzz.py` with value structs in its generator (done 2026-10-15) |
| V4 | Library and measurements: `benchmarks/nbody` with an `(arr Body)`; `std/time`, `std/fs`, `std/json` use values where they return several numbers; spec, prompt guide, FAQ | benchmarks measured before and after in docs/BENCHMARKS.md (done 2026-10-15: nbody 1.9x faster under wasmtime, 22% natively) |
| V5 | Unions, results, and `Option` by value when they do not contain themselves and fit; `map.get` | the whole suite, and the leak gone (a test that a million `map.get`s allocate nothing) (done 2026-10-15; `map.get` is `map.find`) |

## V5 as built (2026-10-15)

- **Shared slots.** A union by value is its tag, then slots its variants
  share by kind (i32-sized, f32, i64, f64), each variant taking the first
  of each kind it needs, rather than every variant's fields in a row:
  `alloc.Allocator`, four variants of one pointer, is 2 numbers rather
  than 5. It is in every container and passed to every allocation, and
  at 5 a call with two more arguments would no longer fit the native
  backend's six argument registers. Unused slots hold zero.
- **The rule** (AIPL_SPEC.md 4.J): a union is held by value unless it
  contains itself, a variant holds a `str` (as for value structs), or it
  needs more than 8 slots, counting a union it holds as its slots if that
  one is a value, else one; each union is decided after those it holds.
- **`map.find`, not `map.get`:** a generic function may not take a
  built-in form's name. There was in fact no leak behind `map.get_or`; what
  V5 gives is a lookup returning an `Option` that allocates nothing (the
  test does a million).
- **Results by value need their whole type written.** Building one needs
  its full type, which `(ok v)` did not say (the missing side defaulted to
  `i32`). Decided 2026-10-15: every `ok` and `err` names its other side,
  `(ok:E v)` / `(err:T e)`, rather than both compilers inferring it from
  context; about 60 sites in the repository were annotated. A result is
  then held by value unless a side is a `str` or it needs more than 8
  slots, laid out like a two-variant union.
- Found on the way: a module with a generic function named `make` (as
  `std/vec` and `std/map` have) could not use the `make` form, and the
  Rust toolchain named the wrong file in a line table where a type
  argument and a template token shared a line and column. Both fixed.

## Alternatives considered

- **Any struct by value, as in C and Go.** More flexible, and it brings the
  copy-or-original bug class and two meanings for every struct name. A
  pointer struct holding a value struct covers the cases where a record
  needs an address.
- **Mutable values.** Rust makes this safe with ownership rules AIPL does
  not have; C# and Go show the cost of not having them (a change to a
  copy is silently lost). Field paths give the speed without it.
- **Returning through memory instead of multi-value** (a caller-reserved
  area, as C does for large structs on wasm). It needs a stack in linear
  memory, which AIPL does not have, and is slower for small values; the
  size limit keeps multi-value results short.
- **Escape analysis** (keep `new` and let the compiler put cells on a
  stack when they do not escape). Invisible in the source, so whether code
  is fast depends on an analysis no reader can see; the explicit type says
  it.

## Decisions (taken as recommended, 2026-10-14)

1. **Declared value structs, `(value)` after the field list** (recommended),
   or any struct usable by value.
2. **Immutable, with `with` and field paths** (recommended), or mutable
   values.
3. **At most 8 scalars** (recommended; 64 bytes, nbody's body of seven
   `f64`s fits), a different limit, or none.
4. **No built-in `eq`; implement `cmp.Eq`** (recommended), or bytewise `eq`.
5. **Unions, results, and `Option` by value as stage V5** (recommended), or
   leave them as cells.
6. **Order: the native rewrite first, then V1-V5** (recommended), or V1-V2
   first with native support following (value structs would then be
   rejected by `compile --exe` until V3).
