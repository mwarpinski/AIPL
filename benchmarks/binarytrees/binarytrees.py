"""binarytrees: the same algorithm as binarytrees.aipl (binary-trees), in Python (garbage collected)."""
import sys

def bottom_up(depth):
    return (bottom_up(depth - 1), bottom_up(depth - 1)) if depth > 0 else (None, None)

def check(n):
    return 1 if n[0] is None else 1 + check(n[0]) + check(n[1])

def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 10
    min_depth = 4
    max_depth = max(min_depth + 2, n)
    out = [f"stretch tree of depth {max_depth + 1}\t check: {check(bottom_up(max_depth + 1))}"]
    long_lived = bottom_up(max_depth)
    for d in range(min_depth, max_depth + 1, 2):
        iterations = 1 << (max_depth - d + min_depth)
        checked = sum(check(bottom_up(d)) for _ in range(iterations))
        out.append(f"{iterations}\t trees of depth {d}\t check: {checked}")
    out.append(f"long lived tree of depth {max_depth}\t check: {check(long_lived)}")
    print("\n".join(out))

main()
