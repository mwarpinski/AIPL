"""std/show's test vectors (tests/aipl/show/): random doubles of every
exponent, f32s, integers, and strings, each with a random format, and what
Python prints for each (repr, format(), and, for an f32, the fewest digits
that read back through the nearest f64 as AIPL reads an f32 literal).

    python3 tools/show_vectors.py tests/aipl/show [seed] [scale]

tests/test_formats.rs runs the program in every backend against the answers.
"""
import random, struct, sys
from decimal import Decimal, getcontext
getcontext().prec = 1200
random.seed(int(sys.argv[2]) if len(sys.argv) > 2 else 1)
scale = float(sys.argv[3]) if len(sys.argv) > 3 else 0.25
out_dir = sys.argv[1]

def f32_of(bits): return struct.unpack('<f', struct.pack('<I', bits))[0]
def to_f32(x):
    try: return struct.unpack('<f', struct.pack('<f', x))[0]
    except OverflowError: return float('inf') if x > 0 else float('-inf')

def layout(digits, n):
    k = len(digits)
    if -4 < n <= 16:
        if n <= 0: return "0." + "0"*(-n) + digits
        if n >= k: return digits + "0"*(n-k) + ".0"
        return digits[:n] + "." + digits[n:]
    e = n - 1
    return digits[0] + ("." + digits[1:] if k > 1 else "") + "e" + ("-" if e < 0 else "+") + ("%02d" % abs(e))

def f32_repr(x):
    # fewest digits reading back via nearest f64 then nearest f32; among
    # those, the closest to x, ties to an even last digit
    if x != x: return "nan"
    if x in (float('inf'), float('-inf')): return "inf" if x > 0 else "-inf"
    neg = struct.pack('<f', x)[3] & 0x80
    if x == 0: return "-0.0" if neg else "0.0"
    v = Decimal(abs(x))
    for p in range(1, 10):
        e = v.adjusted() - p + 1
        lo = int(v.scaleb(-e).to_integral_value(rounding="ROUND_FLOOR"))
        cands = []
        for d in (lo, lo + 1):
            if d == 0: continue
            if to_f32(float(Decimal(d).scaleb(e))) == abs(x):
                cands.append(d)
        if cands:
            best = min(cands, key=lambda d: (abs(Decimal(d).scaleb(e) - v), d % 2))
            s = str(best)
            n = len(s) + e
            s = s.rstrip("0") or "0"
            return ("-" if neg else "") + layout(s, n)
    raise Exception("no")

def spec_word(width=0, dec=-1, align=0, zero=False, base=0, plus=False, fill=0):
    return width | ((dec + 1) << 12) | (align << 18) | (1048576 if zero else 0) | (base << 21) | (8388608 if plus else 0) | (fill << 24)

def py_spec(width=0, dec=-1, align=0, zero=False, base=0, plus=False, fill=0, kind="int"):
    s = ""
    if fill: s += chr(fill)
    if align: s += "<>^"[align - 1]
    elif fill: s += ">" if kind != "str" else "<"
    if plus: s += "+"
    if zero: s += "0"
    if width: s += str(width)
    if dec >= 0: s += "." + str(dec) + "f"
    if base: s += "xXb"[base - 1]
    return s

lines, want = [], []
def case(call, expect):
    lines.append(f"    (call show.println {call})")
    want.append(expect)

def rnd_spec(kind):
    w = random.choice([0, 0, 0, 1, 5, 12, 30])
    align = random.choice([0, 0, 1, 2, 3])
    fill = random.choice([0, 0, 0, ord('*'), ord('#')]) if align else 0
    zero = kind != "str" and not fill and random.random() < 0.25
    plus = kind != "str" and random.random() < 0.2
    base = random.choice([0, 0, 0, 1, 2, 3]) if kind == "int" else 0
    dec = random.choice([-1, -1, 0, 2, 6, 17]) if kind == "float" else -1
    return dict(width=w, dec=dec, align=align, zero=zero, base=base, plus=plus, fill=fill)

# f64: random bit patterns (every exponent), random short decimals, edges
xs = [struct.unpack('<d', struct.pack('<Q', random.getrandbits(64)))[0] for _ in range(int(1500 * scale))]
xs += [round(random.uniform(-1000, 1000), random.randint(0, 6)) for _ in range(int(800 * scale))]
xs += [10.0**k for k in range(-30, 30)] + [2.0**k for k in range(-1074, 1024, 37)] + [5e-324, 1.7976931348623157e308, 0.1, 0.2, 0.3, 1/3, 2/3]
for x in xs:
    bits = struct.unpack('<q', struct.pack('<d', x))[0]
    lit = f"(f64.reinterpret_i64 {bits}i64)"
    if random.random() < 0.6 or x != x:
        sp = dict(width=0, dec=-1, align=0, zero=False, base=0, plus=False, fill=0)
    else:
        sp = rnd_spec("float")
    py = py_spec(**sp, kind="float")
    expect = format(x, py) if py else repr(x)
    if x != x: expect = format(x, py) if py else "nan"
    case(f"(call show.put_f64 (call show.start) {lit} {spec_word(**sp)})", expect)
# f32
for _ in range(int(1500 * scale)):
    bits = random.getrandbits(32)
    x = f32_of(bits)
    sbits = bits - (1 << 32) if bits >= (1 << 31) else bits
    case(f"(call show.put_f32 (call show.start) (f32.reinterpret_i32 {sbits}) 0)", f32_repr(x))
# integers
for _ in range(int(1500 * scale)):
    v = random.choice([random.randint(-2**63, 2**63 - 1), random.randint(-1000, 1000), -2**63, 2**63 - 1, 0])
    sp = rnd_spec("int")
    case(f"(call show.put_i64 (call show.start) {v}i64 {spec_word(**sp)})", format(v, py_spec(**sp)))
# text
for s in ["", "a", "abc", "héllo", "日本語", "x y"]:
    sp = rnd_spec("str")
    case(f'(call show.put_str (call show.start) "{s}" {spec_word(**sp)})', format(s, py_spec(**sp, kind="str")))

src = ";; Generated by tools/show_vectors.py; do not edit.\n(module vectors\n  (import show)\n  (fn main [] -> i32\n" + "\n".join(lines) + "\n    0))\n"
open(out_dir + "/vectors.aipl", "w").write(src)
open(out_dir + "/vectors.expected", "w").write("\n".join(want) + "\n")
print(len(want), "cases")
