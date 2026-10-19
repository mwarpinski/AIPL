# A broader standard library, and string formatting

**Status: approved 2026-10-16, every decision as recommended; L1 to L5
built 2026-10-18; L6 next.** Four choices were made before writing
(2026-10-16): formatting is part of the language, not a library;
cryptography stops at hashes and HMAC; regular expressions are the
guaranteed-linear kind; the work goes in batches. At approval,
networking was added as batch L7. After a review (REVISIONS.md,
2026-10-16), `try` and `defer` joined L1 and `std/stream` L2, so that the
libraries after them are written with both.

## The gap

The standard library (AIPL_SPEC.md 12.6) covers files, text slices, a
string builder, JSON, vectors, hash maps, big integers, dates, timing,
allocators, and parallel work. A program that does ordinary work still
writes by hand what other languages import:

- **Text:** no split, trim, find-a-substring, replace, join, case
  conversion, or reading UTF-8 one character at a time; printing a
  sentence with three numbers in it takes seven calls.
- **Data:** no CSV, no statistics, no sets, no seeded random numbers, no
  command-line options.
- **Web formats:** no URLs, query strings, base64, percent-encoding, HTTP
  messages, or HTML escaping.
- **Bytes:** no checksums or hashes, no compression.
- **Patterns:** no regular expressions.

WASI preview 1 has no sockets (ROADMAP.md), so networking comes last
(L7), through AIPL's own host interface beside WASI; until then the web
libraries do everything but the sending and receiving.

## Rules every library follows

1. **Written in AIPL, over the existing primitives.** No new Rust, no new
   host imports (DEVELOPING.md, "Direction"). Each behaves the same
   in the VM, under WebAssembly, and natively, and the self-hosted
   toolchain compiles it to the Rust toolchain's bytes.
2. **Memory is the caller's.** A function that keeps memory takes an
   `alloc.Allocator` and says what to free; a function that can avoid
   allocating returns a view (`(ptr str.Bytes)`) into its input.
3. **Failure is a value.** Anything that can fail on its input (parsing a
   number, a CSV row, a URL, a pattern) returns a `result` or an
   `option.Option`, with the byte offset of the problem where there is one.
   Contracts (`req`) are for the caller's mistakes, not the input's.
4. **Checked against a reference.** Each library is tested against
   Python's standard library (`re`, `csv`, `hashlib`, `zlib`, `base64`,
   `urllib.parse`, `statistics`, `email`/`http` parsing) on generated
   inputs, the way std/json was checked against node: a script in `tools/`
   writes the vectors and the expected answers, which are checked in.
5. **No closures, so no callbacks where a value will do.** Where Python
   takes a function (a regex replacement, a CSV row handler), AIPL takes a
   template or returns the data; where a function really is needed, it is
   a reference plus a typed context (AIPL_SPEC.md 4.G).

## 1. String formatting, in the language

```lisp
(let name:str "Ada")
(let n:i32 3)
(let mean:f64 2.71828)
(println "{name} ran {n} tests, mean {mean:.2}")       ;; Ada ran 3 tests, mean 2.72
(let s:str (format "{n:>5}|{(get p Point.x)}|{{}}"))   ;; "    3|1.5|{}"
(format_to b "row {i}: {(call score i)}")              ;; appends to buf b
```

| Form | Gives | Notes |
|---|---|---|
| `(format "...")` | a new `str` | never freed, like every run-time `str` (AIPL_SPEC.md 4.B): for finished text, not a loop |
| `(format_to b "...")` | appends to `b:(ptr buf.Buf)` | for building text in a loop |
| `(print "...")`, `(println "...")` | writes to standard output | through a buffer it frees; allocates nothing that stays |
| `(eprintln "...")` | the same, to standard error | |

