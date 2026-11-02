# Shortest paths

The first line of the input is `N M`: N nodes, numbered 0 to N-1, and M
edges. Each of the next M lines is `u v w`: an edge from u to v of length
w, 0 <= w <= 1000 (edges are one-way; there may be several between the
same nodes). N <= 2000 and M <= 20000.

Print N lines: the length of the shortest path from node 0 to each node in
order, or `inf` if there is none (node 0's is 0).

Example input:

    4 4
    0 1 5
    0 2 1
    2 1 2
    1 3 1

Example output:

    0
    3
    1
    4

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
