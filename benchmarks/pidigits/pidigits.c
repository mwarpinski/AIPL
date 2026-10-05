/* pidigits: the same algorithm as pidigits.aipl (Gibbons' spigot), in C on
   GMP, as the Benchmarks Game's C entries do. GMP is decades of hand-tuned
   assembly, so this measures AIPL's std/bigint against the best available,
   not against C the language. Build with -lgmp. */
#include <stdio.h>
#include <stdlib.h>
#include <gmp.h>

static mpz_t numer, accum, denom, t1, t2;

static unsigned extract_digit(unsigned nth) {
    mpz_mul_ui(t1, numer, nth);
    mpz_add(t2, t1, accum);
    mpz_tdiv_q(t1, t2, denom);
    return mpz_get_ui(t1);
}

int main(int argc, char **argv) {
    int n = argc > 1 ? atoi(argv[1]) : 1000;
    mpz_init_set_ui(numer, 1); mpz_init_set_ui(accum, 0); mpz_init_set_ui(denom, 1);
    mpz_init(t1); mpz_init(t2);
    int count = 0;
    for (unsigned k = 1; count < n; k++) {
        unsigned k2 = 2 * k + 1;
        mpz_addmul_ui(accum, numer, 2);
        mpz_mul_ui(accum, accum, k2);
        mpz_mul_ui(denom, denom, k2);
        mpz_mul_ui(numer, numer, k);
        if (mpz_cmp(numer, accum) > 0) continue;
        unsigned d = extract_digit(3);
        if (d != extract_digit(4)) continue;
        putchar('0' + d);
        if (++count % 10 == 0) printf("\t:%d\n", count);
        mpz_submul_ui(accum, denom, d);
        mpz_mul_ui(accum, accum, 10);
        mpz_mul_ui(numer, numer, 10);
    }
    if (count % 10) printf("%*s\t:%d\n", 10 - count % 10, "", count);
    return 0;
}
