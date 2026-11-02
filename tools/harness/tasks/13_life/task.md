# Game of Life

The first line of the input is a number of generations G >= 0. The rest is
a grid of `.` (dead) and `#` (alive) cells, all lines the same width. Run
Conway's Game of Life for G generations: a live cell with 2 or 3 live
neighbours (of its 8) stays alive, a dead cell with exactly 3 becomes
alive, and every other cell is dead in the next generation. Cells outside
the grid are always dead.

Print the grid after G generations, in the same form.

Example input:

    1
    .....
    ..#..
    ..#..
    ..#..
    .....

Example output:

    .....
    .....
    .###.
    .....
    .....

Read all of standard input and write the answer to standard output. The
output must match exactly: every line ends with a newline, with no other
trailing spaces or blank lines.
