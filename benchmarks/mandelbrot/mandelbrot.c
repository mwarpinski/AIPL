/* mandelbrot: the same algorithm as mandelbrot.aipl (Benchmarks Game mandelbrot), in C. */
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    int n = argc > 1 ? atoi(argv[1]) : 200;
    printf("P4\n%d %d\n", n, n);
    double w = (double)n;
    int bits = 0, count = 0;
    for (int y = 0; y < n; y++) {
        double ci = (2.0 * (double)y) / w - 1.0;
        for (int x = 0; x < n; x++) {
            double cr = (2.0 * (double)x) / w - 1.5;
            double zr = 0.0, zi = 0.0, tr = 0.0, ti = 0.0;
            int i = 0;
            while (i < 50 && tr + ti <= 4.0) {
                zi = (2.0 * zr) * zi + ci; zr = (tr - ti) + cr; tr = zr * zr; ti = zi * zi; i++;
            }
            bits = (bits << 1) | (tr + ti <= 4.0 ? 1 : 0);
            count++;
            if (count == 8) { putchar(bits); bits = 0; count = 0; }
            else if (x == n - 1) { putchar(bits << (8 - n % 8)); bits = 0; count = 0; }
        }
    }
    return 0;
}
