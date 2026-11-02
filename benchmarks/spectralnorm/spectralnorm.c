/* spectralnorm: the same algorithm as spectralnorm.aipl (Benchmarks Game spectral-norm), in C. */
#include <math.h>
#include <stdio.h>
#include <stdlib.h>

static double a(int i, int j) { return 1.0 / (double)((i + j) * (i + j + 1) / 2 + i + 1); }
static void times(int n, const double *v, double *out) {
    for (int i = 0; i < n; i++) { double sum = 0.0; for (int j = 0; j < n; j++) sum = sum + a(i, j) * v[j]; out[i] = sum; }
}
static void times_transposed(int n, const double *v, double *out) {
    for (int i = 0; i < n; i++) { double sum = 0.0; for (int j = 0; j < n; j++) sum = sum + a(j, i) * v[j]; out[i] = sum; }
}
static void times_ata(int n, const double *v, double *out, double *tmp) { times(n, v, tmp); times_transposed(n, tmp, out); }
int main(int argc, char **argv) {
    int n = argc > 1 ? atoi(argv[1]) : 100;
    double *u = malloc(n * sizeof(double)), *v = malloc(n * sizeof(double)), *tmp = malloc(n * sizeof(double));
    for (int i = 0; i < n; i++) u[i] = 1.0;
    for (int r = 0; r < 10; r++) { times_ata(n, u, v, tmp); times_ata(n, v, u, tmp); }
    double vbv = 0.0, vv = 0.0;
    for (int i = 0; i < n; i++) { vbv = vbv + u[i] * v[i]; vv = vv + v[i] * v[i]; }
    printf("%.9f\n", sqrt(vbv / vv));
    return 0;
}
