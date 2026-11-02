# Evaluating expressions

Each line of the input is an arithmetic expression: non-negative integer
literals, `+`, `-`, `*`, `/`, parentheses, and spaces anywhere between
them. `*` and `/` bind tighter than `+` and `-`; operators of the same
kind group from the left (`8 - 3 - 2` is 3); a `-` before a number or a
parenthesis is a negation (`-(2 + 3)`, `4 * -2`). `/` divides and rounds
toward zero.

Print, for each line, the value, or `error` if the line is not an
expression of that form (including an empty line) or a division is by
zero. Every value, final and intermediate, fits in a signed 64-bit
integer.

Example input:

    1 + 2 * 3
    (1 + 2) * 3
    -7 / 2
    8 - 3 - 2
    2 * (3
    1 / 0

Example output:

    7
    9
    -3
    3
    error
    error

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
