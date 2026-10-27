# Reading a settings file

The input is a settings file. Each line is one of:

- blank, or a comment: its first non-space character is `;` or `#`;
- a section header, `[name]`: the keys after it belong to that section;
- a setting, `key = value`: spaces around the key and the value are
  dropped; the value is everything after the first `=`.

Keys before the first header are in the section with the empty name. A key
set twice in one section keeps its last value.

Print every setting as `section.key=value`, ordered by section name, then
by key (so the empty section's come first, as `.key=value`).

Example input:

    name = demo
    [server]
    port = 8080
    host=localhost
    ; a comment
    [client]
    retries = 3
    [server]
    port = 9090

Example output:

    .name=demo
    client.retries=3
    server.host=localhost
    server.port=9090

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
