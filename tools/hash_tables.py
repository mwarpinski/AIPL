"""The constant tables in aipl_src/std/hash.aipl, computed exactly: SHA-2's
starting values and round constants are the first 32 or 64 bits of the
fractional parts of the square and cube roots of the first primes (FIPS
180-4, 4.2.2, 4.2.3, 5.3.3, 5.3.5), found here with integer roots, not
floats; MD5's are floor(abs(sin(i + 1)) * 2^32) (RFC 1321, 3.4).

    python3 tools/hash_tables.py aipl_src/std/hash.aipl

rewrites the functions between the `;; @tables` and `;; @end` markers
in place (md5_tables, sha256_tables, sha512_tables), the numbers as
signed decimals; run with no argument, it prints them. The asserts
check the first and last of each against the standards' printed tables.
"""
import math, re, sys

def iroot(n, k):  # floor(n ** (1/k)), exactly
    x = 1 << ((n.bit_length() + k - 1) // k)
    while True:
        y = ((k - 1) * x + n // x ** (k - 1)) // k
        if y >= x:
            break
        x = y
    while x ** k > n:
        x -= 1
    while (x + 1) ** k <= n:
        x += 1
    return x

primes = [p for p in range(2, 500) if all(p % d for d in range(2, int(p ** 0.5) + 1))]
M32, M64 = (1 << 32) - 1, (1 << 64) - 1
k256 = [iroot(p << 96, 3) & M32 for p in primes[:64]]
h256 = [iroot(p << 64, 2) & M32 for p in primes[:8]]
k512 = [iroot(p << 192, 3) & M64 for p in primes[:80]]
h512 = [iroot(p << 128, 2) & M64 for p in primes[:8]]
md5k = [int(abs(math.sin(i + 1)) * 2 ** 32) & M32 for i in range(64)]
assert (k256[0], k256[63], h256[0], h256[7]) == (0x428a2f98, 0xc67178f2, 0x6a09e667, 0x5be0cd19)
assert (k512[0], k512[79], h512[0], h512[7]) == (0x428a2f98d728ae22, 0x6c44198c4a475817, 0x6a09e667f3bcc908, 0x5be0cd19137e2179)
assert (md5k[0], md5k[63]) == (0xd76aa478, 0xeb86d391)

def s32(v):
    return v - (1 << 32) if v >= 1 << 31 else v

def s64(v):
    return v - (1 << 64) if v >= 1 << 63 else v

def sets(name, t, vals, signed, suffix):
    return [f"    (arr.set {t} {name} {i} {signed(v)}{suffix})" for i, v in enumerate(vals)]

def fn(head, body):
    return f"  (fn {head} -> void\n" + "\n".join(body) + ")\n"

md5_h = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476]
text = "\n".join([
    fn("md5_tables [h:(arr i32) k:(arr i32)]", sets("h", "i32", md5_h, s32, "") + sets("k", "i32", md5k, s32, "")),
    fn("sha256_tables [h:(arr i32) k:(arr i32)]", sets("h", "i32", h256, s32, "") + sets("k", "i32", k256, s32, "")),
    fn("sha512_tables [h:(arr i64) k:(arr i64)]", sets("h", "i64", h512, s64, "i64") + sets("k", "i64", k512, s64, "i64")),
])

if len(sys.argv) < 2:
    print(text, end="")
    sys.exit()
path = sys.argv[1]
src = open(path).read()
pat = re.compile(r"(  ;; @tables \(tools/hash_tables.py\)\n).*?(  ;; @end\n)", re.S)
assert len(pat.findall(src)) == 1
open(path, "w").write(pat.sub(lambda m: m.group(1) + text + m.group(2), src))
