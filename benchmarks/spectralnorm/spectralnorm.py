"""spectralnorm: the same algorithm as spectralnorm.aipl (spectral-norm), in Python."""
import math, sys

def a(i, j):
    return 1.0 / float((i + j) * (i + j + 1) // 2 + i + 1)

def times(n, v, out):
    for i in range(n):
        s = 0.0
        for j in range(n):
            s = s + a(i, j) * v[j]
        out[i] = s

def times_transposed(n, v, out):
    for i in range(n):
        s = 0.0
        for j in range(n):
            s = s + a(j, i) * v[j]
        out[i] = s

def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 100
    u = [1.0] * n; v = [0.0] * n; tmp = [0.0] * n
    for _ in range(10):
        times(n, u, tmp); times_transposed(n, tmp, v)
        times(n, v, tmp); times_transposed(n, tmp, u)
    vbv = vv = 0.0
    for i in range(n):
        vbv = vbv + u[i] * v[i]
        vv = vv + v[i] * v[i]
    print(f"{math.sqrt(vbv / vv):.9f}")

main()
