#!/usr/bin/env python3
"""Rewrites `(loop VAR START END 1 body...)` as `(for VAR START END' body...)`
(CANONICAL_PLAN.md step 3). `loop` includes its end and `for` leaves it out,
so END' is END plus one, written the shortest way that means the same:

    (- X 1)  ->  X          (the common case: `for` is (loop i s (- e 1) 1))
    (- X k)  ->  (- X k-1)  and  (+ X k) -> (+ X k+1)   for an integer k
    7        ->  8
    E        ->  (+ E 1)

Only the head, the end, and the step are touched; every other byte of the
file stays (comments, line breaks, the body). A loop it cannot rewrite
mechanically (a step other than 1, a variable that is not a plain name, a
comment between the end and the step) is left alone and listed.

    python3 tools/retire_loop.py [--write] FILE...
"""
import re
import sys


def tokens(text, rust):
    """(kind, start, end) for ( ) [ ] atoms and strings; comments skipped.
    In a Rust file, \\n \\t and \\" are escapes of the Rust string around the
    AIPL, not AIPL, and a quote is one token (strings are not told apart)."""
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c == ';' and text.startswith(';;', i):
            while i < n and text[i] != '\n':
                i += 1
        elif rust and c == '\\' and i + 1 < n and text[i + 1] in 'nt"':
            i += 2
        elif c in '()[]':
            yield (c, i, i + 1)
            i += 1
        elif c == '"' and rust:
            # a Rust string's quote (or an AIPL string's, inside it): not told apart
            yield ('a', i, i + 1)
            i += 1
        elif c == '"':
            j = i + 1
            while j < n and text[j] != '"':
                if text[j] == '\\':
                    j += 1
                j += 1
            yield ('s', i, j + 1)
            i = j + 1
        elif c.isspace():
            i += 1
        else:
            j = i
            while j < n and not text[j].isspace() and text[j] not in '()[]"':
                if rust and text[j] == '\\' and j + 1 < n and text[j + 1] in 'nt"':
                    break
                j += 1
            yield ('a', i, max(j, i + 1))
            i = max(j, i + 1)


def form_end(toks, k):
    """Index just past the form that starts at token k."""
    if toks[k][0] in 'as':
        return k + 1
    depth, j = 0, k
    while j < len(toks):
        if toks[j][0] in '([':
            depth += 1
        elif toks[j][0] in ')]':
            depth -= 1
        j += 1
        if depth == 0:
            return j
    return j


def new_end(text, toks, k0, k1):
    """The text of the exclusive end for the inclusive end in tokens k0..k1."""
    t = lambda a, b: text[toks[a][1]:toks[b - 1][2]]
    whole = t(k0, k1)
    if re.fullmatch(r'-?\d+', whole):
        v = int(whole) + 1
        if -2147483648 <= v <= 2147483647:
            return str(v)
    if toks[k0][0] == '(':
        # (op A B): three forms between the parens
        a0 = k0 + 2
        a1 = form_end(toks, a0)
        b0 = a1
        b1 = form_end(toks, b0)
        if b1 == k1 - 1 and toks[k0 + 1][0] == 'a':
            op = text[toks[k0 + 1][1]:toks[k0 + 1][2]]
            rhs = t(b0, b1)
            if op in ('-', '+') and re.fullmatch(r'\d+', rhs):
                k = int(rhs)
                if op == '-' and k == 1:
                    return t(a0, a1)
                if op == '-' and k > 1:
                    return f"(- {t(a0, a1)} {k - 1})"
                if op == '+':
                    return f"(+ {t(a0, a1)} {k + 1})"
    return f"(+ {whole} 1)"


def rewrite(path, text):
    rust = path.endswith('.rs')
    toks = list(tokens(text, rust))
    edits, skipped, kinds = [], [], {'(- X 1)': 0, 'other': 0}
    for k, (kd, a, b) in enumerate(toks):
        if kd != '(' or k + 1 >= len(toks) or text[toks[k + 1][1]:toks[k + 1][2]] != 'loop':
            continue
        line = text.count('\n', 0, a) + 1
        j = k + 2
        parts = []
        for _ in range(4):
            if j >= len(toks) or toks[j][0] in ')]':
                break
            e = form_end(toks, j)
            parts.append((j, e))
            j = e
        var = text[toks[parts[0][0]][1]:toks[parts[0][1] - 1][2]] if parts else ''
        if len(parts) < 4 or not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*', var):
            skipped.append((line, 'not a loop form (a name, a variant, prose)'))
            continue
        step = text[toks[parts[3][0]][1]:toks[parts[3][1] - 1][2]]
        if step != '1':
            skipped.append((line, f'step {step}'))
            continue
        gap = text[toks[parts[2][1] - 1][2]:toks[parts[3][0]][1]]
        if gap.strip():
            skipped.append((line, 'something between the end and the step'))
            continue
        e0, e1 = parts[2]
        end_text = new_end(text, toks, e0, e1)
        kinds['(- X 1)' if not end_text.startswith('(+ ') or end_text == text[toks[e0][1]:toks[e1 - 1][2]] else 'other'] += 1
        s0 = toks[parts[3][0]][1]
        s1 = toks[parts[3][1] - 1][2]
        head = toks[k + 1]
        edits.append((head[1], head[2], 'for'))
        edits.append((toks[e0][1], toks[e1 - 1][2], end_text))
        edits.append((toks[parts[2][1] - 1][2], s1, ''))  # the gap and the step
    out, last = [], 0
    for a, b, s in sorted(edits):
        out.append(text[last:a])
        out.append(s)
        last = b
    out.append(text[last:])
    return ''.join(out), len(edits) // 3, skipped, kinds


def rewrite_markdown(path, text):
    """Only the ```lisp blocks: prose has quotes and parentheses of its own."""
    parts = re.split(r'(```[^\n]*\n.*?```)', text, flags=re.S)
    total, skipped = 0, []
    for k, part in enumerate(parts):
        if part.startswith('```lisp') or part.startswith('```\n'):
            head, body = part.split('\n', 1)
            out, n, sk, _ = rewrite(path, body)
            parts[k] = head + '\n' + out
            total += n
            skipped += sk
    return ''.join(parts), total, skipped, {}


def main():
    args = sys.argv[1:]
    write = '--write' in args
    total = 0
    for path in [a for a in args if a != '--write']:
        text = open(path).read()
        out, n, skipped, kinds = (rewrite_markdown if path.endswith('.md') else rewrite)(path, text)
        total += n
        if n or skipped:
            print(f"{path}: {n} rewritten" + (f", skipped {skipped}" if skipped else ""))
        if write and out != text:
            open(path, 'w').write(out)
    print(f"{total} loops {'rewritten' if write else 'would be rewritten'}")


main()