Inside the string, `{name}` is a variable, parameter, or constant, and
`{(expr)}` is any expression; `{{` and `}}` are literal braces. After a
colon, an optional format: `.N` decimals for a float (default: the
shortest digits that read back, as std/json writes), a width with `>` (the
default for numbers), `<`, or `^` alignment, `0` to pad numbers with
zeros, `x` / `X` for hexadecimal, `b` for binary: `{n:08x}`, `{label:<12}`.

A value is printable if it is an `i32`, `i64`, `f32`, `f64`, `bool`, `str`,
`(ptr str.Bytes)`, or an enum (its member's name). Anything else (a
pointer, an array, a value struct, a union) is a compile error that says
what to do: `{p}: (ptr Point) is not printable; format its fields, e.g.
{(get p Point.x)}`. So are a name that does not exist, an unclosed `{`, and
a format that does not fit the type (`{name:.2}` on a `str`).

**Why in the language.** AIPL has no functions that take any number of
arguments, so a library could only take its values wrapped in a list
(`(call fmt.format "x = {}" (args (fmt.i32 x)))`), and a wrong count would
show up only when it ran. As a form, the compiler sees each value's type,
checks every placeholder before the program runs, and calls the right
printer directly: as fast as the hand-written calls it replaces.

**How it runs.** Both front ends parse the string into its pieces. The
checker types each piece; then a desugaring step, in both toolchains,
rewrites the form into the `buf` and `fmt` calls a programmer would have
written (`buf.push_str`, `buf.push_int`, `fmt.push_f64_shortest`, ...),
importing `std/buf` and `std/fmt` itself if the module did not. So the VM,
the wasm backend, and the native backend need nothing new, and byte parity
is the existing kind. A format string only known at run time (read from a
file) is not supported; that is the library version this plan does not
build.

### As built (2026-10-16)

As designed, with three changes found in building it. The strings are split
in the resolver, not the constant pass, since only there are a module's
names not yet qualified: a placeholder in an imported module names that
module's private helpers. Placeholders may not hold a quote, a backslash,
or an escape, so that the Rust toolchain (which reads strings unescaped)
and the self-hosted one (which reads them raw) split every string alike.
Floats print as Python's `repr` (`100.0`, `1e+16`), and `{x:.2}` is
Python's `{x:.2f}`, so the output can be checked against Python
(tools/show_vectors.py, 1,455 cases in tests/aipl/show; 10,860 matched
when built). Printing a float or an integer now keeps no memory: io's
print functions and fmt.f64_fixed leaked a buffer per call, and
buf.push_str a slice.

## 2. Text (`str` additions, new `utf8`)

`str` gains, on `(ptr str.Bytes)` views (nothing copied unless stated):
`starts_with`, `ends_with`, `contains`, `find [b needle] -> i32` (first
index or -1; two-way search, linear time), `find_from`, `rfind`,
`count`, `trim`, `trim_start`, `trim_end`, `split [a b sep] -> (ptr (vec.Vec (ptr Bytes)))`,
`split_once [b sep] -> (option.Option Pair)`, `lines`, `fields` (split on
runs of whitespace), `join [a parts sep] -> (ptr buf.Buf)`,
`replace [a b from to] -> (ptr buf.Buf)`, `repeat`, ASCII `to_upper` /
`to_lower` / `eq_ignore_case`, `parse_i64`, `parse_f64` (exact, via
`float.from_decimal`), `parse_bool`, and `cmp` (byte order).

`utf8`: `decode [b i] -> Char` (a value struct: the code point and its
length in bytes, or an error marker), `encode [buf cp]`, `count [b]`
(characters), `is_letter` / `is_digit` / `is_space` for ASCII and the
common Unicode ranges, and `width` is left out (it needs the Unicode
tables).

### As built (2026-10-16)

