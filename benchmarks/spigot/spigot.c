/* spigot: the same algorithm as spigot.aipl (Rabinowitz-Wagon), in C. */
#include <stdio.h>
#include <stdlib.h>

static char *out; static int count, used;
static void emit(int d) {
    out[used++] = '0' + d; count++;
    if (count % 10 == 0) used += sprintf(out + used, "\t:%d\n", count);
}
int main(int argc, char **argv) {
    int n = argc > 1 ? atoi(argv[1]) : 1000;
    int len = n * 10 / 3 + 1;
    long long *a = malloc(sizeof(long long) * len);
    out = malloc(n * 2 + 64);
    for (int i = 0; i < len; i++) a[i] = 2;
    int nines = 0, predigit = 0, first = 1;
    for (int j = 1; j <= n; j++) {
        long long q = 0;
        for (int i = len; i >= 1; i--) {
            long long x = 10 * a[i - 1] + q * i, d = 2 * i - 1;
            a[i - 1] = x % d; q = x / d;
        }
        a[0] = q % 10; q /= 10;
        if (q == 9) nines++;
        else if (q == 10) { emit(predigit + 1); for (int m = 0; m < nines; m++) emit(0); predigit = 0; nines = 0; }
        else { if (first) first = 0; else emit(predigit); predigit = (int)q; for (int m = 0; m < nines; m++) emit(9); nines = 0; }
    }
    emit(predigit);
    if (count % 10) { for (int k = count % 10; k < 10; k++) out[used++] = ' '; used += sprintf(out + used, "\t:%d\n", count); }
    fwrite(out, 1, used, stdout);
    return 0;
}
