# Wrapping text

The first line of the input is a width W >= 10. The rest is text: words
separated by spaces and newlines, in paragraphs separated by one or more
blank lines (a line that is empty or only spaces).

Print each paragraph wrapped to width W: put as many words on a line as
fit with single spaces between them in at most W characters, then start
the next line. A word longer than W goes on a line of its own. Separate
paragraphs with one empty line. Print nothing for a text with no words.

Example input:

    20
    The quick brown fox jumps over
    the lazy dog.

    A second paragraph.

Example output:

    The quick brown fox
    jumps over the lazy
    dog.

    A second paragraph.

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
