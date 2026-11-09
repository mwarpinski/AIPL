#!/usr/bin/env python3
"""Damaged archives through std/tar (aipl_src/std/tar.aipl).

    python3 tools/tar_fuzz.py [--cases N] [--seed S] [--aipl PATH]      (default: 2000 cases, seed 1)

Archives that Python's tarfile writes (ustar, gnu, and pax formats; long and non-ASCII names, links, fractional times)
are damaged at random (bytes changed, the end cut off, bytes inserted) and read by a program built with the compiler
(`--aipl`, else $AIPL, else target/bootstrap/aipl, so no Rust is needed). Two things must hold, and the exit status is 1 if one does not:
  - the reader never traps or crashes (it says an archive is fine, or says what is wrong);
  - where both accept an archive, Python's entries (name, size, SHA-256 of the contents) are the first entries of ours.
Python is the more forgiving reader after the first header (it stops quietly at a damaged one, so it may list fewer
entries), and the stricter one in two places (it refuses junk in the padding of an extended header, as old tars leave in
padding, and a header field this reader does not use that is not a number), so the counts of archives that only one of
them accepts are printed and are not failures.
"""
import argparse, hashlib, io, os, random, subprocess, sys, tarfile, tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

LISTER = r"""
(module lister
  (import io)
  (import str)
  (import buf)
  (import alloc)
  (import tar)
  (import hash)
  (import encode)

  (fn kind_name [k:tar.Kind] -> str
    (cond
      ((eq k tar.Kind.file) "file")
      ((eq k tar.Kind.dir) "dir")
      ((eq k tar.Kind.symlink) "symlink")
      ((eq k tar.Kind.hardlink) "hardlink")
      (else "other")))

  (fn list [a:alloc.Allocator data:str.Bytes] -> i32
    (match_result (call tar.open a data)
      (ok ar
        (let o:(ptr buf.Buf) (call buf.make a 256))
        (for i 0 (call tar.count ar)
          (call buf.clear o)
          (call buf.push_str o (call kind_name (call tar.kind ar i)))
          (call buf.push_str o "\t")
          (call buf.push_int o (call tar.size ar i))
          (call buf.push_str o "\t")
          (call encode.hex_to o (call tar.name ar i) false)
          (call buf.push_str o "\t")
          (let d:(ptr buf.Buf) (call buf.make a 64))
          (call hash.digest_to d hash.Algorithm.sha256 (call tar.data ar i))
          (call encode.hex_to o (call buf.bytes d) false)
          (call buf.free d)
          (call buf.print o)
          (call io.println ""))
        (call buf.free o)
        (call tar.free ar)
        0)
      (err e
        (call io.eprintln (format "tar: {(get e tar.Error.what)} at {(get e tar.Error.at)}"))
        (call tar.free_error a e)
        1)))

  (fn main [] -> i32
    (let a:alloc.Allocator (call alloc.default))
    (match_result (call io.read_stdin)
      (ok data (call list a data))
      (err e (call io.eprintln "cannot read standard input") 2))))
"""


def archive(fmt, rnd):
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w", format=fmt) as t:
        for i in range(rnd.randint(1, 5)):
            name = rnd.choice(["f", "dir/g", "é" * 70, "p" * 120 + "/q" * 30, "x" * 200])
            ti = tarfile.TarInfo(f"{name}{i}")
            data = bytes(rnd.randrange(256) for _ in range(rnd.choice([0, 1, 511, 512, 513, 2000])))
            ti.size = len(data)
            ti.mtime = rnd.choice([0, 1700000000, 1700000000.5])
            if rnd.random() < 0.2:
                ti.type, ti.linkname, ti.size, data = tarfile.SYMTYPE, "t" * rnd.choice([3, 130]), 0, b""
            try:
                t.addfile(ti, io.BytesIO(data))
            except (ValueError, tarfile.TarError):
                pass    # a name or link target too long for ustar: the archive is shorter
    return out.getvalue()


def damage(data, rnd):
    data = bytearray(data)
    for _ in range(rnd.choice([0, 1, 1, 2, 5])):
        k = rnd.random()
        if k < 0.6 and data:
            data[rnd.randrange(len(data))] = rnd.randrange(256)
        elif k < 0.8 and data:
            del data[rnd.randrange(len(data)):]
        else:
            data[rnd.randrange(len(data) + 1):0] = bytes(rnd.randrange(256) for _ in range(rnd.randrange(1, 40)))
    return bytes(data)


def python_entries(data):
    """(name, size, sha256) of each entry as Python's tarfile reads them, or None if it refuses the archive."""
    try:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as t:
            out = []
            for m in t:
                body = t.extractfile(m).read() if m.isreg() else b""
                out.append((m.name.encode("utf-8", "surrogateescape").rstrip(b"/"), len(body) if m.isreg() else 0, hashlib.sha256(body).hexdigest()))
            return out
    except Exception:
        return None


def keep(directory, n, data):
    if directory:
        os.makedirs(directory, exist_ok=True)
        open(os.path.join(directory, f"case_{n}.tar"), "wb").write(data)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cases", type=int, default=2000)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--aipl")
    ap.add_argument("--keep", metavar="DIR", help="save each archive that fails a check in DIR")
    args = ap.parse_args()
    aipl = os.path.abspath(args.aipl or os.environ.get("AIPL") or os.path.join(ROOT, "target", "bootstrap", "aipl"))
    env = dict(os.environ, AIPL_STD=os.path.join(ROOT, "aipl_src", "std") + "/")
    work = tempfile.mkdtemp()
    src, exe = os.path.join(work, "lister.aipl"), os.path.join(work, "lister")
    open(src, "w").write(LISTER)
    built = subprocess.run([aipl, "compile", "--exe", src, "-o", exe], capture_output=True, text=True, env=env)
    if built.returncode != 0:
        sys.exit("cannot build the lister with " + aipl + ": " + built.stderr[:400])
    rnd = random.Random(args.seed)
    crash = wrong = both = stricter = looser = 0
    for n in range(args.cases):
        data = damage(archive(rnd.choice([tarfile.USTAR_FORMAT, tarfile.GNU_FORMAT, tarfile.PAX_FORMAT]), rnd), rnd)
        ran = subprocess.run([exe], input=data, capture_output=True, env=env)
        if ran.returncode not in (0, 1):
            crash += 1
            keep(args.keep, n, data)
            print(f"case {n}: the reader exited with status {ran.returncode}: {ran.stderr[:200]!r}")
            continue
        theirs = python_entries(data)
        if ran.returncode == 1:
            stricter += theirs is not None
            continue
        if theirs is None:
            looser += 1
            continue
        both += 1
        ours = []
        for line in ran.stdout.split(b"\n"):
            if line:
                kind, size, name, digest = line.split(b"\t")
                ours.append((bytes.fromhex(name.decode()).rstrip(b"/"), int(size) if kind == b"file" else 0, digest.decode()))
        if ours[:len(theirs)] != theirs:
            wrong += 1
            keep(args.keep, n, data)
            print(f"case {n}: the entries differ: {len(ours)} here, {len(theirs)} in Python; first difference at "
                  f"{next((i for i, (x, y) in enumerate(zip(ours, theirs)) if x != y), min(len(ours), len(theirs)))}")
    print(f"{args.cases} damaged archives, seed {args.seed}: {crash} crashes, {wrong} wrong, {both} read by both; "
          f"{stricter} refused here that Python reads, {looser} read here that Python refuses")
    sys.exit(1 if crash or wrong else 0)


main()