The functions that build (split, fields, lines, join, replace, repeat, case,
parse_f64) are a module of their own, `std/text`, since std/str is beneath
std/vec and std/buf in the imports and cannot use them; std/str gained the
rest. `lines` follows Rust's str::lines (a last line keeps a `\r` with no
`\n` after it). std/utf8's classes are exact: Unicode's White_Space
property and the ASCII classes; letters beyond ASCII need the Unicode
tables and are left out. Checked against Python on 2,400 generated cases
when built (800 kept, tests/aipl/text).

## 2b. Early returns and cleanup (`try`, `defer`)

`(try e)`: `e` is a `(result T E)`; its `ok` value, or the function
returns `(err:R x)` with its `err` value `x` (the function returns `(result
R E)`). `(try_opt e)`: `e` is an `(option.Option T)`; its value, or the
function returns `none`. `(try_or e x)`: `e` is an Option; its value, or
the function returns `(err:R x)`. Each is a `match_result` or `match`
written for the programmer, expanded before checking, so no backend sees
it.

`(defer e ...)`: runs `e ...` when the function returns, however it
returns (falling through, `return`, `try`), in reverse order of the
defers; expanded into copies at each exit, so it costs what the
hand-written cleanup would. A trap does not run them (it stops the
program).

As built (2026-10-16): a defer is a statement of a function's body (one in
a nested block is an error), covering the returns after it in the text; it
may not itself return or try. `try` needed a language rule first, also
built: a branch that always leaves (`return`, `break`, `continue`) takes
the other branches' type, as in Rust, so `(if ok v (return e))` is a value.

The cost of L1 (2026-10-18): std/str doubled (8 KB of wasm to 16 KB), and
since std/io and std/os import it, so did nearly every program: a native
`word_count` went from 83 KB to 104 KB. A program carries every function of
every module it imports, used or not (the test runners too). Dropping the
functions nothing calls, in both toolchains with the same bytes, would pay
this back and keep later batches from adding to every program; it belongs
with the compiler's optimizer work rather than in a library batch.

## 3. Data

- **`stream`**: `Reader` and `Writer`, value structs of a state and
  function references (as `alloc.Allocator` is), over a file descriptor,
  a `buf.Buf`, a byte slice, and later a socket; `read_byte`,
  `read_line`, `read_all`, `write_bytes`, `write_str`, `flush`, buffering
  of its own. `(format_to w ...)` gains a writer form.

  As built (2026-10-18): a Reader and a Writer are each one struct behind
  a pointer, holding a union of their sources (a file descriptor, memory,
  a function of your own) or sinks (a file descriptor, a `buf.Buf`, a
  function), as `alloc.Allocator` is a union of allocators. A custom source
  appends to a `buf.Buf` it is given and a custom sink takes a
  `(ptr str.Bytes)`, so neither sees a raw address and neither needs
  `unsafe`. Reading a line allocates nothing: the line is a view into the
  reader's window, reused, good until the next read. A writer to a
  `buf.Buf` holds nothing back (its buffer is that buffer). Every OS error
  is -1, so a stream records only that one happened (`read_error`,
  `write_error`). `(format_to w ...)` takes a writer only as a variable,
  since the lowering names it twice (its buffer, then the flush check).
  Building it found a compiler bug: a result cell gave a pointer payload 4
  bytes of an 8-byte cell, so `(ok:str p)` wrote the pointer's generation
  over the next object's (fixed in both toolchains, ed12333).
- **`csv`**: RFC 4180 reading (quoted fields, doubled quotes, newlines
  inside quotes, CRLF or LF, a chosen delimiter, an optional header row)
  into rows of `(ptr str.Bytes)` fields, both all at once and one row at a
  time for big files; a reader keyed by header name (`csv.column r "price"`);
  typed helpers (`csv.f64_at`, `csv.i64_at`, returning results with the
  row and column of a bad value); writing with quoting where needed.
  Checked against Python's `csv`.

  As built (2026-10-18): the reader is Python's _csv.c state machine,
  non-strict, over a `stream.Reader`, with Python's universal-newline line
  splitting, so a lone `\r` ends a line as Python's reader sees it. Fields
  are views into one reused buffer (no allocation per row). Writing is per
  field (`field_str`, `field_i64`, `field_f64`, then `end_row`) rather than
  from an array. 3,600 generated cases match Python 3.14; Python's field
  size limit (131,072 bytes) is not kept.
