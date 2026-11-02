/* binarytrees: the same algorithm as binarytrees.aipl (Benchmarks Game binary-trees), in C,
   with malloc and free per node (the classic C version; AIPL uses an arena). */
#include <stdio.h>
#include <stdlib.h>

typedef struct Node { struct Node *left, *right; } Node;
static Node *bottom_up(int depth) {
    Node *n = malloc(sizeof(Node));
    if (depth > 0) { n->left = bottom_up(depth - 1); n->right = bottom_up(depth - 1); }
    else n->left = n->right = NULL;
    return n;
}
static int check(Node *n) { return n->left == NULL ? 1 : 1 + check(n->left) + check(n->right); }
static void release(Node *n) { if (n->left) { release(n->left); release(n->right); } free(n); }
int main(int argc, char **argv) {
    int n = argc > 1 ? atoi(argv[1]) : 10, min_depth = 4;
    int max_depth = min_depth + 2 > n ? min_depth + 2 : n;
    Node *stretch = bottom_up(max_depth + 1);
    printf("stretch tree of depth %d\t check: %d\n", max_depth + 1, check(stretch));
    release(stretch);
    Node *long_lived = bottom_up(max_depth);
    for (int d = min_depth; d <= max_depth; d += 2) {
        int iterations = 1 << (max_depth - d + min_depth), checked = 0;
        for (int i = 1; i <= iterations; i++) { Node *t = bottom_up(d); checked += check(t); release(t); }
        printf("%d\t trees of depth %d\t check: %d\n", iterations, d, checked);
    }
    printf("long lived tree of depth %d\t check: %d\n", max_depth, check(long_lived));
    return 0;
}
