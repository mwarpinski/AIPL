"""std/regex's test vectors (tests/aipl/regex/): random patterns in the
syntax std/regex and Python's re share (literals, escapes, classes, \\d \\w
\\s, anchors, groups of each kind, alternation, every quantifier greedy and
lazy, the flags i m s), matched against random short texts over an
alphabet heavy in what the patterns use (with a newline and a non-ASCII
letter), and compared with Python's re under re.ASCII: whether the pattern
compiles, the first match's groups, every match (re.finditer), the text
with the matches replaced (re.sub, the template's $1 and ${name} as \\g<1>
and \\g<name>), and the text between the matches.

    python3 tools/regex_vectors.py tests/aipl/regex [seed]

Spans are byte offsets of the UTF-8 text. Each case prints one line.
tests/test_regex.rs runs the program in every backend against the answers.
"""
import random, re, sys

random.seed(int(sys.argv[2]) if len(sys.argv) > 2 else 1)
out = sys.argv[1]

def lit(s):  # an AIPL string literal for s
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\t", "\\t") + '"'

atoms = ["a", "a", "b", "b", "c", "1", "_", " ", ".", "\\.", "\\d", "\\w", "\\s", "\\W", "\\D",
         "[ab]", "[^a]", "[a-c]", "[\\d_]", "[^\\s]", "[.]", "\\n", "é", "[é1]"]
anchors = ["^", "$", "\\b", "\\B", "\\A", "\\Z"]
quants = ["*", "+", "?", "{2}", "{1,2}", "{0,1}", "{2,}", "{,2}", "*?", "+?", "??", "{1,2}?", "{,1}?"]

def gen(depth, names):
    r = random.random()
    if depth > 2 or r < 0.35:
        if random.random() < 0.12:
            return random.choice(anchors)
        a = random.choice(atoms)
        return a + random.choice(quants) if random.random() < 0.35 else a
    if r < 0.6:
        return "".join(gen(depth + 1, names) for _ in range(random.randint(2, 3)))
    if r < 0.72:
        return gen(depth + 1, names) + "|" + gen(depth + 1, names)
    kind = random.random()
    inner = gen(depth + 1, names)
    if kind < 0.5:
        g = "(" + inner + ")"
    elif kind < 0.75:
        g = "(?:" + inner + ")"
    else:
        names.append(f"n{len(names)}")
        g = f"(?P<{names[-1]}>" + inner + ")"
    return g + random.choice(quants) if random.random() < 0.4 else g

def bspan(text, i):
    return len(text[:i].encode())

def span_text(text, s, e):
    return f"{bspan(text, s)},{bspan(text, e)}" if s >= 0 else "-"

lines, want = [], []
def case(call, result):
    lines.append("    " + call)
    want.append(result)

alphabet = ["a", "b", "c", "1", "_", " ", ".", "\n", "é", "ab", "ba"]
for _ in range(1500):
    names = []
    pat = gen(0, names)
    if random.random() < 0.15:
        pat = random.choice(["(?i)", "(?m)", "(?s)", "(?im)"]) + pat
    text = "".join(random.choice(alphabet) for _ in range(random.randint(0, 8)))
    if random.random() < 0.5:
        text = text.upper() if random.random() < 0.3 else text
    try:
        rx = re.compile(pat, re.ASCII)
    except re.error:
        case(f"(call find {lit(pat)} {lit(text)})", "err")
        continue
    m = rx.search(text)
    if m is None:
        first = "none"
    else:
        first = " ".join(span_text(text, *m.span(g)) for g in range(rx.groups + 1))
    every = "".join(" " + span_text(text, *mm.span()) for mm in rx.finditer(text))
    case(f"(call find {lit(pat)} {lit(text)})", f"{first} |{every}")
    # replace, with a template naming a group the pattern has
    groups = list(range(rx.groups + 1))
    pieces, pyt = [], []
    for _ in range(random.randint(1, 3)):
        r = random.random()
        if r < 0.4:
            g = random.choice(groups)
            pieces.append(f"${g}" if random.random() < 0.5 else f"${{{g}}}")
            pyt.append(f"\\g<{g}>")
        elif r < 0.55 and names:
            n = random.choice(names)
            pieces.append(f"${{{n}}}" if random.random() < 0.5 else f"${n}")
            pyt.append(f"\\g<{n}>")
        elif r < 0.65:
            pieces.append("$$")
            pyt.append("$")
        else:
            t = random.choice(["<", ">", "-", "x"])
            pieces.append(t)
            pyt.append(t)
    # a bare $name must not run into a following word character
    tpl = ""
    for p_, nxt in zip(pieces, pieces[1:] + [""]):
        tpl += p_ + ("-" if p_.startswith("$") and not p_.startswith("${") and p_ != "$$" and nxt[:1].isalnum() else "")
    pyt_s = ""
    for p_, q_, nxt in zip(pieces, pyt, pieces[1:] + [""]):
        pyt_s += q_ + ("-" if p_.startswith("$") and not p_.startswith("${") and p_ != "$$" and nxt[:1].isalnum() else "")
    case(f"(call sub {lit(pat)} {lit(text)} {lit(tpl)})", rx.sub(pyt_s, text).encode().hex())
    spans = [mm.span() for mm in rx.finditer(text)]
    prev, parts = 0, []
    for s, e in spans:
        parts.append(text[prev:s])
        prev = e
    parts.append(text[prev:])
    case(f"(call cut {lit(pat)} {lit(text)})", ",".join(p_.encode().hex() for p_ in parts))

