#!/usr/bin/env python3
"""The notes of a release, as Markdown on standard output (docs/RELEASING.md).

  python3 tools/release_notes.py VERSION [--repo OWNER/NAME]

What the person installing needs: which platforms are first-class and which are a preview, how to install, how to
verify the download, and what changed since the release before this one (the subject of each commit on the main
line). The release workflow passes this to `gh release create`.
"""
import subprocess
import sys

REPO = "mwarpinski/AIPL"


def git(*args):
    return subprocess.run(["git", *args], capture_output=True, text=True, check=True).stdout


def previous_tag(version):
    tags = [t for t in git("tag", "--list", "v*", "--sort=-v:refname").split() if t != f"v{version}"]
    return tags[0] if tags else None


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    version = argv[1]
    repo = argv[argv.index("--repo") + 1] if "--repo" in argv else REPO
    prev = previous_tag(version)
    span = f"{prev}..HEAD" if prev else "HEAD"
    subjects = [s for s in git("log", "--first-parent", "--format=%s", span).splitlines() if s]
    out = []
    out.append(f"AIPL {version}, the AI Programming Language.\n")
    out.append("| Platform | Status | What it ships |")
    out.append("|---|---|---|")
    out.append("| Linux x86-64 | first-class | the self-hosted `aipl`, built from the checked-in seed with no Rust compiler, and `aipl-net` (the one Rust helper: host names and TLS) |")
    out.append("| Linux AArch64 | first-class | the same, built on an AArch64 machine |")
    out.append("| macOS (Apple Silicon) | preview | the Rust `aipl` and `aipl-run`; the self-hosted `aipl` follows once the Mach-O backend exists |")
    out.append("| Windows x86-64 | preview | the Rust `aipl` and `aipl-run`, installed by `install.ps1`; the self-hosted `aipl` follows once the PE backend exists |")
    out.append("")
    out.append("Install (Linux and macOS):\n")
    out.append("```sh")
    out.append(f"curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/{repo}/main/install.sh | sh")
    out.append("```\n")
    out.append("The installer checks the download against its SHA-256. To verify the release itself, which GitHub Actions signed with")
    out.append("Sigstore (no key to trust; the signature names the workflow that made it):\n")
    out.append("```sh")
    out.append(f"BASE=https://github.com/{repo}/releases/download/v{version}")
    out.append("curl -sSLO $BASE/SHA256SUMS && curl -sSLO $BASE/SHA256SUMS.bundle")
    out.append("cosign verify-blob --bundle SHA256SUMS.bundle \\")
    out.append(f"  --certificate-identity 'https://github.com/{repo}/.github/workflows/release.yml@refs/tags/v{version}' \\")
    out.append("  --certificate-oidc-issuer https://token.actions.githubusercontent.com SHA256SUMS")
    out.append("sha256sum -c SHA256SUMS --ignore-missing      # on macOS: shasum -a 256 -c SHA256SUMS --ignore-missing")
    out.append("```\n")
    out.append(f"Changes since {prev}:\n" if prev else "Changes:\n")
    shown = subjects[:60]
    out.extend(f"- {s}" for s in shown)
    if len(subjects) > len(shown):
        out.append(f"- and {len(subjects) - len(shown)} more")
    print("\n".join(out))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
