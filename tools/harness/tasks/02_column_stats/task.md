# Column statistics

The first line of the input is the name of a column. The rest is a table
in CSV form: a header line naming the columns, then rows. Fields are
separated by commas; no field contains a comma or a quote. Every value in
every column is an integer (it may be negative).

Print five lines about the named column: `count N`, `sum S`, `mean M` with
M rounded to 2 decimals, `min X`, and `max Y`. If no column has that name,
print `no column NAME` instead. If the table has no rows, print `count 0`
only.

Example input:

    price
    item,price,qty
    apple,3,10
    pear,4,2
    fig,-1,5

Example output:

    count 3
    sum 6
    mean 2.00
    min -1
    max 4

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
