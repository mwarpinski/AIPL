/* knucleotide: the same task as knucleotide.aipl (Benchmarks Game k-nucleotide), in C:
   k-mers (k <= 18) packed two bits per letter into a hash table. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { unsigned long long key; int count; int used; } Slot;
typedef struct { Slot *s; size_t cap, n; } Table;
static unsigned code(char c) { return c == 'A' ? 0 : c == 'C' ? 1 : c == 'G' ? 2 : 3; }
static size_t h(unsigned long long k, size_t cap) { k ^= k >> 29; k *= 0xbf58476d1ce4e5b9ULL; k ^= k >> 32; return k & (cap - 1); }
static void grow(Table *t);
static void add(Table *t, unsigned long long k) {
    if (t->n * 2 >= t->cap) grow(t);
    size_t i = h(k, t->cap);
    while (t->s[i].used && t->s[i].key != k) i = (i + 1) & (t->cap - 1);
    if (!t->s[i].used) { t->s[i].used = 1; t->s[i].key = k; t->s[i].count = 0; t->n++; }
    t->s[i].count++;
}
static void grow(Table *t) {
    Slot *old = t->s; size_t oc = t->cap;
    t->cap = oc ? oc * 2 : 64; t->s = calloc(t->cap, sizeof(Slot)); t->n = 0;
    for (size_t i = 0; i < oc; i++) if (old[i].used) { size_t j = h(old[i].key, t->cap); while (t->s[j].used) j = (j + 1) & (t->cap - 1); t->s[j] = old[i]; t->n++; }
    free(old);
}
static int lookup(Table *t, unsigned long long k) {
    size_t i = h(k, t->cap);
    while (t->s[i].used) { if (t->s[i].key == k) return t->s[i].count; i = (i + 1) & (t->cap - 1); }
    return 0;
}
static Table count_kmers(const char *seq, size_t len, int k) {
    Table t = {0, 0, 0}; grow(&t);
    unsigned long long mask = k == 32 ? ~0ULL : (1ULL << (2 * k)) - 1, key = 0;
    for (size_t i = 0; i < len; i++) { key = ((key << 2) | code(seq[i])) & mask; if (i + 1 >= (size_t)k) add(&t, key); }
    return t;
}
static void decode(unsigned long long key, int k, char *out) { for (int i = k - 1; i >= 0; i--) { out[i] = "ACGT"[key & 3]; key >>= 2; } out[k] = 0; }
typedef struct { char key[20]; int count; } Entry;
static int by_count(const void *a, const void *b) {
    const Entry *x = a, *y = b;
    return x->count != y->count ? y->count - x->count : strcmp(x->key, y->key);
}
static void frequencies(const char *seq, size_t len, int k) {
    Table t = count_kmers(seq, len, k);
    Entry *e = malloc(t.n * sizeof(Entry)); size_t n = 0;
    for (size_t i = 0; i < t.cap; i++) if (t.s[i].used) { decode(t.s[i].key, k, e[n].key); e[n].count = t.s[i].count; n++; }
    qsort(e, n, sizeof(Entry), by_count);
    double total = (double)(len - k + 1);
    for (size_t i = 0; i < n; i++) printf("%s %.3f\n", e[i].key, (100.0 * (double)e[i].count) / total);
    printf("\n");
}
static void occurrences(const char *seq, size_t len, const char *frag) {
    int k = strlen(frag); Table t = count_kmers(seq, len, k);
    unsigned long long key = 0; for (int i = 0; i < k; i++) key = (key << 2) | code(frag[i]);
    printf("%d\t%s\n", lookup(&t, key), frag);
}
int main(void) {
    size_t cap = 1 << 20, n = 0; char *in = malloc(cap);
    for (size_t r; (r = fread(in + n, 1, cap - n, stdin)) > 0;) { n += r; if (n == cap) in = realloc(in, cap *= 2); }
    char *seq = malloc(n + 1); size_t len = 0;
    char *p = in, *end = in + n;
    while (p < end && !(end - p >= 6 && memcmp(p, ">THREE", 6) == 0)) { while (p < end && *p != '\n') p++; p++; }
    while (p < end && *p != '\n') p++;
    for (; p < end && *p != '>'; p++) if (*p != '\n') seq[len++] = (*p >= 'a' && *p <= 'z') ? *p - 32 : *p;
    frequencies(seq, len, 1); frequencies(seq, len, 2);
    const char *frags[] = {"GGT", "GGTA", "GGTATT", "GGTATTTTAATT", "GGTATTTTAATTTATAGT"};
    for (int i = 0; i < 5; i++) occurrences(seq, len, frags[i]);
    return 0;
}
