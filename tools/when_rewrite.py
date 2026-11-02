#!/usr/bin/env python3
"""Rewrites every `if` with an empty `(block)` branch as `when`
(CANONICAL_PLAN.md step 4), keeping every other byte of the file:

    (if C (block B...) (block))   ->  (when C B...)        the same tree
    (if C X (block))              ->  (when C X)           X one form
    (if C (block) (block B...))   ->  (when (not C) B...)
    (if C (block) X)              ->  (when (not C) X)
    (if (not C) (block) X)        ->  (when C X)           no double not

    (if C (block) (block))        ->  (when C)             C for its effect alone
Innermost forms first, in passes, since these ifs nest. In a Markdown
file only the code blocks (fenced by lines starting with ```) are read;
in a Rust file a quote is one token (strings inside Rust strings are not
told apart), as tools/retire_loop.py does.

    python3 tools/when_rewrite.py [--write] FILE...
"""
import re
import sys


def tokens(text, rust):
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


def candidates(text, rust):
    """(start, end, replacement or None, line) for each one-branch if."""
    toks = list(tokens(text, rust))
    t = lambda a, b: text[toks[a][1]:toks[b - 1][2]]
    found = []
    for k, (kd, a, b) in enumerate(toks):
        if kd != '(' or k + 1 >= len(toks) or text[toks[k + 1][1]:toks[k + 1][2]] != 'if':
            continue
        parts, j = [], k + 2
        for _ in range(3):
            if j >= len(toks) or toks[j][0] in ')]':
                break
            e = form_end(toks, j)
            parts.append((j, e))
            j = e
        if len(parts) != 3 or j >= len(toks) or toks[j][0] != ')':
            continue
        (c0, c1), (t0, t1), (e0, e1) = parts
        empty = lambda x0, x1: x1 - x0 == 3 and toks[x0][0] == '(' and text[toks[x0 + 1][1]:toks[x0 + 1][2]] == 'block'
        is_block = lambda x0, x1: toks[x0][0] == '(' and x0 + 1 < x1 and text[toks[x0 + 1][1]:toks[x0 + 1][2]] == 'block'
        start, end = toks[k][1], toks[j][2]
        line = text.count('\n', 0, start) + 1
        head = text[start:toks[c1 - 1][2]].replace('(if', '(when', 1)   # "(when C" with C's own spacing
        cond = t(c0, c1)

        def inner(x0, x1):
            # a (block ...)'s items, as written, with the space before the first
            return text[toks[x0 + 1][2]:toks[x1 - 1][1]].rstrip()

        if empty(t0, t1) and empty(e0, e1):
            found.append((start, end, head + ')', line))
        elif empty(e0, e1):
            body = inner(t0, t1) if is_block(t0, t1) else text[toks[c1 - 1][2]:toks[t1 - 1][2]]
            found.append((start, end, head + body + ')', line))
        elif empty(t0, t1):
            m = re.fullmatch(r'\(not\s+(.*)\)', cond, re.S)
            neg = m.group(1).strip() if m else f'(not {cond})'
            body = inner(e0, e1) if is_block(e0, e1) else ' ' + t(e0, e1)
            found.append((start, end, f'(when {neg}{body})', line))
    return found


def rewrite(path, text):
    rust = path.endswith('.rs')
    done, listed = 0, []
    while True:
        cands = candidates(text, rust)
        edits = [c for c in cands if c[2] is not None]
        listed = [c[3] for c in cands if c[2] is None]
        if not edits:
            return text, done, listed
        # innermost first: an edit holding another waits for the next pass
        inner = [c for c in edits if not any(d is not c and c[0] <= d[0] and d[1] <= c[1] for d in edits)]
        out, last = [], 0
        for s0, s1, new, _ in sorted(inner):
            out.append(text[last:s0])
            out.append(new)
            last = s1
        out.append(text[last:])
        text = ''.join(out)
        done += len(inner)


def rewrite_markdown(path, text):
    lines = text.split('\n')
    out, block, in_code, done, listed = [], [], False, 0, []
    for line in lines:
        if line.startswith('```'):
            if in_code:
                new, n, li = rewrite(path, '\n'.join(block))
                out.extend(new.split('\n'))
                done += n
                listed += li
                block = []
            out.append(line)
            in_code = not in_code
        elif in_code:
            block.append(line)
        else:
            out.append(line)
    out.extend(block)
    return '\n'.join(out), done, listed


def main():
    args = sys.argv[1:]
    write = '--write' in args
    total = 0
    for path in [a for a in args if a != '--write']:
        text = open(path).read()
        new, n, listed = (rewrite_markdown if path.endswith('.md') else rewrite)(path, text)
        total += n
        if n or listed:
            print(f"{path}: {n} rewritten" + (f"; left, both branches empty, at lines {listed}" if listed else ""))
        if write and new != text:
            open(path, 'w').write(new)
    print(f"{total} {'rewritten' if write else 'would be rewritten'}")


main()
