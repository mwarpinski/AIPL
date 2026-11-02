# A priority queue of tasks

Each line of the input is `add NAME PRIORITY` (PRIORITY an integer) or
`next`. `add` puts a task in the queue. `next` takes out the task with the
highest priority (of equal priorities, the one added first) and prints its
name, or prints `idle` if the queue is empty.

After the last line, print `left N`: the number of tasks still queued.

Example input:

    add write 2
    add test 5
    add ship 5
    next
    next
    next
    next
    add rest 1

Example output:

    test
    ship
    write
    idle
    left 1

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
