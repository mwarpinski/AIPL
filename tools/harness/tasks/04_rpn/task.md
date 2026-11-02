# Reverse Polish calculator

Each line of the input is an expression in reverse Polish notation: tokens
separated by single spaces, each an integer (possibly negative, as `-3`)
or one of the operators `+`, `-`, `*`, `/`. An operator takes the two
numbers on top of the stack (the deeper one is its left operand) and
pushes its result. `/` divides and rounds toward zero.

For each line print the single number left on the stack, or `error` if the
line is empty, an operator lacks operands, a division is by zero, or more
or fewer than one number is left. Every number, final and intermediate,
fits in a signed 64-bit integer.

Example input:

    3 4 + 2 *
    7 -2 /
    1 +
    4 0 /

Example output:

    14
    -3
    error
    error

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
