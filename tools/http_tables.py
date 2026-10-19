"""The reason phrases in aipl_src/std/http.aipl, from Python's
http.HTTPStatus.

    python3 tools/http_tables.py aipl_src/std/http.aipl

rewrites the function between the `;; @tables` and `;; @end` markers in
place; with no argument, prints it.
"""
import http, re, sys

rows = "\n".join(f'      ((eq status {s.value}) "{s.phrase}")' for s in sorted(http.HTTPStatus, key=lambda s: s.value))
text = f'''  (fn reason [status:i32] -> str (pub)
    (cond
{rows}
      (else "Unknown")))
'''

if len(sys.argv) < 2:
    print(text, end="")
    sys.exit()
path = sys.argv[1]
src = open(path).read()
pat = re.compile(r"(  ;; @tables \(tools/http_tables.py\)\n).*?(  ;; @end\n)", re.S)
assert len(pat.findall(src)) == 1
open(path, "w").write(pat.sub(lambda m: m.group(1) + text + m.group(2), src))
