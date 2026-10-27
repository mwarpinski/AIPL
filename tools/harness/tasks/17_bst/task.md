# A binary search tree

The input is integers separated by whitespace. Insert them, in order, into
an empty binary search tree: smaller values go left, larger go right, and
a value already in the tree is ignored. (The input may be sorted, making
the tree as deep as the input is long.)

Print three lines: the values in order (an in-order walk), separated by
single spaces; `height H`, the number of nodes on the longest path from
the root down (0 for an empty tree); and the values in pre-order (each
node before its left, then right, subtree). For an empty input the first
and third lines are empty.

Example input:

    5 3 8 1 4 5 9

Example output:

    1 3 4 5 8 9
    height 3
    5 3 1 4 8 9

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
