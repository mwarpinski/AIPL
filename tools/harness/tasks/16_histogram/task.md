# A histogram

The first line of the input is `LO HI B`: the range LO to HI is cut into B
buckets of equal width W = (HI - LO) / B, which is a whole number. Bucket i
(from 0) holds the values v with LO + i*W <= v < LO + (i+1)*W. Each of the
following lines is one integer value.

Print one line per bucket, in order: `[START,END) COUNT`, then, if COUNT is
not 0, a space and COUNT `#` characters. Then print `below N` and
`above N`: how many values were less than LO, and how many were HI or
more.

Example input:

    0 10 2
    3
    7
    4
    10
    -1

Example output:

    [0,5) 2 ##
    [5,10) 1 #
    below 1
    above 1

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
