# An order that respects every rule

The first line of the input is `N M`: N tasks numbered 0 to N-1 and M
rules. Each of the next M lines is `a b`: task a must come before task b.
N <= 5000 and M <= 50000.

Print every task, separated by single spaces, on one line, in an order
that keeps every rule. Of all such orders, print the one that is smallest
when compared number by number: at each step take the smallest task whose
earlier tasks have all been taken. If no order keeps every rule, print
`cycle`.

Example input:

    4 3
    2 0
    1 0
    3 1

Example output:

    2 3 1 0

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
