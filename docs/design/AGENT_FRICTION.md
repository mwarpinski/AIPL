# Where an agent writing AIPL goes wrong, and what would prevent it

A running log, started 2026-10-18 while writing the standard library
(L2 to L6, about 9,000 lines of AIPL in a day). Each entry is a mistake
the agent (Claude) actually made, how often, what the toolchain said,
and the language or tool change that would have prevented it. Not every
proposal belongs in the language: each says what it would cost, and a
mistake that a clear error fixes in one round may be cheaper to keep.
New mistakes go at the end of their table with the date.

The verdicts: **fix** (worth doing, with a launch checklist item or a
plan), **error** (keep the rule, improve the message), **keep** (the
rule earns its cost), **open** (for a decision).

## Mistakes the checker caught

| # | Mistake (times, 2026-10-18) | What it said | What would prevent it | Verdict |
|---|---|---|---|---|
| 1 | A string literal inside a `{}` placeholder: `{(call f "x")}` (7) | "a placeholder cannot hold a string or an escape; bind it with let first" (clear; before the L1 work it was "unexpected input after the module's closing ')'") | Allow `\"...\"` string literals inside placeholders: the format string is already one token, so the placeholder's text can be lexed again with escapes undone. Removes the most frequent mistake of the day. | fix |
| 2 | One name, two types in one function: `w` as i32 and i64 in different branches, `e` as `url.Error` and `json.Error` in two `match_result` arms (7) | "'e' is (ptr json.Error) here but (ptr url.Error) elsewhere in this function; a name keeps one type per function (rename one)" (clear) | Scope pattern binders (`match`, `match_result` arms) to their arm: most of the seven were binders, where the second name is the natural one. Keep the rule for `let`, where a function-wide meaning helps the reader. | fix (binders only) |
| 3 | `(req ...)` after the start of a function body (1) | "Unknown op/keyword: req" | Say "req and ens go before the body; for a check here, ..." and add an `(assert e)` statement for checks in the middle of a body, which today need a helper function called from a `req`. | error, and open (`assert`) |
| 4 | A value struct with 14 fields (1) | "value struct 'Url' holds 14 scalars ...; at most 8: keep a larger record behind a pointer" (clear) | Nothing: the limit keeps values in registers, and the message says what to do. | keep |
| 5 | Unbalanced parentheses in a long nested `(or ...)` (2) | "unexpected input after the module's closing ')'" at the end of the file | Report where the imbalance starts: the first form whose indentation does not match its depth (as rustc's "unclosed delimiter" points at the opener). Variadic `and`/`or` (2026-10-18) removes most such nesting. | error |
| 6 | `(when c (call f x))` with a value-returning `f` (in the painpoints test) | "If branch type mismatch: then is i32, else is void", naming an `if` the program does not have | `when` discards its body's value (it has no else to give one); launch checklist section 2. | fix |

## Mistakes nothing caught (wrong output or wasted work)

| # | Mistake | What happened | What would prevent it | Verdict |
|---|---|---|---|---|
| 7 | Calling `buf.bytes` (and `str.slice`, `str.from_str`) in a loop | Each call makes a `(ptr str.Bytes)` with `new`, which is never freed: std/regex leaked a view on every search, std/deflate one per copied byte (found by reading, fixed). In a long-running program (an L7 server) every request would leak this way. | Make `str.Bytes` a value struct (address and length: 2 scalars), so a view costs nothing and is never allocated. A change to std's most used type, in both toolchains; the biggest item here. | open (before L7) |
| 8 | Writing `(sys.print "err\n")` | Two newlines: `sys.print` prints each argument on its own line (AIPL_SPEC.md 4), while the form `print` does not end the line | Rename `sys.print` to `sys.println`, or document the difference where both appear: two names one letter apart that disagree on the newline. | open |
| 9 | Bit constants as decimals: `-306674912` for CRC-32's `0xEDB88320`, 400 SHA and MD5 constants | No mistake reached the code only because a script made them; by hand each is a chance to get a sign or a digit wrong | Hex integer literals (`0xEDB88320`, `0x6a09e667f3bcc908i64`), read with the same signed-or-unsigned rule as decimals. | fix |
| 10 | Rotations and bitwise not written by hand: `(bitor (shl v n) (shru v (- 32 n)))`, `(^ x -1)` | None wrong, but each is a place to get a shift count wrong | `rotl`, `rotr`, `bitnot` ops (wasm has `i32.rotl`, `i32.rotr`). Small. | open |
| 11 | Tables of constants as 64 to 259 lines of `arr.set` or `cond` | Kept correct only by generating them (tools/hash_tables.py, html_tables.py, http_tables.py) | Array literals, or constant arrays (`(const K:(arr i32) [...])`). | open |
| 12 | `aipl run file.aipl` (run takes a compiled `.wasm`) | "not a valid module: expected `(`" pointing at the module name | Have `run` compile a `.aipl` first, or say "run takes a .wasm; aipl eval runs source". | error |
| 13 | Setting `AIPL_STD` for the Rust CLI | Ignored: the Rust CLI reads the standard library from the checkout it was built in | Launch checklist section 4, "A binary that works wherever it is". | fix |

## What a corpus would fix instead

Most of the remaining slowness was not a mistake but recall: the exact
name of a std function (`str.find_from`, `vec.at`), whether a module has
one (`buf.byte_at` did not exist), and the argument order. `aipl doc
MODULE` answers each in one call, and the agent used it less than it
should have. A larger body of AIPL to learn from would make this
disappear; until then the prompt guide's rule 14 carries the most-used
names.
