# A hex dump

Print the bytes of the input, 16 to a line. Each line is:

- the offset of its first byte, as 8 lowercase hexadecimal digits;
- two spaces;
- each byte as 2 lowercase hexadecimal digits, separated by single spaces,
  the whole padded on the right with spaces to 47 characters;
- two spaces, `|`, each byte as its character if it is 32 to 126 or `.`
  otherwise, and `|`.

After the last line, print the total number of bytes as 8 hexadecimal
digits, alone on its line (`00000000` for an empty input).

Example input (the line `Hello, world!` and its newline):

    Hello, world!

Example output:

    00000000  48 65 6c 6c 6f 2c 20 77 6f 72 6c 64 21 0a        |Hello, world!.|
    0000000e

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
