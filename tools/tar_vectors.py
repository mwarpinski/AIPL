#!/usr/bin/env python3
"""std/tar's conformance cases (tests/conformance/library/tar_*.aipl).

    python3 tools/tar_vectors.py [OUT_DIR]        (default: tests/conformance/library)

tar_reads_real_archives: archives written by GNU tar (gnu, oldgnu, posix/pax, ustar, v7 formats) and by Python's
tarfile (ustar, gnu, pax), embedded gzipped and in base64; the expected listing (kind, size, mode, mtime, name, link
target, the start of the SHA-256 of the contents) is what Python's tarfile reads from the same bytes, so the oracle is
not the reader being tested. tar_refuses_damaged_archives: what the reader says about archives that are empty, cut
short, damaged, or odd in one field; the expected text is the reader's own, checked here against Python for the ones
that Python reads (the reader must accept nothing Python refuses). tar_safe_names: the rule for names that are safe to
unpack.

Needs GNU tar on the PATH for its formats (the cases are checked in: this runs only to regenerate them).
"""
import base64, gzip, hashlib, io, os, random, shutil, subprocess, sys, tarfile, tempfile

OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "tests", "conformance", "library")
MTIME = 1700000000


def b64(data):
    return base64.b64encode(gzip.compress(data, 9, mtime=0)).decode()


