# A least-recently-used cache

The first line of the input is the cache's capacity C >= 1. Each following
line is `get K` or `put K V` (K and V are integers). The cache holds at
most C keys. `put` stores V under K; `get` looks K up. Both make K the most
recently used. When a `put` of a new key would make more than C keys, the
least recently used key is removed first.

For each `get`, print the value stored under K, or `-1` if K is not in the
cache. After the last line, print `hits H misses M`: how many gets found
their key, and how many did not.

Example input:

    2
    put 1 10
    put 2 20
    get 1
    put 3 30
    get 2
    get 3

Example output:

    10
    -1
    30
    hits 2 misses 1

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
