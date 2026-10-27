# Balanced brackets

Each line of the input is a string. Check its brackets: `()`, `[]`, and
`{}`; every other character is ignored. For each line print one line:

- `ok` if every bracket is closed by the matching one, in order;
- `mismatch at K` if a closing bracket does not match the last open one,
  or nothing is open, K being its position in the line, counting from 1;
- `unclosed` if the line ends with brackets still open.

Example input:

    (a[b]{c})
    ([)]
    ((x)
    }

Example output:

    ok
    mismatch at 3
    unclosed
    mismatch at 1

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
