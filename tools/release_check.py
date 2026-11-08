#!/usr/bin/env python3
"""The facts a release depends on, checked in one place (docs/RELEASING.md).

  python3 tools/release_check.py version [--tag vX.Y.Z]   the version is one number everywhere
  python3 tools/release_check.py seeds                    each seed's hash is the one bootstrap/README.md records
  python3 tools/release_check.py set X.Y.Z                bump the version in every place that holds it
  python3 tools/release_check.py show                     print the version (aipl_src/aipl.aipl is the source)

The version lives in aipl_src/aipl.aipl (what `aipl --version` prints, and so what the self-hosted Linux release says),
in Cargo.toml and in the `aipl` package of Cargo.lock (the Rust build, which macOS and Windows ship). The first is the
source; the other two must agree with it. Exit status 0 when all is well, 1 on a finding, 2 on a usage error.
"""
import hashlib
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

AIPL_SRC = ROOT / "aipl_src/aipl.aipl"
CARGO_TOML = ROOT / "Cargo.toml"
CARGO_LOCK = ROOT / "Cargo.lock"

SELF_HOSTED = re.compile(r'"aipl (\d+\.\d+\.\d+)"')
TOML = re.compile(r'^version = "(\d+\.\d+\.\d+)"', re.M)
LOCK = re.compile(r'(\[\[package\]\]\nname = "aipl"\nversion = ")(\d+\.\d+\.\d+)(")')


def versions():
    found = {}
    m = SELF_HOSTED.findall(AIPL_SRC.read_text())
    found["aipl_src/aipl.aipl"] = m[0] if len(m) == 1 else f"{len(m)} places say it"
    m = TOML.search(CARGO_TOML.read_text())
    found["Cargo.toml"] = m.group(1) if m else "none"
    m = LOCK.search(CARGO_LOCK.read_text())
    found["Cargo.lock"] = m.group(2) if m else "none"
    return found


def check_version(tag):
    found = versions()
    source = found["aipl_src/aipl.aipl"]
    bad = [f"{name} says {v}, but aipl_src/aipl.aipl says {source}" for name, v in found.items() if v != source]
    if tag is not None and tag != f"v{source}":
        bad.append(f"the tag is {tag}, but the version is {source}")
    if not re.fullmatch(r"\d+\.\d+\.\d+", source):
        bad.append(f"aipl_src/aipl.aipl has no single version ({source})")
    for line in bad:
        print(f"release_check: {line}")
    if not bad:
        print(f"release_check: version {source} in {', '.join(found)}")
    return 1 if bad else 0


def set_version(new):
    if not re.fullmatch(r"\d+\.\d+\.\d+", new):
        print("release_check: a version is X.Y.Z")
        return 2
    s = AIPL_SRC.read_text()
    if len(SELF_HOSTED.findall(s)) != 1:
        print("release_check: aipl_src/aipl.aipl does not have exactly one \"aipl X.Y.Z\"")
        return 1
    AIPL_SRC.write_text(SELF_HOSTED.sub(f'"aipl {new}"', s))
    t = CARGO_TOML.read_text()
    CARGO_TOML.write_text(TOML.sub(f'version = "{new}"', t, count=1))
    l = CARGO_LOCK.read_text()
    if not LOCK.search(l):
        print("release_check: no aipl package in Cargo.lock")
        return 1
    CARGO_LOCK.write_text(LOCK.sub(rf"\g<1>{new}\g<3>", l, count=1))
    return check_version(None)


def check_seeds():
    readme = (ROOT / "bootstrap/README.md").read_text()
    bad = 0
    for name in sorted(p.name for p in (ROOT / "bootstrap").glob("aipl-linux-*")):
        digest = hashlib.sha256((ROOT / "bootstrap" / name).read_bytes()).hexdigest()
        if digest in readme:
            print(f"release_check: {name} {digest[:12]}... is recorded in bootstrap/README.md")
        else:
            print(f"release_check: bootstrap/{name} has sha256 {digest}, which bootstrap/README.md does not record")
            bad = 1
    return bad


def main(argv):
    if len(argv) >= 2 and argv[1] == "version":
        tag = argv[argv.index("--tag") + 1] if "--tag" in argv else None
        return check_version(tag)
    if len(argv) == 2 and argv[1] == "seeds":
        return check_seeds()
    if len(argv) == 3 and argv[1] == "set":
        return set_version(argv[2])
    if len(argv) == 2 and argv[1] == "show":
        print(versions()["aipl_src/aipl.aipl"])
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
