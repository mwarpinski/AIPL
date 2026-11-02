"""fasta: the Benchmarks Game's input generator for k-nucleotide (a
deterministic pseudo-random DNA file in FASTA format). Not a benchmark here,
only k-nucleotide's input: python3 fasta.py N > input."""
import sys

ALU = ("GGCCGGGCGCGGTGGCTCACGCCTGTAATCCCAGCACTTTGGGAGGCCGAGGCGGGCGGATCACCTGAGGTCAGGAGTTCGAGACCAGCCTGGCCAACATGGTGAAACCCCGTCTCTACTAAAAATACAAAAATTAGCCGGGCGTGGTGGCGCGCGCCTGTAATCCCAGCTACTCGGGAGGCTGAGGCAGGAGAATCGCTTGAACCCGGGAGGCGGAGGTTGCAGTGAGCCGAGATCGCGCCACTGCACTCCAGCCTGGGCGACAGAGCGAGACTCCGTCTCAAAAA")
IUB = [("a", 0.27), ("c", 0.12), ("g", 0.12), ("t", 0.27), ("B", 0.02), ("D", 0.02), ("H", 0.02), ("K", 0.02),
       ("M", 0.02), ("N", 0.02), ("R", 0.02), ("S", 0.02), ("V", 0.02), ("W", 0.02), ("Y", 0.02)]
HOMO = [("a", 0.3029549426680), ("c", 0.1979883004921), ("g", 0.1975473066391), ("t", 0.3015094502008)]
last = 42

def rand():
    global last
    last = (last * 3877 + 29573) % 139968
    return last / 139968.0

def repeat(out, seq, n):
    s = seq * (n // len(seq) + 1)
    for i in range(0, n, 60):
        out.append(s[i:min(i + 60, n)])

def random_fasta(out, table, n):
    cum, acc = [], 0.0
    for c, p in table:
        acc += p
        cum.append((acc, c))
    for i in range(0, n, 60):
        line = []
        for _ in range(min(60, n - i)):
            r = rand()
            line.append(next(c for p, c in cum if r < p) if r < cum[-1][0] else cum[-1][1])
        out.append("".join(line))

def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 1000
    out = [">ONE Homo sapiens alu"]
    repeat(out, ALU, n * 2)
    out.append(">TWO IUB ambiguity codes")
    random_fasta(out, IUB, n * 3)
    out.append(">THREE Homo sapiens frequency")
    random_fasta(out, HOMO, n * 5)
    sys.stdout.write("\n".join(out) + "\n")

main()
