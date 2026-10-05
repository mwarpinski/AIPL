"""spigot: the same algorithm as spigot.aipl (Rabinowitz-Wagon), in Python."""
import sys

def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 1000
    size = n * 10 // 3 + 1
    a = [2] * size
    out, count = [], 0
    def emit(d):
        nonlocal count
        out.append(chr(48 + d)); count += 1
        if count % 10 == 0:
            out.append(f"\t:{count}\n")
    nines, predigit, first = 0, 0, True
    for _ in range(n):
        q = 0
        for i in range(size, 0, -1):
            x = 10 * a[i - 1] + q * i
            d = 2 * i - 1
            a[i - 1] = x % d
            q = x // d
        a[0] = q % 10; q //= 10
        if q == 9: nines += 1
        elif q == 10:
            emit(predigit + 1)
            for _ in range(nines): emit(0)
            predigit, nines = 0, 0
        else:
            if first: first = False
            else: emit(predigit)
            predigit = q
            for _ in range(nines): emit(9)
            nines = 0
    emit(predigit)
    if count % 10:
        out.append(" " * (10 - count % 10) + f"\t:{count}\n")
    sys.stdout.write("".join(out))

main()
