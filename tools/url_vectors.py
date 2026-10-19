"""std/url's test vectors (tests/aipl/url/), against Python's
urllib.parse:

- generated URLs (schemes or none, user info with and without a password,
  hosts including IPv6, ports including bad ones, paths with dot
  segments, queries, fragments, each maybe empty) taken apart as
  urlsplit and its username, password, hostname, and port give them;
- relative references resolved against bases as RFC 3986 (5.2) resolves
  them: by the RFC's own examples (5.4.1 and 5.4.2), and on generated
  ones by the RFC's algorithm written out here (checked first against
  those examples). Python's urljoin differs from the RFC in three ways
  (it leaves dot segments in a network-path reference, drops empty
  segments, and gives an empty reference back as the base, fragment and
  all); the script counts where, and checks every other case with it;
- query strings read as parse_qsl(keep_blank_values=True) reads them
  (where every % has two hex digits and the bytes are UTF-8), and built
  as urlencode builds them.

    python3 tools/url_vectors.py tests/aipl/url [seed]

Each case prints one line. tests/test_url.rs runs the program in every
backend against the answers.
"""
import random, re, sys
from urllib.parse import parse_qsl, urlencode, urljoin, urlsplit, unquote_to_bytes

random.seed(int(sys.argv[2]) if len(sys.argv) > 2 else 1)
out = sys.argv[1]

