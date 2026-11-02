# Days between dates

Each line of the input is two dates, `YYYY-MM-DD YYYY-MM-DD`, in the
Gregorian calendar, with years from 1600 to 2400. Print, for each line, the
number of days from the first date to the second: negative if the second
is earlier, 0 if they are the same.

Example input:

    2024-02-28 2024-03-01
    2023-02-28 2023-03-01
    2000-01-01 1999-12-31

Example output:

    2
    1
    -1

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
