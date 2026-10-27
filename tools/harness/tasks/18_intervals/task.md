# Merging intervals

Each line of the input is `A B`, two integers with A <= B: the interval
from A to B. Merge every set of intervals that overlap or touch (one
starts at or before another ends: `1 3` and `3 5` merge to `1 5`, but
`1 3` and `4 5` do not).

Print the merged intervals, one per line as `A B`, ordered by start.

Example input:

    5 8
    1 3
    2 4
    10 10
    8 9

Example output:

    1 4
    5 9
    10 10

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