def lit(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'

def pick(*xs):
    return random.choice(xs)

def gen_url():
    s = pick("", "http:", "HTTPS:", "ftp:", "mailto:", "x+y.z-1:", "urn:")
    auth = ""
    if random.random() < 0.7:
        user = pick("", "", "u@", "User:Pw@", ":p@", "a%40b:c@", "u:@")
        host = pick("a", "Example.COM", "h.x", "[::1]", "[2001:db8::7]", "1.2.3.4", "")
        port = pick("", "", ":80", ":", ":8080", ":65535", ":0", ":65536", ":x1")
        auth = "//" + user + host + port
    path = "".join(pick("/a", "/b/", "/.", "/..", "/./c", "/../d", "e", "/%7E", "/x;y", "") for _ in range(random.randint(0, 3)))
    q = pick("", "?", "?a=1", "?a=1&b=two+w", "?x?y")
    f = pick("", "#", "#top", "#a#b")
    return s + auth + path + q + f

lines, want = [], []
def case(call, result):
    lines.append("    " + call)
    want.append(result)

def show(b):
    return "-" if b is None else b

# parts
for _ in range(500):
    u = gen_url()
    try:
        sp = urlsplit(u)
        port = sp.port
        r = f"{sp.scheme}|{sp.netloc}|{sp.path}|{sp.query}|{sp.fragment}|{show(sp.username)}|{show(sp.password)}|{show(sp.hostname)}|{'-' if port is None else port}"
    except ValueError:
        r = "err"
    case(f"(call parts {lit(u)})", r)

# resolving: RFC 3986's examples, with the strict "http:g"
base = "http://a/b/c/d;p?q"
rfc = {"g:h": "g:h", "g": "http://a/b/c/g", "./g": "http://a/b/c/g", "g/": "http://a/b/c/g/", "/g": "http://a/g",
       "//g": "http://g", "?y": "http://a/b/c/d;p?y", "g?y": "http://a/b/c/g?y", "#s": "http://a/b/c/d;p?q#s",
       "g#s": "http://a/b/c/g#s", "g?y#s": "http://a/b/c/g?y#s", ";x": "http://a/b/c/;x", "g;x": "http://a/b/c/g;x",
       "g;x?y#s": "http://a/b/c/g;x?y#s", "": "http://a/b/c/d;p?q", ".": "http://a/b/c/", "./": "http://a/b/c/",
       "..": "http://a/b/", "../": "http://a/b/", "../g": "http://a/b/g", "../..": "http://a/", "../../": "http://a/",
       "../../g": "http://a/g", "../../../g": "http://a/g", "../../../../g": "http://a/g", "/./g": "http://a/g",
       "/../g": "http://a/g", "g.": "http://a/b/c/g.", ".g": "http://a/b/c/.g", "g..": "http://a/b/c/g..",
       "..g": "http://a/b/c/..g", "./../g": "http://a/b/g", "./g/.": "http://a/b/c/g/", "g/./h": "http://a/b/c/g/h",
       "g/../h": "http://a/b/c/h", "g;x=1/./y": "http://a/b/c/g;x=1/y", "g;x=1/../y": "http://a/b/c/y",
       "g?y/./x": "http://a/b/c/g?y/./x", "g?y/../x": "http://a/b/c/g?y/../x", "g#s/./x": "http://a/b/c/g#s/./x",
       "g#s/../x": "http://a/b/c/g#s/../x", "http:g": "http:g"}
for ref, r in rfc.items():
    case(f"(call join {lit(base)} {lit(ref)})", r)
# RFC 3986's algorithm (5.2), with undefined parts as None
def split(u):
    m = re.fullmatch(r"(?:([^:/?#]+):)?(?://([^/?#]*))?([^?#]*)(?:\?([^#]*))?(?:#(.*))?", u, re.S)
    return list(m.groups())

def remove_dots(path):
    out, i = [], path
    while i:
        if i.startswith("../"): i = i[3:]
        elif i.startswith("./"): i = i[2:]
        elif i.startswith("/./"): i = i[2:]
        elif i == "/.": i = "/"
        elif i.startswith("/../"): i = i[3:]; out = out[:-1]
        elif i == "/..": i = "/"; out = out[:-1]
        elif i in (".", ".."): i = ""
        else:
            k = i.find("/", 1 if i.startswith("/") else 0)
            k = len(i) if k < 0 else k
            out.append(i[:k]); i = i[k:]
    return "".join(out)

def rfc_join(base, ref):
    bs, ba, bp, bq, bf = split(base)
    rs, ra, rp, rq, rf = split(ref)
    if rs is not None:
        ts, ta, tp, tq = rs, ra, remove_dots(rp), rq
    elif ra is not None:
        ts, ta, tp, tq = bs, ra, remove_dots(rp), rq
    else:
        if rp == "":
            tp, tq = bp, (rq if rq is not None else bq)
        else:
            if rp.startswith("/"):
                tp = remove_dots(rp)
            else:
                merged = "/" + rp if ba is not None and bp == "" else bp[:bp.rfind("/") + 1] + rp
                tp = remove_dots(merged)
            tq = rq
        ts, ta = bs, ba
    s = (ts + ":" if ts is not None else "") + ("//" + ta if ta is not None else "") + tp
    return s + ("?" + tq if tq is not None else "") + ("#" + rf if rf is not None else "")

for ref, r in rfc.items():
    assert rfc_join(base, ref) == r, (ref, rfc_join(base, ref), r)
# generated ones; where urljoin agrees, it checks them too
python_differs = 0
for _ in range(300):
    b = pick("http://a/b/c/d;p?q", "http://h", "http://h/", "https://u@h:1/x/y/z?k#f", "http://a/b/c/d/")
    ref = pick("", "//h2/p", "/x", "x") + "".join(pick("/.", "/..", "/y", "z", "/", ".", "..", "/./w") for _ in range(random.randint(0, 3))) + pick("", "?", "?n=1") + pick("", "#", "#g")
    want_rfc = rfc_join(b, ref)
    if urljoin(b, ref) != want_rfc:
        python_differs += 1
        assert ref == "" or ref.startswith("//") or "//" in ref.split("?")[0].split("#")[0], (b, ref, urljoin(b, ref), want_rfc)
    case(f"(call join {lit(b)} {lit(ref)})", want_rfc)

# query strings
def valid_query(q):
    for part in q.split("&"):
        if re.search(r"%(?![0-9a-fA-F]{2})", part):
            return False
        try:
            unquote_to_bytes(part.replace("+", " ")).decode("utf-8")
        except UnicodeDecodeError:
            return False
    return True
for _ in range(200):
    q = "&".join("".join(pick("a", "b", "=", "+", "%20", "%3D", "%26", "%C3%A9", "%zz", "%FF", "x") for _ in range(random.randint(0, 4))) for _ in range(random.randint(0, 4)))
    if valid_query(q):
        r = "|".join(f"{k}={v}" for k, v in parse_qsl(q, keep_blank_values=True))
    else:
        r = "err"
    case(f"(call pairs {lit(q)})", r)
for _ in range(100):
    kv = [("".join(pick("a", " ", "&", "=", "é", "/", "~", "_", "+") for _ in range(random.randint(0, 3))),
           "".join(pick("1", " ", "&", "%", "é", "?", "-") for _ in range(random.randint(0, 3)))) for _ in range(random.randint(1, 3))]
    args = " ".join(lit(x) for kvp in kv for x in kvp)
    case(f"(call build{len(kv)} {args})", urlencode(kv))

src = ''';; Generated by tools/url_vectors.py; do not edit.
(module vectors
  (import url)
  (import str)
  (import buf)
  (import alloc)
  (import vec)

  (fn parts [t:str] -> void
    (let a:alloc.Allocator (call alloc.default))
    (match_result (call url.parse a (call str.from_str t))
      (ok u
        (block
          (let o:(ptr buf.Buf) (call buf.make a 64))
          ;; as Python gives them: the scheme and host lowercase, none for
          ;; what is absent
          (call buf.push_bytes o (call lower (get u url.Url.scheme)))
          (call buf.push_byte o 124)
          (call buf.push_bytes o (get u url.Url.authority))
          (call buf.push_byte o 124)
          (call buf.push_bytes o (get u url.Url.path))
          (call buf.push_byte o 124)
          (call buf.push_bytes o (get u url.Url.query))
          (call buf.push_byte o 124)
          (call buf.push_bytes o (get u url.Url.fragment))
          (call buf.push_byte o 124)
          (if (get u url.Url.has_user) (call buf.push_bytes o (get u url.Url.user)) (call buf.push_byte o 45))
          (call buf.push_byte o 124)
          (if (get u url.Url.has_password) (call buf.push_bytes o (get u url.Url.password)) (call buf.push_byte o 45))
          (call buf.push_byte o 124)
          (if (gt (get (get u url.Url.host) str.Bytes.len) 0) (call buf.push_bytes o (call lower (get u url.Url.host))) (call buf.push_byte o 45))
          (call buf.push_byte o 124)
          (if (gte (get u url.Url.port) 0) (call buf.push_int o (get u url.Url.port)) (call buf.push_byte o 45))
          (call buf.push_byte o 10)
          (call buf.print o)
          (call buf.free o)
          (call url.free u)))
      (err e (call url.free_error a e) (sys.print "err"))))

  (fn lower [b:str.Bytes] -> str.Bytes
    (let o:(ptr buf.Buf) (call buf.make (call alloc.default) 16))
    (for i 0 (get b str.Bytes.len)
      (let c:i32 (call str.byte_at b i))
      (call buf.push_byte o (if (and (gte c 65) (lte c 90)) (+ c 32) c)))
    (call buf.bytes o))

  (fn join [base:str ref:str] -> void
    (let a:alloc.Allocator (call alloc.default))
    (match_result (call url.resolve_str a base ref)
      (ok s (println "{s}"))
      (err e (call url.free_error a e) (sys.print "err"))))

  (fn pairs [q:str] -> void
    (let a:alloc.Allocator (call alloc.default))
    (match_result (call url.query_pairs a (call str.from_str q))
      (ok v
        (block
          (let o:(ptr buf.Buf) (call buf.make a 64))
          (let k:i32 0)
          (while (lt k (call (vec.len str) v))
            (when (gt k 0) (call buf.push_byte o 124))
            (call buf.push_str o (call (vec.at str) v k))
            (call buf.push_byte o 61)
            (call buf.push_str o (call (vec.at str) v (+ k 1)))
            (set! k (+ k 2)))
          (call buf.push_byte o 10)
          (call buf.print o)
          (call buf.free o)
          (call (vec.free str) v)))
      (err e (call url.free_error a e) (sys.print "err"))))

  (fn build_from [v:(ptr (vec.Vec str))] -> void
    (println "{(call url.query_build v)}")
    (call (vec.free str) v))

  (fn build1 [k1:str v1:str] -> void
    (let v:(ptr (vec.Vec str)) (call (vec.make str) (call alloc.default) 2))
    (call (vec.push str) v k1) (call (vec.push str) v v1)
    (call build_from v))
  (fn build2 [k1:str v1:str k2:str v2:str] -> void
    (let v:(ptr (vec.Vec str)) (call (vec.make str) (call alloc.default) 4))
    (call (vec.push str) v k1) (call (vec.push str) v v1) (call (vec.push str) v k2) (call (vec.push str) v v2)
    (call build_from v))
  (fn build3 [k1:str v1:str k2:str v2:str k3:str v3:str] -> void
    (let v:(ptr (vec.Vec str)) (call (vec.make str) (call alloc.default) 6))
    (call (vec.push str) v k1) (call (vec.push str) v v1) (call (vec.push str) v k2) (call (vec.push str) v v2) (call (vec.push str) v k3) (call (vec.push str) v v3)
    (call build_from v))

  (fn main [] -> i32
''' + "\n".join(lines) + "\n    0))\n"
open(f"{out}/vectors.aipl", "w").write(src)
open(f"{out}/vectors.expected", "w").write("\n".join(want) + "\n")
print(f"{len(lines)} cases; urljoin differs from RFC 3986 on {python_differs} of 300 generated references, each one of its three known ways")
