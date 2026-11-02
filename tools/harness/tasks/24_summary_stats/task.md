# Summary statistics

The input is integers, one per line; there is at least one. Print five
lines:

- `mean M`, the mean, rounded to 2 decimals;
- `median X`, the middle value of the sorted values, or the mean of the
  two middle ones if there is an even number, with 1 decimal;
- `mode X`, the most frequent value (the smallest of those, on a tie);
- `stddev S`, the population standard deviation (the square root of the
  mean of the squared differences from the mean), rounded to 3 decimals;
- `range R`, the largest value minus the smallest.

Example input:

    2
    4
    4
    5

Example output:

    mean 3.75
    median 4.0
    mode 4
    stddev 1.090
    range 3

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
