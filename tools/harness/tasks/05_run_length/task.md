# Run-length encoding

Each line of the input is `E` or `D`, a space, and a text. Print one line
for each:

- `E text`: the text encoded, each run of one repeated character written
  as its length then the character: `aaabcc` is `3a1b2c`.
- `D code`: the code decoded: each count (one or more digits) is followed
  by the character it repeats.

Texts contain only letters, so no character in them is a digit. A text may
be empty.

Example input:

    E aaabcc
    D 3a1b2c
    E x
    D 12z

Example output:

    3a1b2c
    aaabcc
    1x
    zzzzzzzzzzzz

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
