"""pidigits: the same algorithm as pidigits.aipl (Gibbons' spigot), on Python's built-in big integers."""
import sys

def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 1000
    numer, accum, denom, k = 1, 0, 1, 0
    out, count = [], 0
    while count < n:
        k += 1
        k2 = 2 * k + 1
        accum = (accum + 2 * numer) * k2
        denom *= k2
        numer *= k
        if numer > accum:
            continue
        d = (numer * 3 + accum) // denom
        if d != (numer * 4 + accum) // denom:
            continue
        out.append(chr(48 + d))
        count += 1
        if count % 10 == 0:
            out.append("\t:%d\n" % count)
        accum = (accum - denom * d) * 10
        numer *= 10
    if count % 10:
        out.append(" " * (10 - count % 10) + "\t:%d\n" % count)
    sys.stdout.write("".join(out))

main()
