# Word frequencies

The input is any text. A word is a maximal run of ASCII letters (`a`-`z`,
`A`-`Z`); everything else separates words. Words are compared ignoring
case, and printed in lowercase.

Print the 10 most frequent words (all of them, if there are fewer than 10
distinct words), one per line, as the word, a space, and its count. Order
them by count, highest first, and words with the same count alphabetically.
If the input has no words, print nothing.

Example input:

    The cat and the hat.
    THE END, and the bat!

Example output:

    the 4
    and 2
    bat 1
    cat 1
    end 1
    hat 1

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
