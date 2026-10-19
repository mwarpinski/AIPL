"""std/deflate's test vectors (tests/aipl/deflate/), both ways against
Python's zlib and gzip:

- streams Python makes (raw at every level and with the Huffman-only, RLE,
  and fixed strategies, so every block type appears; zlib and gzip; a gzip
  member with a name and a comment, two members, zero padding)
  decompressed by std/deflate to the same bytes;
- Python's streams with one bit flipped or cut short: std/deflate refuses
  what zlib and gzip refuse and otherwise gives the same bytes;
- what std/deflate compresses (raw, zlib, gzip, levels 0 to 9)
  decompressed by Python to the input. Those bytes are std/deflate's own,
  so this script runs a copy of the program that prints them (with the
  aipl binary given), checks each with Python, and keeps their length and
  CRC-32 as the answers: the test checks every backend makes the same.

    python3 tools/deflate_vectors.py tests/aipl/deflate target/release/aipl [seed]

Each input is a hex unit repeated a number of times (so a long run stays
short), written once in the program and named by its number. Each case
prints one line: a length and CRC-32, or "err".
"""
import gzip, os, random, subprocess, sys, tempfile, zlib

out, aipl = sys.argv[1], sys.argv[2]
random.seed(int(sys.argv[3]) if len(sys.argv) > 3 else 1)

words = ["the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog", "data", "compress", "\n", ", ", "AIPL"]

def text(n):
    s = []
    while sum(map(len, s)) < n:
        s.append(random.choice(words) + " ")
    return "".join(s)[:n].encode()

def noise(n):
    return bytes(random.randrange(256) for _ in range(n))

# (unit, count): the input is unit repeated count times
inputs = [(b"", 1), (b"a", 1), (b"ab", 1), (b"\x00", 70000), (b"xyz", 400), (b"ab", 129),
          (text(40), 1), (text(300), 1), (text(3000), 1), (text(5000), 1),
          (noise(1), 1), (noise(100), 1), (noise(1200), 1), (noise(37), 300)]

def s32(v):
    return v - (1 << 32) if v >= 1 << 31 else v

def summary(b):
    return f"{len(b)} {s32(zlib.crc32(b))}"

def raw(data, level, strategy=zlib.Z_DEFAULT_STRATEGY):
    c = zlib.compressobj(level, zlib.DEFLATED, -15, 9, strategy)
    return c.compress(data) + c.flush()

def wrap(kind, data, level):
    return raw(data, level) if kind == "raw" else zlib.compress(data, level) if kind == "zlib" else gzip.compress(data, level, mtime=0)

lines, want = [], []
def case(call, result):
    lines.append("    " + call)
    want.append(result)

# Python's streams, decompressed (not the long run: its stored stream would
# be 140 KB of hex here; api.aipl takes it through std/deflate both ways)
for k, (unit, count) in enumerate(inputs):
    data = unit * count
    if count >= 70000:
        continue
    # keep the source small: noise compresses alike at every level, and a
    # long input stored or Huffman-only is as long as itself
    big = len(data) > 3000
    is_noise = k >= 10
    for level in (((6,) if big else (0, 6)) if is_noise else range(1, 10) if big else range(10)):
        case(f'(call unpack "{raw(data, level).hex()}" "raw")', summary(data))
    for strategy in ((zlib.Z_FIXED,) if big else (zlib.Z_HUFFMAN_ONLY, zlib.Z_RLE, zlib.Z_FIXED)):
        case(f'(call unpack "{raw(data, 6, strategy).hex()}" "raw")', summary(data))
    for kind in ("zlib", "gzip"):
        case(f'(call unpack "{wrap(kind, data, random.choice([1, 6, 9])).hex()}" "{kind}")', summary(data))
t1, t2 = text(500), text(80)
named = bytearray(gzip.compress(t1, 6, mtime=0))
named[3] = 8 | 16
named[10:10] = b"file.txt\x00a comment\x00"
for stream, back in [(bytes(named), t1), (gzip.compress(t1, mtime=0) + gzip.compress(t2, mtime=0), t1 + t2),
                     (gzip.compress(t2, mtime=0) + b"\x00" * 7, t2), (b"", b"")]:
    case(f'(call unpack "{stream.hex()}" "gzip")', summary(back))

# one bit flipped, or cut short
def python_result(kind, stream):
    try:
        if kind == "raw":
            return summary(zlib.decompress(stream, -15))
        if kind == "zlib":
            return summary(zlib.decompress(stream))
        # reserved header flags must be 0 (RFC 1952); zlib refuses them,
        # though Python's gzip module does not look
        if len(stream) > 3 and stream[3] & 0xE0:
            return "err"
        return summary(gzip.decompress(stream))
    except Exception:
        return "err"
