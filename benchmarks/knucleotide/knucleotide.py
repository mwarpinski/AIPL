"""knucleotide: the same task as knucleotide.aipl (k-nucleotide), in Python."""
import sys

def main():
    data = sys.stdin.buffer.read().decode()
    lines = data.split("\n")
    i = next(n for n, l in enumerate(lines) if l.startswith(">THREE"))
    seq = []
    for l in lines[i + 1:]:
        if l.startswith(">"): break
        seq.append(l)
    seq = "".join(seq).upper()
    out = []
    for k in (1, 2):
        counts = {}
        for j in range(len(seq) - k + 1):
            s = seq[j:j + k]; counts[s] = counts.get(s, 0) + 1
        total = float(len(seq) - k + 1)
        for s, c in sorted(counts.items(), key=lambda kv: (-kv[1], kv[0])):
            out.append(f"{s} {(100.0 * float(c)) / total:.3f}")
        out.append("")
    for frag in ("GGT", "GGTA", "GGTATT", "GGTATTTTAATT", "GGTATTTTAATTTATAGT"):
        k = len(frag)
        n = sum(1 for j in range(len(seq) - k + 1) if seq[j:j + k] == frag)
        out.append(f"{n}\t{frag}")
    sys.stdout.write("\n".join(out) + "\n")

main()
