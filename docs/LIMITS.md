# What the language does not do

The limits of language version 1 in one place, each with the way around it. A limit here is deliberate or not yet
lifted; either way it is the same in every backend, and a program that hits one gets an error that says so.

| Limit | Why | What to write instead |
|---|---|---|
| **A value holds at most 16 scalars.** A value struct, a union held by value, and a result are each at most 16 slots (`value struct 'Big' holds 17 scalars ...`). | A value lives in locals and in a function's results, and each scalar is one of them. | Keep the larger record behind a pointer: a struct that is not `(value)`, held as `(ptr S)`. |
| **A value cannot contain itself**, directly or through another value. | It has a fixed size. | Hold the recursive part through a pointer: a `(ptr S)` to a struct that holds it. |
| **A value's fields do not change in place.** `put` on a value struct is an error. | A value is copied where it goes, so a change would be lost. | `(with v V.x 3)`, a copy with one field replaced; or use a struct behind a pointer. |
| **`match` has no nested patterns.** `(match o (some [(ok x)] ...))` is not a pattern. | Each arm names a variant and binds its fields. | Match again inside the arm. |
| **There are no unsigned integer types.** `u8`, `i8`, `u16`, `i16` are storage types for struct fields and array elements only; reading one gives an `i32`. | One signed type per width keeps the rules few. | Use `i32` or `i64` with the unsigned operations: `divu`, `remu`, `shru`, `ltu`, `lteu`, `gtu`, `gteu`. A `u32` is an `i32`; a `u64` is an `i64`. |
| **`+ - *` on integers stop the program on overflow.** | Silent wrapping hides bugs. | `wrap.add`, `wrap.sub`, `wrap.mul` where wrapping is the point (hashes, generators, checksums). |
| **A `str` made at run time is never freed.** `format` and `str.concat` allocate one every time. | A `str` has no generation to check a use against. | Build text in a `buf` from an allocator (`buf.make a 64`, `format_to b "..."`, `buf.bytes b`), or `buf.concat a x y`; `aipl lint` finds the ones in loops. |
| **No floats as map or set keys.** `std/cmp` has no implementation for `f32` or `f64`. | NaN is not equal to itself, so equality and hashing would break the law a map relies on. | Wrap the float in a struct and decide what NaN means. |
| **Threads are experimental.** They need `unsafe`, AIPL does not check data races, and the wasi-threads proposal they use was removed from wasmtime in version 47. | See [LANGUAGE_VERSIONING.md](LANGUAGE_VERSIONING.md). | `std/par` for parallel maps and reductions on plain values, without `unsafe`. |

These were limits until 2026-11: a result could not be a struct field, an array element, or a type argument, and a
value held at most 8 scalars.
