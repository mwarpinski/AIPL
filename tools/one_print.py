#!/usr/bin/env python3
"""Rewrites std/io's labelled printing helpers as the format forms
(docs/design/LANGUAGE_DECISIONS.md item 6):

    (call io.println_int "lines: " n)       ->  (println "lines: {n}")
    (call io.println_i64 "t " (call f))     ->  (println "t {(call f)}")
    (call io.println_f64 "e " x 9)          ->  (println "e {x:.9}")
    (call io.print_int n)                   ->  (print "{n}")
    (call io.print_f64 x 9)                 ->  (print "{x:.9}")

A label that is not a string literal is a placeholder of its own; a
literal's braces are doubled. A value a placeholder cannot hold (one with a
string literal, a colon, a line break, or a form expanded during import
resolution, AIPL_SPEC.md 4.M) is bound first, in a block of its own:
(block (let _shown:i32 v) (println "lines: {_shown}")). In a Rust file, an
AIPL string is written \\"...\\" but in an r#"..."# string, and in a string a
format macro reads (format!, println!, ...) a brace of the AIPL is doubled
and the label's braces are left to the macro. A call whose digits are not a number is
left alone and listed. Every other byte of the file stays.

    python3 tools/one_print.py [--write] FILE...
"""
import re
import sys

HELPERS = {
    # name: (has a label, the value's type, has digits, newline)
    'println_int': (True, 'i32', False, True),
    'println_i64': (True, 'i64', False, True),
    'println_f64': (True, 'f64', True, True),
    'print_int': (False, 'i32', False, False),
    'print_i64': (False, 'i64', False, False),
    'print_f64': (False, 'f64', True, False),
}
# forms a placeholder does not expand (they are expanded before its text is read)
SUGAR = {'for', 'when', 'and', 'or', 'new', 'arr.new', 'arr.of', 'zero', 'make', 'dbg', 'format',
         'format_to', 'print', 'println', 'eprintln', 'expect', 'cond'}


def forms(text, i, q):
    """The forms from i up to the ')' that closes the call: [(start, end)],
    and the index just past that ')'. q is how a string's quote is written."""
    out = []
    n = len(text)
    while i < n:
        c = text[i]
        if c.isspace():
            i += 1
        elif c == ')':
            return out, i + 1
        elif text.startswith(q, i):
            j = i + len(q)
            while j < n and not text.startswith(q, j):
                j += 2 if text[j] == '\\' and q == '"' else 1
            out.append((i, j + len(q)))
            i = j + len(q)
        elif c in '([':
            depth, j = 0, i
            while j < n:
                if text.startswith(q, j):
                    k = j + len(q)
                    while k < n and not text.startswith(q, k):
                        k += 2 if text[k] == '\\' and q == '"' else 1
                    j = k + len(q)
                    continue
                if text[j] in '([':
                    depth += 1
                elif text[j] in ')]':
                    depth -= 1
                    if depth == 0:
                        break
                j += 1
            out.append((i, j + 1))
            i = j + 1
        else:
            j = i
            while j < n and not text[j].isspace() and text[j] not in '()[]':
                j += 1
            out.append((i, j))
            i = j
    return out, n


def fits(expr, q):
    """Whether a placeholder can hold expr as written."""
    if q in expr or ':' in expr or '\n' in expr or '{' in expr or '}' in expr:
        return False
    heads = re.findall(r'\(\s*([^\s()\[\]]+)', expr)
    return not any(h in SUGAR for h in heads)


def raw_spans(text):
    """The r#"..."# strings of a Rust file, where a quote is written plain."""
    return [(m.start(), m.end()) for m in re.finditer(r'r(#+)".*?"\1', text, re.S)]


def macro_spans(text):
    """The string literals Rust's format macros read, where { and } are
    Rust's: a format string's own {x} stays, and an AIPL brace is doubled."""
    spans = []
    for m in re.finditer(r'\b(?:format|println|eprintln|print|panic|write|writeln)!\(\s*(?:[A-Za-z_][A-Za-z0-9_.]*\s*,\s*)?', text):
        i = m.end()
        r = re.match(r'r(#*)"', text[i:])
        if r:
            end = text.find('"' + r.group(1), i + len(r.group(0)))
            spans.append((i, end))
        elif text.startswith('"', i):
            j = i + 1
            while j < len(text) and text[j] != '"':
                j += 2 if text[j] == '\\' else 1
            spans.append((i, j))
    return spans


def rewrite(text, rust):
    out, last, done, skipped = [], 0, 0, []
    raw = raw_spans(text) if rust else []
    fmt = macro_spans(text) if rust else []
    for m in re.finditer(r'\(call io\.(println_int|println_i64|println_f64|print_int|print_i64|print_f64)\s', text):
        if m.start() < last:
            continue
        q = '\\"' if rust and not any(a <= m.start() < b for a, b in raw) else '"'
        in_fmt = any(a <= m.start() < b for a, b in fmt)
        o, c = ('{{', '}}') if in_fmt else ('{', '}')
        label_first, ty, digits, newline = HELPERS[m.group(1)]
        args, end = forms(text, m.end(), q)
        want = (1 if label_first else 0) + 1 + (1 if digits else 0)
        if len(args) != want:
            skipped.append((m.start(), 'argument count'))
            continue
        k = 0
        lead = ''
        if label_first:
            a, b = args[0]
            label = text[a:b]
            if label.startswith(q):
                lead = label[len(q):-len(q)]
                # a format macro's braces are Rust's; elsewhere a brace is doubled
                if not in_fmt:
                    lead = lead.replace('{', '{{').replace('}', '}}')
            elif fits(label, q) and not in_fmt:
                lead = '{' + label + '}'
            else:
                skipped.append((m.start(), 'label'))
                continue
            k = 1
        a, b = args[k]
        value = text[a:b]
        spec = ''
        if digits:
            d = text[args[k + 1][0]:args[k + 1][1]]
            if not d.isdigit():
                skipped.append((m.start(), 'digits ' + d))
                continue
            spec = ':.' + d
        form = 'println' if newline else 'print'
        if fits(value, q) and not (in_fmt and ('{' in value or '}' in value)):
            new = f'({form} {q}{lead}{o}{value}{spec}{c}{q})'
        else:
            new = f'(block (let _shown:{ty} {value}) ({form} {q}{lead}{o}_shown{spec}{c}{q}))'
        out.append(text[last:m.start()])
        out.append(new)
        last = end
        done += 1
    out.append(text[last:])
    return ''.join(out), done, skipped


def main():
    args = sys.argv[1:]
    write = '--write' in args
    total = 0
    for path in [a for a in args if a != '--write']:
        text = open(path).read()
        new, n, skipped = rewrite(text, path.endswith('.rs'))
        total += n
        for at, why in skipped:
            print(f"left alone: {path}:{text.count(chr(10), 0, at) + 1}: {why}")
        if n and write:
            open(path, 'w').write(new)
        elif n:
            print(f"{path}: {n}")
    print(f"{total} calls {'rewritten' if write else 'to rewrite'}")


if __name__ == '__main__':
    main()
