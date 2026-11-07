#!/usr/bin/env python3
"""The standard library's reference, docs/STD_REFERENCE.md (docs/LAUNCH_CHECKLIST.md, section 2).

    python3 tools/std_reference.py --write     rewrite the generated blocks
    python3 tools/std_reference.py --check     fail if a generated block is not what `aipl doc` says now

For each module in aipl_src/std/ the reference holds a summary written by hand (what the module is for and the rules a
caller needs that no single function shows) and, between the markers

    <!-- api:NAME -->  ...  <!-- /api:NAME -->

the module's public items exactly as `aipl doc NAME` prints them (signature, the comment above it, its req/ens).
Nothing between the markers is edited by hand: --write puts it back. Everything outside the markers is the author's.

It runs the first of: $AIPL, target/release/aipl, bootstrap/aipl-linux-x86_64 (the self-hosted `aipl`, whose `doc`
is the same code). Exit status 1 when --check finds a difference, a module with no section, or a section with no module.
"""
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REF = os.path.join(ROOT, "docs", "STD_REFERENCE.md")
STD = os.path.join(ROOT, "aipl_src", "std")


def aipl():
    for c in (os.environ.get("AIPL"), os.path.join(ROOT, "target", "release", "aipl"),
              os.path.join(ROOT, "bootstrap", "aipl-linux-x86_64")):
        if c and os.path.exists(c):
            return c
    sys.exit("std_reference: no aipl to ask (set AIPL, or build with cargo build --release)")


def modules():
    return sorted(f[:-5] for f in os.listdir(STD) if f.endswith(".aipl"))


def api(exe, name):
    out = subprocess.run([exe, "doc", name], cwd=ROOT, capture_output=True, text=True)
    if out.returncode != 0:
        sys.exit(f"std_reference: aipl doc {name} failed: {out.stdout}{out.stderr}")
    text = out.stdout.rstrip("\n")
    # the first line names the module's file, as it was found; the reference is the same wherever the checkout is
    return re.sub(r"^module (\S+) \(.*\)$", r"module \1", text, count=1, flags=re.M)


def block(name, body):
    return f"<!-- api:{name} -->\n```text\n{body}\n```\n<!-- /api:{name} -->"


BLOCK = re.compile(r"<!-- api:([a-z0-9_]+) -->\n.*?<!-- /api:\1 -->", re.S)


def main():
    mode = "--write" if "--write" in sys.argv else "--check" if "--check" in sys.argv else None
    if mode is None:
        sys.exit(__doc__)
    exe = aipl()
    text = open(REF, encoding="utf-8").read()
    have = [m.group(1) for m in BLOCK.finditer(text)]
    findings = []
    for name in modules():
        if name not in have:
            findings.append(f"docs/STD_REFERENCE.md has no section for std/{name} (it needs `<!-- api:{name} -->` ... `<!-- /api:{name} -->`)")
    for name in have:
        if name not in modules():
            findings.append(f"docs/STD_REFERENCE.md has a section for std/{name}, which is not in aipl_src/std/")
    stale = []

    def fresh(m):
        name = m.group(1)
        if name not in modules():
            return m.group(0)
        new = block(name, api(exe, name))
        if new != m.group(0):
            stale.append(name)
        return new

    new_text = BLOCK.sub(fresh, text)
    if mode == "--write":
        if new_text != text:
            open(REF, "w", encoding="utf-8").write(new_text)
        print(f"std_reference: {len(have)} modules, {len(stale)} rewritten")
    else:
        for name in stale:
            findings.append(f"docs/STD_REFERENCE.md: the api block of std/{name} is not what `aipl doc {name}` prints: run tools/std_reference.py --write")
    for f in findings:
        print(f)
    if mode == "--check":
        print(f"{len(findings)} findings")
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
