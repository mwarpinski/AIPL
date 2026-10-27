# Longest Collatz chain

The Collatz chain of a positive integer x repeats: if x is even, x becomes
x / 2, otherwise 3x + 1, until x is 1. Its length counts every number in
it, the first and the 1 included: the chain of 6 is 6 3 10 5 16 8 4 2 1,
of length 9.

The input is one integer n, 1 <= n <= 1000000. Print the starting number
from 1 to n whose chain is longest, a space, and that length. If several
starts tie, print the smallest. (Numbers in a chain can exceed 2^31.)

Example input:

    10

Example output:

    9 20

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
