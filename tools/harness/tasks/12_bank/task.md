# A small bank

Each line of the input is one operation:

- `open NAME`: opens an account with balance 0.
- `deposit NAME AMOUNT`, `withdraw NAME AMOUNT`, `transfer FROM TO AMOUNT`.

Names are words of letters; amounts are positive integers, in cents. An
operation that cannot be done has no effect, and prints `error L: REASON`,
L being its line number (from 1) and REASON one of: `already open` (opening
an open account), `no such account` (any account named is not open; for a
transfer, check FROM first), `insufficient funds` (a withdrawal or
transfer of more than the balance).

After the last line, print every open account, ordered by name, as the
name, a space, and its balance in dollars with two decimals: 1205 cents is
`12.05`.

Example input:

    open alice
    deposit alice 1050
    open bob
    transfer alice bob 300
    withdraw bob 500
    open alice
    deposit carol 5

Example output:

    error 5: insufficient funds
    error 6: already open
    error 7: no such account
    alice 7.50
    bob 3.00

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
