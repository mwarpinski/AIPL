"""The tables in aipl_src/std/html.aipl, from Python's own: the named
character references std/html knows (HTML 4's 252 names, with apos and
the six uppercase legacy names, each meaning what HTML5 gives it), the
legacy ones that may come without a ';' (the keys of html.entities.html5
without one), and the numeric references Python maps to other characters
(html._invalid_charrefs: the Windows-1252 range and a few others).

    python3 tools/html_tables.py aipl_src/std/html.aipl

rewrites the functions between the `;; @tables` and `;; @end` markers in
place; with no argument, prints them.
"""
import html, html.entities as E, re, sys

legacy = sorted(k for k in E.html5 if not k.endswith(";"))
names = set(E.name2codepoint) | {"apos"} | set(legacy)
# each means what HTML5 says (as Python's html.unescape reads it): for
# lang and rang that is not HTML 4's character
named = {k: ord(E.html5[k + ";"]) for k in names}
for k in legacy:
    assert E.html5[k + ";"] == E.html5[k], k

def lit(s):
    return '"' + s + '"'

def cond(fn, rows, default):
    body = "\n".join(f"      ({test} {value})" for test, value in rows)
    return f"{fn}\n    (cond\n{body}\n      (else {default})))\n"

text = "\n".join([
    "  ;; The code point of the named reference name (without its ';'), or -1.\n" +
    cond("  (fn named [n:str.Bytes] -> i32", [(f"(call str.is n {lit(k)})", cp) for k, cp in sorted(named.items())], -1),
    "  ;; Whether name may come without its ';' (HTML5's legacy names).\n" +
    cond("  (fn legacy [n:str.Bytes] -> bool", [(f"(call str.is n {lit(k)})", "true") for k in legacy], "false"),
    "  ;; The character a numeric reference to n means instead of n, or -1\n  ;; (html._invalid_charrefs: mostly the Windows-1252 range).\n" +
    cond("  (fn replaced [n:i32] -> i32", [(f"(eq n {k})", ord(v)) for k, v in sorted(html._invalid_charrefs.items())], -1),
])

if len(sys.argv) < 2:
    print(text, end="")
    sys.exit()
path = sys.argv[1]
src = open(path).read()
pat = re.compile(r"(  ;; @tables \(tools/html_tables.py\)\n).*?(  ;; @end\n)", re.S)
assert len(pat.findall(src)) == 1
open(path, "w").write(pat.sub(lambda m: m.group(1) + text + m.group(2), src))