# patterns both refuse
for pat in ["(", ")", "a(", "[a", "[z-a]", "a**", "*a", "+", "a{2,1}", "\\1", "(?P<a>x)(?P<a>y)",
            "a(?i)b", "\\q", "\\x4", "[\\d-z]", "^*", "\\b+", "(?P=a)", "{1}", "a|*"]:
    try:
        re.compile(pat, re.ASCII)
        raise SystemExit(f"Python accepts {pat!r}")
    except re.error:
        case(f'(call find {lit(pat)} "")', "err")
# patterns Python takes and std/regex refuses: lookaround, backreferences,
# possessive quantifiers, scoped flags, and counts over 1000
for pat in ["(?=a)", "(?!a)", "(?<=a)b", "(?<!a)b", "(a)\\1", "(?P<x>a)(?P=x)", "a*+", "(?i:a)", "a{1001}"]:
    re.compile(pat, re.ASCII)
    case(f'(call find {lit(pat)} "")', "err")

src = ''';; Generated by tools/regex_vectors.py; do not edit.
(module vectors
  (import regex)
  (import encode)
  (import str)
  (import buf)
  (import alloc)
  (import vec)
  (import option)

  (fn span_text [o:(ptr buf.Buf) s:(option.Option regex.Span)] -> void
    (match s
      ((option.Option regex.Span) some [v]
        (call buf.push_int o (get v regex.Span.start))
        (call buf.push_byte o 44)
        (call buf.push_int o (get v regex.Span.end)))
      ((option.Option regex.Span) none [] (call buf.push_byte o 45))))

  (fn find [pat:str text:str] -> void
    (let a:alloc.Allocator (call alloc.default))
    (let t:(ptr str.Bytes) (call str.from_str text))
    (match_result (call regex.compile a pat)
      (ok re
        (block
          (let o:(ptr buf.Buf) (call buf.make a 64))
          (match (call regex.captures a re t)
            ((option.Option (ptr regex.Captures)) some [c]
              (for g 0 (+ (call regex.groups re) 1)
                (when (gt g 0) (call buf.push_byte o 32))
                (call span_text o (call regex.group c g)))
              (call regex.free_captures c))
            ((option.Option (ptr regex.Captures)) none [] (call buf.push_str o "none")))
          (call buf.push_str o " |")
          (let all:(ptr (vec.Vec regex.Span)) (call regex.find_all a re t))
          (for k 0 (call (vec.len regex.Span) all)
            (let s:regex.Span (call (vec.at regex.Span) all k))
            (call buf.push_byte o 32)
            (call span_text o (make (option.Option regex.Span) some s)))
          (call (vec.free regex.Span) all)
          (call buf.push_byte o 10)
          (call buf.print o)
          (call buf.free o)
          (call regex.free re)))
      (err e (call regex.free_error a e) (sys.print "err"))))

  (fn sub [pat:str text:str tpl:str] -> void
    (let a:alloc.Allocator (call alloc.default))
    (match_result (call regex.compile a pat)
      (ok re
        (block
          (match_result (call regex.replace_all re (call str.from_str text) tpl)
            (ok s (println "{(call encode.hex s)}"))
            (err e (call regex.free_error a e) (sys.print "template err")))
          (call regex.free re)))
      (err e (call regex.free_error a e) (sys.print "err"))))

  (fn cut [pat:str text:str] -> void
    (let a:alloc.Allocator (call alloc.default))
    (match_result (call regex.compile a pat)
      (ok re
        (block
          (let parts:(ptr (vec.Vec (ptr str.Bytes))) (call regex.split a re (call str.from_str text)))
          (let o:(ptr buf.Buf) (call buf.make a 64))
          (for k 0 (call (vec.len (ptr str.Bytes)) parts)
            (when (gt k 0) (call buf.push_byte o 44))
            (call encode.hex_to o (call (vec.at (ptr str.Bytes)) parts k) false))
          (call buf.push_byte o 10)
          (call buf.print o)
          (call buf.free o)
          (call (vec.free (ptr str.Bytes)) parts)
          (call regex.free re)))
      (err e (call regex.free_error a e) (sys.print "err"))))

  (fn main [] -> i32
''' + "\n".join(lines) + "\n    0))\n"
open(f"{out}/vectors.aipl", "w").write(src)
open(f"{out}/vectors.expected", "w").write("\n".join(want) + "\n")
print(f"{len(lines)} cases")
