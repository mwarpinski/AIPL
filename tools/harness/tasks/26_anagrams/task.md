# Groups of anagrams

Each line of the input is a word of lowercase letters. Two words are
anagrams if one's letters can be rearranged to make the other. A word
that appears more than once counts once.

Print every group of two or more different words that are all anagrams of
each other, one group per line: the group's words in alphabetical order,
separated by single spaces. Order the groups by their first word.

Example input:

    listen
    enlist
    google
    silent
    banana
    tinsel
    listen

Example output:

    enlist listen silent tinsel

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