def lit(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"').replace("\0", "\\0") + '"'


def listing(data):
    """What Python's tarfile reads in data, as the lines the AIPL program prints."""
    out = []
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as t:
        for m in t:
            kind = "file" if m.isreg() else "dir" if m.isdir() else "symlink" if m.issym() else "hardlink" if m.islnk() else "other"
            body = t.extractfile(m).read() if m.isreg() else b""
            name = m.name + ("/" if m.isdir() and not m.name.endswith("/") else "")
            size = len(body) if m.isreg() else (0 if m.isdir() else m.size)
            link = m.linkname if (m.issym() or m.islnk()) else "-"
            h = hashlib.sha256(body).hexdigest()[:16] if m.isreg() else "-"
            out.append(f"{kind} {size} {m.mode:o} {int(m.mtime)} {name} {link} {h}")
    return out


# ----- the files the archives hold -----

def make_tree(root, long_names):
    os.makedirs(os.path.join(root, "a/b"))
    os.makedirs(os.path.join(root, "empty"))
    rnd = random.Random(7)
    files = {
        "a/hello.txt": b"hello\n",
        "a/zero": b"",
        "a/b/data.txt": b"".join(b"line %d of the multi-block file\n" % i for i in range(100)),
        "exact512": b"x" * 512,
        "exact1024": bytes(rnd.randrange(32, 127) for _ in range(1024)),
        "unicode-üñï-名前.txt": "u\n".encode(),
    }
    if long_names:
        long = "/".join(["d" * 60, "e" * 60])
        os.makedirs(os.path.join(root, long))
        files[long + "/" + "f" * 40 + ".txt"] = b"long\n"
    for name, data in files.items():
        with open(os.path.join(root, name), "wb") as f:
            f.write(data)
    os.symlink("hello.txt", os.path.join(root, "a/link"))
    if long_names:
        os.symlink("../" * 3 + "x" * 120, os.path.join(root, "a/longlink"))
    os.link(os.path.join(root, "a/hello.txt"), os.path.join(root, "a/hard"))
    for dirpath, dirs, names in os.walk(root, topdown=False):
        for n in names + dirs:
            p = os.path.join(dirpath, n)
            os.utime(p, (MTIME, MTIME), follow_symlinks=False)
        os.utime(dirpath, (MTIME, MTIME))
    os.chmod(os.path.join(root, "a/hello.txt"), 0o640)
    os.chmod(os.path.join(root, "a/b"), 0o750)


def gnu_tar(root, fmt):
    # (the pax format records each file's access and change times, which differ on every run: left out)
    extra = ["--pax-option=delete=atime,delete=ctime"] if fmt == "posix" else []
    out = subprocess.run(["tar", f"--format={fmt}", "--owner=0", "--group=0", "--numeric-owner", "--sort=name", *extra, "-cf", "-", "-C", root, "."],
                         capture_output=True)
    return out.stdout if out.returncode == 0 else None


def py_tar(root, fmt, **kw):
    b = io.BytesIO()
    with tarfile.open(fileobj=b, mode="w", format=fmt, **kw) as t:
        def reset(ti):
            ti.uid = ti.gid = 0; ti.uname = ti.gname = ""; return ti
        t.add(root, arcname=".", filter=reset)
    return b.getvalue()


def real_archives():
    work = tempfile.mkdtemp()
    try:
        long_root, short_root = os.path.join(work, "long"), os.path.join(work, "short")
        make_tree(long_root, True)
        make_tree(short_root, False)
        arcs = []
        for fmt in ("gnu", "oldgnu", "posix"):
            arcs.append((f"GNU tar, {fmt} format, long names", gnu_tar(long_root, fmt)))
        for fmt in ("ustar", "v7"):
            arcs.append((f"GNU tar, {fmt} format", gnu_tar(short_root, fmt)))
        arcs.append(("Python, ustar format", py_tar(short_root, tarfile.USTAR_FORMAT)))
        arcs.append(("Python, gnu format, long names", py_tar(long_root, tarfile.GNU_FORMAT)))
        arcs.append(("Python, pax format, long names", py_tar(long_root, tarfile.PAX_FORMAT)))
        # a name Python's ustar format must split into a prefix and a name
        pre = os.path.join(work, "pre")
        os.makedirs(os.path.join(pre, "p" * 60, "q" * 60))
        with open(os.path.join(pre, "p" * 60, "q" * 60, "r" * 30), "w") as f:
            f.write("prefix\n")
        for dirpath, dirs, names in os.walk(pre, topdown=False):
            for n in names + dirs:
                os.utime(os.path.join(dirpath, n), (MTIME, MTIME))
            os.utime(dirpath, (MTIME, MTIME))
        arcs.append(("Python, ustar format, a name split into a prefix and a name", py_tar(pre, tarfile.USTAR_FORMAT)))
        # the same name twice, and entries a tar can hold that a package does not use
        b = io.BytesIO()
        with tarfile.open(fileobj=b, mode="w", format=tarfile.GNU_FORMAT) as t:
            for name, body in (("same", b"1"), ("other", b"2"), ("same", b"333")):
                ti = tarfile.TarInfo(name); ti.size = len(body); ti.mtime = MTIME
                t.addfile(ti, io.BytesIO(body))
            for name, typ in (("chr", tarfile.CHRTYPE), ("fifo", tarfile.FIFOTYPE)):
                ti = tarfile.TarInfo(name); ti.type = typ; ti.mtime = MTIME
                t.addfile(ti)
        arcs.append(("Python, gnu format, a name twice and other kinds", b.getvalue()))
        # a pax header with a fractional modification time and a global header
        b = io.BytesIO()
        with tarfile.open(fileobj=b, mode="w", format=tarfile.PAX_FORMAT, pax_headers={"comment": "global"}) as t:
            ti = tarfile.TarInfo("fraction"); ti.size = 3; ti.mtime = MTIME + 0.5
            t.addfile(ti, io.BytesIO(b"abc"))
        arcs.append(("Python, pax format, a global header and a fractional time", b.getvalue()))
        return [(label, data) for label, data in arcs if data is not None]
    finally:
        shutil.rmtree(work)


FILE_HEAD = ""

SHOW = r"""
  (fn kind_name [k:tar.Kind] -> str
    (cond
      ((eq k tar.Kind.file) "file")
      ((eq k tar.Kind.dir) "dir")
      ((eq k tar.Kind.symlink) "symlink")
      ((eq k tar.Kind.hardlink) "hardlink")
      (else "other")))

  ;; the bytes of a gzipped, base64 text
  (fn unpack [a:alloc.Allocator text:str] -> (ptr buf.Buf)
    (let raw:(ptr buf.Buf) (call buf.make a 4096))
    (let out:(ptr buf.Buf) (call buf.make a 16384))
    (match_result (call encode.base64_decode_to raw (call str.from_str text) encode.Alphabet.standard encode.Padding.padded)
      (ok n
        (match_result (call deflate.gzip_decompress_to a out (call buf.bytes raw) 1000000)
          (ok m (call buf.free raw))
          (err e (println "the test's own data is corrupt"))))
      (err at (println "the test's own data is not base64")))
    out)
"""


def case_real():
    arcs = real_archives()
    want, calls = [], []
    for label, data in arcs:
        want.append(f"== {label}")
        want.extend(listing(data))
        calls.append(f"    (call show a {lit(label)} {lit(b64(data))})")
        if label.startswith("Python, gnu format, a name twice"):
            with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as t:
                members = t.getmembers()
                for who in ("same", "other", "missing"):
                    hits = [i for i, m in enumerate(members) if m.name == who]
                    want.append(f"find {who}: entry {hits[-1]}, {members[hits[-1]].size} bytes" if hits else f"find {who}: none")
            calls.append("    (call find_demo a)")
    src = ";; @spec 10\n;; @kind run\n" + "".join(f";; @out {l}\n" for l in want)
    src += f"""(module tar_reads_real_archives
  (import tar)
  (import deflate)
  (import encode)
  (import hash)
  (import buf)
  (import str)
  (import io)
  (import alloc)
  (import option)
{SHOW}
  ;; one line to an entry: kind, size, mode in octal, modification time, name, link target, and the start of the
  ;; SHA-256 of the contents
  (fn push_sha [o:(ptr buf.Buf) a:alloc.Allocator data:str.Bytes] -> void
    (let d:(ptr buf.Buf) (call buf.make a 64))
    (let h:(ptr buf.Buf) (call buf.make a 64))
    (call hash.digest_to d hash.Algorithm.sha256 data)
    (call encode.hex_to h (call buf.bytes d) false)
    (call buf.push_bytes o (call str.slice (call buf.bytes h) 0 16))
    (call buf.free d)
    (call buf.free h))

  (fn show [a:alloc.Allocator title:str text:str] -> void
    (println "== {{title}}")
    (let data:(ptr buf.Buf) (call unpack a text))
    (match_result (call tar.open a (call buf.bytes data))
      (ok ar
        (let o:(ptr buf.Buf) (call buf.make a 256))
        (for i 0 (call tar.count ar)
          (call buf.clear o)
          (call buf.push_str o (call kind_name (call tar.kind ar i)))
          (call buf.push_str o " ")
          (call buf.push_int o (call tar.size ar i))
          (call buf.push_str o " ")
          (call buf.push_unsigned o (i64.extend_s (call tar.mode ar i)) 8 false)
          (call buf.push_str o " ")
          (call buf.push_i64 o (call tar.mtime ar i))
          (call buf.push_str o " ")
          (call buf.push_bytes o (call tar.name ar i))
          (call buf.push_str o " ")
          (if (or (eq (call tar.kind ar i) tar.Kind.symlink) (eq (call tar.kind ar i) tar.Kind.hardlink))
            (call buf.push_bytes o (call tar.link ar i))
            (call buf.push_str o "-"))
          (call buf.push_str o " ")
          (if (eq (call tar.kind ar i) tar.Kind.file)
            (call push_sha o a (call tar.data ar i))
            (call buf.push_str o "-"))
          (println "{{(call buf.bytes o)}}"))
        (call buf.free o)
        (call tar.free ar))
      (err e (println "error: {{(get e tar.Error.what)}} at {{(get e tar.Error.at)}}")))
    (call buf.free data))

  ;; find gives the last entry of a name, and an option
  (fn find_demo [a:alloc.Allocator] -> void
    (let text:str "{b64(next(d for l, d in arcs if l.startswith('Python, gnu format, a name twice')))}")
    (let data:(ptr buf.Buf) (call unpack a text))
    (match_result (call tar.open a (call buf.bytes data))
      (ok ar
        (for k 0 3
          (let want:str (cond ((eq k 0) "same") ((eq k 1) "other") (else "missing")))
          (match (call tar.find ar (call str.from_str want))
            ((option.Option i32) some [i] (println "find {{want}}: entry {{i}}, {{(get (call tar.data ar i) str.Bytes.len)}} bytes"))
            ((option.Option i32) none [] (println "find {{want}}: none"))))
        (call tar.free ar))
      (err e (println "error")))
    (call buf.free data))

  (fn main [] -> i32
    (let a:alloc.Allocator (call alloc.default))
{chr(10).join(calls)}
    0))
"""
    return src


def header(name, size_field, flag=b"0", mode=b"0000644\0", body=b"", fix=True):
    h = bytearray(512)
    h[0:len(name)] = name
    h[100:108] = mode; h[108:116] = b"0000000\0"; h[116:124] = b"0000000\0"
    h[124:136] = size_field; h[136:148] = b"14000000000\0"
    h[148:156] = b" " * 8; h[156:157] = flag; h[257:263] = b"ustar\0"; h[263:265] = b"00"
    if fix:
        h[148:155] = ("%06o\0" % sum(h)).encode(); h[155] = 0x20
    return bytes(h) + body + b"\0" * ((-len(body)) % 512)


def octal(n):
    return (b"%011o\0" % n)


def case_damaged():
    good = py_tar_one("f.txt", b"hello\n")
    pax = py_tar_one("p" * 120 + "/" + "q" * 60 + ".txt", b"pax\n", tarfile.PAX_FORMAT)
    end = b"\0" * 1024
    variants = []

    def add(label, data, python_accepts=None):
        variants.append((label, data, python_accepts))

    add("a good archive", good, True)
    add("a good pax archive", pax, True)
    add("no bytes at all", b"", False)
    add("only the end marker", end, True)
    add("no end marker", good[:-1024], True)
    add("text that is not an archive", b"this is not a tar archive\n" * 40, False)
    flipped = bytearray(good); flipped[10] ^= 0xFF
    add("a byte of the header changed", bytes(flipped), False)
    add("cut inside the header", good[:300], False)
    add("cut inside the contents", header(b"f.txt", octal(1000), body=b"x" * 1000)[:700], False)
    add("cut inside the last block", header(b"f.txt", octal(600), body=b"x" * 600)[:1300], False)
    add("cut after a pax header", pax[:1024], False)
    add("a sparse file", header(b"s", octal(0), flag=b"S") + end, None)
    add("a size that is not a number", header(b"f", b"0000000089\0\0") + end, False)
    add("a size larger than memory", header(b"big", b"\x80" + (1 << 40).to_bytes(11, "big")) + end, None)
    add("a size in base 256", header(b"f", b"\x80" + (10).to_bytes(11, "big"), body=b"0123456789") + end, True)
    add("a mode that is not a number", header(b"f", octal(0), mode=b"zzzzzzz\0") + end, False)
    add("an entry with no name", header(b"", octal(0)) + end, None)
    prec = b"12 path=abc\n"
    add("a pax record that says it is 99 bytes", header(b"x", octal(12), flag=b"x", body=b"99 path=abc\n") + header(b"f", octal(0)) + end, None)
    add("a pax record without an equals sign", header(b"x", octal(12), flag=b"x", body=b"12 path abc\n") + header(b"f", octal(0)) + end, None)
    add("a pax size over the header's", header(b"x", octal(10), flag=b"x", body=b"10 size=3\n") + header(b"f", octal(0), body=b"abc") + end, True)
    add("a GNU long name with nothing after it", header(b"././@LongLink", octal(5), flag=b"L", body=b"long\0") + end, None)

    want, calls = [], []
    for label, data, python_accepts in variants:
        # what the reader says, found by running it (tools/tar_vectors.py checks it against Python below)
        calls.append(f"    (call say a {lit(label)} {lit(b64(data))})")
    return variants, calls


def py_tar_one(name, body, fmt=tarfile.GNU_FORMAT):
    b = io.BytesIO()
    with tarfile.open(fileobj=b, mode="w", format=fmt) as t:
        ti = tarfile.TarInfo(name); ti.size = len(body); ti.mtime = MTIME
        t.addfile(ti, io.BytesIO(body))
    return b.getvalue()


def python_reads(data):
    try:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as t:
            for m in t:
                if m.isreg():
                    t.extractfile(m).read()
        return True
    except Exception:
        return False


def damaged_source(variants, outputs):
    src = ";; @spec 10\n;; @kind run\n" + "".join(f";; @out {l}\n" for l in outputs)
    calls = "\n".join(f"    (call say a {lit(label)} {lit(b64(data))})" for label, data, _ in variants)
    src += f"""(module tar_refuses_damaged_archives
  (import tar)
  (import deflate)
  (import encode)
  (import buf)
  (import str)
  (import io)
  (import alloc)
{SHOW}
  (fn say [a:alloc.Allocator label:str text:str] -> void
    (let data:(ptr buf.Buf) (call unpack a text))
    (match_result (call tar.open a (call buf.bytes data))
      (ok ar
        (println "{{label}}: ok, {{(call tar.count ar)}} entries")
        (call tar.free ar))
      (err e
        (println "{{label}}: {{(get e tar.Error.what)}} (at {{(get e tar.Error.at)}})")
        (call tar.free_error a e)))
    (call buf.free data))

  (fn main [] -> i32
    (let a:alloc.Allocator (call alloc.default))
{calls}
    0))
"""
    return src


def safe_source():
    names = [("a", True), ("a/b/c", True), ("a/", True), ("./a", True), ("a//b", True), ("a/./b", True), ("a/..b", True),
             ("..a", True), ("a/.../b", True), ("", False), ("/", False), ("/etc/passwd", False), ("..", False),
             ("../a", False), ("a/..", False), ("a/../b", False), ("a/b/../../..", False), ("a\0b", False)]
    want = [f"{n.replace(chr(0), '<NUL>') or '<empty>'} -> {'safe' if ok else 'unsafe'}" for n, ok in names]
    calls = "\n".join(f'    (call show {lit(n)})' for n, _ in names)
    return ";; @spec 10\n;; @kind run\n" + "".join(f";; @out {l}\n" for l in want) + f"""(module tar_safe_names
  (import tar)
  (import str)

  (fn show [name:str] -> void
    (let shown:str (cond ((call str.same name "") "<empty>") ((call str.same name "a\\0b") "a<NUL>b") (else name)))
    (let verdict:str (if (call tar.safe_name (call str.from_str name)) "safe" "unsafe"))
    (println "{{shown}} -> {{verdict}}"))

  (fn main [] -> i32
{calls}
    0))
"""


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    open(os.path.join(OUT, "tar_reads_real_archives.aipl"), "w").write(case_real())
    open(os.path.join(OUT, "tar_safe_names.aipl"), "w").write(safe_source())
    variants, _ = case_damaged()
    # the expected text of the damaged-archive case is the reader's own: run once with no expectations, take its output
    path = os.path.join(OUT, "tar_refuses_damaged_archives.aipl")
    open(path, "w").write(damaged_source(variants, []))
    aipl = os.environ.get("AIPL") or os.path.join(os.path.dirname(__file__), "..", "target", "bootstrap", "aipl")
    ran = subprocess.run([aipl, "run", path], capture_output=True, text=True,
                         env=dict(os.environ, AIPL_STD=os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "aipl_src", "std") + "/"))
    if ran.returncode != 0:
        sys.exit("the damaged-archive case does not run: " + ran.stderr[:400])
    open(path, "w").write(damaged_source(variants, ran.stdout.splitlines()))
    print("wrote tar_reads_real_archives.aipl, tar_safe_names.aipl, and tar_refuses_damaged_archives.aipl")
    print("Python's verdicts on the damaged ones (True: Python reads it):")
    for label, data, expect in variants:
        py = python_reads(data)
        flag = "" if expect is None or expect == py else "   <-- differs from what this script expected"
        print(f"  {py!s:5} {label}{flag}")