for _ in range(250):
    data = text(random.choice([20, 120, 400])) if random.random() < 0.7 else noise(80)
    kind = random.choice(["raw", "zlib", "gzip"])
    stream = bytearray(wrap(kind, data, random.randrange(10)))
    if random.random() < 0.25 or not stream:
        stream = stream[:random.randrange(len(stream) + 1)]
    else:
        i = random.randrange(len(stream))
        stream[i] ^= 1 << random.randrange(8)
    case(f'(call unpack "{bytes(stream).hex()}" "{kind}")', python_result(kind, bytes(stream)))

# std/deflate's streams, checked by Python
packs = []
for k, (unit, count) in enumerate(inputs):
    for level in ((0, 6, 9) if count >= 70000 else range(10)):
        kind = random.choice(["raw", "zlib", "gzip"])
        packs.append((len(lines), unit * count, level, kind))
        lines.append(f'    (call pack {k} {level} "{kind}")')
        want.append(None)

source = "\n".join(f'      ((eq k {k}) (call input "{unit.hex()}" {count}))' for k, (unit, count) in enumerate(inputs))

def program(show_hex):
    shown = '''    (let h:(ptr buf.Buf) (call buf.make a (+ 1 (* 2 (call buf.len o)))))
    (call encode.hex_to h (call buf.bytes o) false)
    (call buf.push_byte h 10)
    (call buf.print h)
    (call buf.free h)''' if show_hex else '''    (println "{(call buf.len o)} {(call hash.crc32 (call buf.bytes o))}")'''
    return ''';; Generated by tools/deflate_vectors.py; do not edit.
(module vectors
  (import deflate)
  (import encode)
  (import hash)
  (import str)
  (import buf)
  (import alloc)

  ;; unit (hex) count times
  (fn input [unit:str count:i32] -> (ptr str.Bytes)
    (let a:alloc.Allocator (call alloc.default))
    (let u:(ptr buf.Buf) (call buf.make a 16))
    (match_result (call encode.hex_decode_to u (call str.from_str unit))
      (ok _n (block))
      (err _at (block)))
    (let o:(ptr buf.Buf) (call buf.make a (+ 16 (* count (call buf.len u)))))
    (let ub:(ptr str.Bytes) (call buf.bytes u))
    (for _k 0 count (call buf.push_bytes o ub))
    (call buf.bytes o))

  ;; input k
  (fn source [k:i32] -> (ptr str.Bytes)
    (cond
''' + source + '''
      (else (call input "" 0))))

  (fn unpack [stream:str kind:str] -> void
    (let a:alloc.Allocator (call alloc.default))
    (let s:(ptr str.Bytes) (call input stream 1))
    (let o:(ptr buf.Buf) (call buf.make a 64))
    (let r:(result i32 (ptr deflate.Error))
      (cond
        ((call str.same kind "raw") (call deflate.inflate_to a o s 1000000))
        ((call str.same kind "zlib") (call deflate.zlib_decompress_to a o s 1000000))
        (else (call deflate.gzip_decompress_to a o s 1000000))))
    (match_result r
      (ok _used (println "{(call buf.len o)} {(call hash.crc32 (call buf.bytes o))}"))
      (err e (call deflate.free_error a e) (sys.print "err")))
    (call buf.free o))

  (fn pack [k:i32 level:i32 kind:str] -> void
    (let a:alloc.Allocator (call alloc.default))
    (let data:(ptr str.Bytes) (call source k))
    (let o:(ptr buf.Buf) (call buf.make a 64))
    (cond
      ((call str.same kind "raw") (call deflate.compress_to a o data level))
      ((call str.same kind "zlib") (call deflate.zlib_compress_to a o data level))
      (else (call deflate.gzip_compress_to a o data level)))
''' + shown + '''
    (call buf.free o))

  (fn main [] -> i32
''' + "\n".join(lines) + "\n    0))\n"

os.makedirs(out, exist_ok=True)
open(f"{out}/vectors.aipl", "w").write(program(False))
# std/deflate's own streams: run the copy that prints them, check each
with tempfile.TemporaryDirectory() as tmp:
    open(f"{tmp}/vectors.aipl", "w").write(program(True))
    run = subprocess.run([aipl, "eval", f"{tmp}/vectors.aipl"], capture_output=True, text=True, check=True)
got = [l for l in run.stdout.splitlines() if not l.startswith("[AIPL")]
assert len(got) == len(lines), (len(got), len(lines))
pack_rows = {p[0] for p in packs}
for i, data, level, kind in packs:
    stream = bytes.fromhex(got[i])
    back = zlib.decompress(stream, -15) if kind == "raw" else zlib.decompress(stream) if kind == "zlib" else gzip.decompress(stream)
    assert back == data, f"case {i + 1}: Python reads std/deflate's {kind} stream at level {level} wrongly"
    want[i] = summary(stream)
bad = [i + 1 for i, (g, w) in enumerate(zip(got, want)) if g != w and i not in pack_rows]
open(f"{out}/vectors.expected", "w").write("\n".join(want) + "\n")
print(f"{len(lines)} cases; {len(packs)} streams of ours read back by Python; differing from Python: {bad[:20]}")
