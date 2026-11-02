# Base64

Each line of the input is a text of printable ASCII characters (it may be
empty). Print, for each line, the standard Base64 encoding of its bytes
(not counting the line's newline), with `=` padding, on one line.

Example input:

    Man
    Ma
    hello, world

Example output:

    TWFu
    TWE=
    aGVsbG8sIHdvcmxk

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