- **`stats`** on `(arr f64)`: `sum` (compensated), `mean`, `median`,
  `quantile` (Python's default method, and the others by name),
  `variance` and `stdev` (sample and population), `min`, `max`, `mode`,
  `histogram`, `covariance`, `correlation`, `linear_fit` (least squares:
  slope, intercept, r²), `zscore`. Checked against Python's `statistics`
  to the last bit where Python is exact.

  As built (2026-10-18): to the last bit everywhere, about 44,000 values
  over six generated seeds. Python's mean, variance, and stdev round an
  exact fraction once; here the exact values are expansions (Shewchuk's
  partials, Dekker's exact products) and the rounding is found by exact
  sign tests at the midpoints, so no big integers are needed. Python's
  sumprod is ported from CPython's triple-length accumulation, an exact
  product standing in for fma. linear_fit adds r2 (the correlation
  squared).
- **`set`**: a hash set `(set.Set K)` over std/cmp's `Hash` and `Eq`
  (add, has, remove, count, union, intersection, difference, items).
- **`random`**: a seeded generator (PCG-32, a value struct, so a seed
  gives the same numbers in every backend), `next_u32`, `range [lo hi]`
  (unbiased), `f64` in [0, 1), `normal` (mean and deviation), `shuffle`,
  `choice`, `sample`; `random.seeded_from_os` for a seed from the system.
  `uuid.v4` from the system's random bytes.
- **`cli`**: command-line options. A program declares its options
  (`cli.flag`, `cli.option`, `cli.positional`, with help text and
  defaults), then `cli.parse` reads `os.arg` into a result: the values by
  name (`cli.get_str`, `cli.get_i64`, `cli.get_bool`), or an error naming
  the bad argument. `--help` prints generated usage. Long and short
  forms, `--name=value`, `--` to end options.
- **`path`**: `join`, `parent`, `file_name`, `stem`, `extension`,
  `normalize` (`.` and `..`), `is_absolute`; text only, no file access.

As built (2026-10-18): set is a layer over std/map; random is pcg32 with
its reference numbers checked, and normal uses a port of fdlibm's log
(no std math module yet), checked against Python's math.log; path follows
posixpath (1,600 generated paths match); cli parses lists too
(`parse_list`), so a program's handling of its options can be tested
without starting it. Fixed on the way: `aipl eval` now exits with the
status a program gives sys.exit, as compiled programs do.

A table type (a small data frame: typed columns, filter, group by,
aggregate) is the obvious next step after these and is left for a later
plan: it is a design of its own, and these are what it would be built on.

## 4. Encodings and web formats

- **`encode`**: `base64` (standard and URL-safe, with and without
  padding), `hex`, percent-encoding (`percent_encode` with the set of
  characters to keep, `percent_decode`), each as encode-into-a-buf and
  decode-returning-a-result.
- **`url`**: RFC 3986 parsing into scheme, user info, host, port, path,
  query, and fragment (views, plus decoded forms on request); building one
  back; `url.resolve [base relative]`; query strings both ways
  (`url.query_pairs`, `url.query_get "page"`, `url.query_build`).
  Checked against Python's `urllib.parse`.
- **`http`**: HTTP/1.1 messages without the network: parse a request
  (method, target, version, headers, body) or a response, with headers
  looked up case-insensitively, `Content-Length` and chunked bodies, and
  limits on sizes; write requests and responses; status codes with their
  reason phrases; `http.mime_type "page.html"`. A program can read a
  saved request, or one piped through stdin, and answer it on stdout.
- **`html`**: `escape` (text and attribute values) and `unescape`
  (named entities for the common set, and numeric ones).
- **`json`** exists (AIPL_SPEC.md 12.6); it gains `json.get_path` for
  `"a.b[2].c"` lookups.

As built (2026-10-18), `encode`: the alphabet and the padding are enums
(`encode.Alphabet.url_safe`, `encode.Padding.unpadded`) rather than
flags, so a call says which it means. Decoding accepts exactly what
Python's `binascii.a2b_base64(strict_mode=True)` and `unhexlify` accept,
with the index of the first byte it refuses as the error, and checks
before writing, so a failed decode leaves the buffer as it was.
`percent_decode` refuses a `%` without two hex digits where Python's
`unquote` passes it through. 1,650 generated cases match Python.

## 5. Hashes (`hash`)

CRC-32 (zlib's), Adler-32, MD5, SHA-1, SHA-256, SHA-512, and HMAC over
SHA-256 and SHA-512, each as one call on bytes and as a streaming state
(`sha256_init`, `sha256_update`, `sha256_final`) for big inputs; digests as
bytes or hex; `hash.constant_time_eq` for comparing digests and tokens.
Checked against the published test vectors (NIST, RFC 1321, RFC 4231) and
against Python's `hashlib` on random inputs of every length up to a few
blocks. MD5 and SHA-1 are documented as checksums only (both are broken
for security). No encryption, as decided: ours would be unaudited.

As built (2026-10-18): one `hash.Hasher` for all four hashes, chosen by
`hash.Algorithm` (`make`, `update`, `update_reader` for a whole stream,
`finish_to`, `free`), rather than a set of functions per hash; HMAC works
over any of them. The constant tables are computed exactly with integer
roots by tools/hash_tables.py, which writes them into hash.aipl. 772
generated cases match Python's hashlib, hmac, and zlib, NIST's examples
and RFC 4231's cases among them. CRC-32 goes bit by bit (no table yet);
deflate (L5) may want a faster one.

## 6. Regular expressions (`regex`)

The guaranteed-linear kind, as in Go and Rust: a pattern is compiled to a
program for a Pike virtual machine, which runs every possible match at
once, so matching takes time proportional to the pattern size times the
text length, whatever the input. No backreferences, no lookaround (those
are what make some patterns exponential elsewhere).

Syntax: literals and escapes, `.`, classes `[a-z]`, `[^...]`, `\d \w \s`
and their negations, anchors `^ $ \b`, groups (capturing, `(?:...)`
non-capturing, `(?P<name>...)` named), alternation `|`, quantifiers
`* + ? {m} {m,} {m,n}` and their lazy forms, and the flags `(?i)`
(ASCII case-insensitive), `(?m)` (multi-line anchors), `(?s)` (`.`
matches newline). A bad pattern is an error with its offset and the
reason (`unclosed group at 4`).

API: `regex.compile [a pattern] -> (result (ptr Regex) Error)`,
`is_match`, `find` (the first match's span), `captures` (each group's
span, or none), `find_all`, `replace_all` with a template (`"$1-$name"`),
and `split`. Checked against Python's `re` on generated patterns and
texts, for patterns in the common subset of the two syntaxes. A literal
prefix is found with `str.find` first, so plain-text searches run at
search speed.

As built (2026-10-18): matches are Python's under `re.ASCII`, including
its rule for loops whose body can match nothing (an optional pass that
matches nothing ends the loop). A plain Pike VM gets those wrong in rare
cases (9 of 4,529 generated cases at first, all `(x*?)+`-shaped), so such
a loop's body compiles to two copies, before and after reading a
character, which keeps the machine exact and linear. 18,116 cases over
four seeds match. Text is UTF-8 (`.` and classes match a character; a
byte that is not UTF-8 matches as U+FFFD) and spans are byte offsets;
`\d \w \s \b` are ASCII, as in RE2. Flags go at the start only (as
Python requires); scoped flags, lookaround, backreferences, and possessive
quantifiers are errors. `split` gives only the text between matches
(Python's also gives the groups).

## 7. Compression (`deflate`)

DEFLATE (RFC 1951), with the zlib (RFC 1950) and gzip (RFC 1952)
wrappers, both ways. Decompression handles every block type. Compression
uses LZ77 with hash chains and dynamic Huffman codes, at levels 1 (fast)
to 9 (smallest), plus 0 (stored). Checked both ways against Python's
`zlib` and `gzip`: everything we compress, zlib decompresses to the same
bytes, and the reverse, on text, random data, and degenerate inputs (all
one byte, empty). Sizes are reported against zlib's at the same level;
matching them byte for byte is not a goal.

With `deflate` and `hash.crc32`, writing a PNG is a short program; it is
one of the showcase programs (docs/LAUNCH_CHECKLIST.md), not a library.

As built (2026-10-18): one module, `deflate`, over whole buffers (a
stream version can come with `http`, when a body arrives in pieces). The
decoder is puff.c's (zlib's reference decoder), so it refuses what zlib
refuses; every decoder takes an output limit, so a small input cannot
make a huge one. The raw and zlib decoders give the input bytes the
stream took and leave what follows to the caller (Python ignores it);
gzip reads every member, as Python's gzip does, but refuses reserved
header flags as zlib does (Python ignores them). The compressor picks
stored, fixed, or dynamic codes per block by size and stays within 1% of
zlib's output at every level on the spec's text (smaller at 1 to 3);
nine compressions of 60 KB take 63 ms natively. 544 generated cases
agree with Python both ways; our 133 compressed streams are checked by
Python when the answers are made. Added on the way: `buf.byte_at`, for
reading back without making a view (buf.bytes makes one each call).

## Batches

Each batch is one branch, tested in every backend and both toolchains,
documented (AIPL_SPEC.md 12.6, the prompt guide, the tour where it
helps), and merged to development before the next starts.

| Batch | Contents | Why here |
|---|---|---|
| L1 | formatting in the language; `str` additions, `utf8`; `(try ...)`, `(try_opt ...)`, `(try_or ...)`, and `(defer ...)` | every program prints and handles text, and every library returns results and frees what it allocates; the rest are written with these |
| L2 | `std/stream` (readers and writers over files, buffers, and later sockets); `csv`, `stats`, `set`, `random`, `uuid`, `cli`, `path` | a stream first, so `csv` (and later `deflate`, `hash`, `http`) read and write streams rather than whole files |
| L3 | `encode`, `hash` | small, exact, and needed by L5 and L6 |
| L4 | `regex` | the largest single library |
| L5 | `deflate`, zlib, gzip | needs `hash.crc32` / `adler32` |
| L6 | `url`, `http`, `html`, `json.get_path` | needs `encode` and `str` |
| L7 | networking: an `aipl` host interface (versioned imports beside WASI preview 1: listening and connecting TCP sockets, permissions granted on the command line, `aipl run --allow-net=:8080`), in the VM, natively as Linux system calls, and in aipl-run; then `http` serving and fetching over it | its own design doc first; WebAssembly stays the portable target |

## Decisions (approved as recommended, 2026-10-16)

1. **The formatting forms' names:** `format`, `format_to`, `print`,
   `println`, `eprintln` as built-in forms (recommended), or one form with
   a mode (`(fmt :str "...")`). Built-in names cannot be used for
   functions, as `make` cannot today.
2. **Placeholders allow any expression** in `{(...)}` (recommended), or
   names only, which keeps strings shorter but means a `let` for every
   field read.
3. **Default float printing:** shortest digits that read back
   (recommended; `0.1` prints `0.1`), or a fixed six decimals as C does.
4. **The data-frame table** waits for a later plan (recommended), or is
   added to L2.
5. **Order of batches** as in the table (recommended), or another.
