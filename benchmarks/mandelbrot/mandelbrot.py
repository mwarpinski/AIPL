"""mandelbrot: the same algorithm as mandelbrot.aipl (mandelbrot), in Python."""
import sys

def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 200
    out = bytearray(f"P4\n{n} {n}\n".encode())
    w = float(n)
    bits = count = 0
    for y in range(n):
        ci = (2.0 * float(y)) / w - 1.0
        for x in range(n):
            cr = (2.0 * float(x)) / w - 1.5
            zr = zi = tr = ti = 0.0
            i = 0
            while i < 50 and tr + ti <= 4.0:
                zi = (2.0 * zr) * zi + ci; zr = (tr - ti) + cr; tr = zr * zr; ti = zi * zi; i += 1
            bits = (bits << 1) | (1 if tr + ti <= 4.0 else 0)
            count += 1
            if count == 8:
                out.append(bits & 255); bits = count = 0
            elif x == n - 1:
                out.append((bits << (8 - n % 8)) & 255); bits = count = 0
    sys.stdout.buffer.write(out)

main()
