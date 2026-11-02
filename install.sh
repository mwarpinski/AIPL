#!/bin/sh
# Installs AIPL (https://github.com/mwarpinski/AIPL) for the current user.
#
#   curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/mwarpinski/AIPL/main/install.sh | sh
#
# Or read it first:
#
#   curl --proto '=https' --tlsv1.2 -sSfO https://raw.githubusercontent.com/mwarpinski/AIPL/main/install.sh
#   less install.sh && sh install.sh
#
# What it does, and nothing else:
#   1. finds the release built for this machine (Linux x86-64, Linux arm64,
#      or macOS arm64) and downloads it with curl (or wget), and checks it
#      against its published SHA-256; it needs no git and no Rust;
#   2. unpacks it into ~/.aipl: the two programs, aipl and aipl-run, in
#      ~/.aipl/bin, and the README, specification, and examples in
#      ~/.aipl/share;
#   3. writes ~/.aipl/env, which puts ~/.aipl/bin on PATH, and adds one line
#      sourcing it to ~/.profile, to ~/.bashrc if it exists, and to
#      ~/.zshenv if it exists or zsh is installed (with fish, it writes
#      fish's conf.d/aipl.fish), unless --no-modify-path is given;
#   4. checks the result by compiling and running a small program.
# Nothing runs as root and nothing is written outside ~/.aipl except that
# one line. `sh install.sh --uninstall` removes ~/.aipl and that line; run
# the installer again to update.
#
# To build from source instead (--from-source, or --ref, which implies it),
# it needs git, Rust's cargo, and a C linker: it fetches the source into
# ~/.aipl/src (a shallow clone of one branch, tag, or commit), builds it
# there with `cargo build --release` (a few minutes), and links the two
# programs into ~/.aipl/bin. A machine with no release gets this too.
#
# Options:
#   --version VERSION  the release to install, e.g. v0.1.0 (default: the latest)
#   --from-source      build the source instead of downloading a release
#   --ref REF          the branch, tag, or commit to build (default: main);
#                      implies --from-source
#   --no-modify-path   leave the shell profiles alone
#   --uninstall        remove an installation
#   -h, --help         this text
# Environment: AIPL_HOME (default ~/.aipl), AIPL_VERSION (as --version),
# AIPL_REF (as --ref), AIPL_REPO (the git URL for a source build, default
# https://github.com/mwarpinski/AIPL.git), AIPL_RELEASES (the URL the
# release files are under, default https://github.com/mwarpinski/AIPL/releases).

set -eu

AIPL_HOME="${AIPL_HOME:-$HOME/.aipl}"
AIPL_REPO="${AIPL_REPO:-https://github.com/mwarpinski/AIPL.git}"
FROM_SOURCE=no
[ -z "${AIPL_REF:-}" ] || FROM_SOURCE=yes
AIPL_REF="${AIPL_REF:-main}"
AIPL_VERSION="${AIPL_VERSION:-latest}"
AIPL_RELEASES="${AIPL_RELEASES:-https://github.com/mwarpinski/AIPL/releases}"
MODIFY_PATH=yes
UNINSTALL=no

say() { printf 'aipl-install: %s\n' "$1"; }
err() {
    printf 'aipl-install: error: %s\n' "$1" >&2
    exit 1
}
has() { command -v "$1" >/dev/null 2>&1; }

usage() {
    # the header comment, up to its first non-comment line
    sed -n '2,/^[^#]/p' "$0" 2>/dev/null | sed -e '$d' -e 's/^# \{0,1\}//' || true
}

# The line each shell profile gets, and the files that may have it.
source_line() { printf '. "%s/env"\n' "$AIPL_HOME"; }
fish_conf() { printf '%s/fish/conf.d/aipl.fish' "${XDG_CONFIG_HOME:-$HOME/.config}"; }

parse_args() {
    while [ $# -gt 0 ]; do
        case "$1" in
            --ref)
                [ $# -ge 2 ] || err "--ref needs a branch, tag, or commit"
                AIPL_REF="$2"
                FROM_SOURCE=yes
                shift 2
                ;;
            --version)
                [ $# -ge 2 ] || err "--version needs a release, e.g. v0.1.0"
                AIPL_VERSION="$2"
                shift 2
                ;;
            --from-source) FROM_SOURCE=yes; shift ;;
            --no-modify-path) MODIFY_PATH=no; shift ;;
            --uninstall) UNINSTALL=yes; shift ;;
            -h | --help) usage; exit 0 ;;
            *) err "unknown option '$1' (see --help)" ;;
        esac
    done
}

# TARGET names this machine's release, or is empty when there is none.
check_platform() {
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os/$arch" in
        Linux/x86_64) TARGET=x86_64-unknown-linux-gnu ;;
        Linux/aarch64 | Linux/arm64) TARGET=aarch64-unknown-linux-gnu ;;
        Darwin/arm64) TARGET=aarch64-apple-darwin ;;
        *) TARGET="" ;;
    esac
    case "$os/$arch" in
        Linux/x86_64 | Linux/aarch64 | Linux/arm64 | Darwin/arm64) ;;
        Darwin/*) say "note: $os $arch has no release; building from source" ;;
        *) say "note: $os $arch is untested; building from source may still work (Linux is the tested platform)" ;;
    esac
}

check_source_tools() {
    has git || err "git is needed to build from source; install it with your system's package manager"
    if ! has cargo; then
        err "Rust's cargo is needed to build AIPL. Install Rust with
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
then open a new shell (or run: . \"\$HOME/.cargo/env\") and run this installer again"
    fi
    if ! has cc && ! has gcc && ! has clang; then
        err "a C compiler is needed as the linker (cc, gcc, or clang); install one with your system's package manager (e.g. build-essential)"
    fi
}

check_download_tools() {
    has curl || has wget || err "curl (or wget) is needed to download AIPL"
    has tar || err "tar is needed to unpack AIPL"
    has sha256sum || has shasum || err "sha256sum (or shasum) is needed to check the download"
}

# download URL FILE
download() {
    if has curl; then
        curl --proto '=https,file' --tlsv1.2 -fsSL -o "$2" "$1" || return 1
    else
        wget -q -O "$2" "$1" || return 1
    fi
}

sha256_of() {
    if has sha256sum; then
        sha256sum "$1" | cut -d' ' -f1
    else
        shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

# The release for this machine, in AIPL_HOME: its two programs in bin, its
# documents and examples in share.
fetch_release() {
    name="aipl-$TARGET.tar.gz"
    if [ "$AIPL_VERSION" = latest ]; then
        base="$AIPL_RELEASES/latest/download"
    else
        base="$AIPL_RELEASES/download/$AIPL_VERSION"
    fi
    tmp="$(mktemp -d)"
    say "downloading $name ($AIPL_VERSION)"
    download "$base/$name" "$tmp/$name" ||
        err "could not download $base/$name (is there a release $AIPL_VERSION? --from-source builds the source instead)"
    download "$base/$name.sha256" "$tmp/$name.sha256" || err "could not download $base/$name.sha256"
    want="$(cut -d' ' -f1 "$tmp/$name.sha256")"
    [ "$(sha256_of "$tmp/$name")" = "$want" ] || err "the download does not match its SHA-256 ($want); not installing it"
    tar -C "$tmp" -xzf "$tmp/$name"
    dir="$tmp/aipl-$TARGET"
    if [ ! -x "$dir/bin/aipl" ] || [ ! -x "$dir/bin/aipl-run" ]; then
        err "the download is not an AIPL release"
    fi
    mkdir -p "$AIPL_HOME/bin"
    for b in aipl aipl-run; do
        # a source install links these into its build; remove the link, not its target
        rm -f "$AIPL_HOME/bin/$b"
        cp "$dir/bin/$b" "$AIPL_HOME/bin/$b.new"
        mv -f "$AIPL_HOME/bin/$b.new" "$AIPL_HOME/bin/$b"
    done
    rm -rf "$AIPL_HOME/share"
    cp -R "$dir/share" "$AIPL_HOME/share"
    rm -rf "$tmp"
}

# The source at AIPL_REF in AIPL_HOME/src: fetched shallowly by name or by
# commit, so a branch, a tag, and a commit work alike.
fetch_source() {
    src="$AIPL_HOME/src"
    if [ -d "$src/.git" ]; then
        say "updating $src to $AIPL_REF"
    else
        [ ! -e "$src" ] || err "$src exists and is not an AIPL checkout; move it away first"
        say "fetching AIPL ($AIPL_REF) into $src"
        mkdir -p "$src"
        git -C "$src" init -q
        git -C "$src" remote add origin "$AIPL_REPO"
    fi
    git -C "$src" remote set-url origin "$AIPL_REPO"
    git -C "$src" fetch -q --depth 1 origin "$AIPL_REF" || err "could not fetch '$AIPL_REF' from $AIPL_REPO"
    git -C "$src" checkout -q --force FETCH_HEAD
    git -C "$src" clean -q -fdx -e target
    say "at commit $(git -C "$src" rev-parse --short HEAD)"
}

build() {
    say "building (cargo build --release; this takes a few minutes)"
    (cd "$AIPL_HOME/src" && cargo build --release --locked) || err "the build failed (the messages above say why)"
    mkdir -p "$AIPL_HOME/bin"
    for b in aipl aipl-run; do
        ln -sf "../src/target/release/$b" "$AIPL_HOME/bin/$b"
    done
}

write_env() {
    cat >"$AIPL_HOME/env" <<EOF
# Puts AIPL on PATH (written by install.sh; sourced from your shell profile).
case ":\${PATH}:" in
    *:"$AIPL_HOME/bin":*) ;;
    *) export PATH="$AIPL_HOME/bin:\$PATH" ;;
esac
EOF
}

add_line() {
    file="$1"
    if [ -f "$file" ] && grep -qxF "$(source_line)" "$file"; then
        return
    fi
    source_line >>"$file"
    say "added to $file: $(source_line)"
}

modify_path() {
    add_line "$HOME/.profile"
    [ ! -f "$HOME/.bashrc" ] || add_line "$HOME/.bashrc"
    if [ -f "$HOME/.zshenv" ] || has zsh; then
        add_line "$HOME/.zshenv"
    fi
    if has fish; then
        conf="$(fish_conf)"
        mkdir -p "$(dirname "$conf")"
        printf 'fish_add_path -g "%s/bin"\n' "$AIPL_HOME" >"$conf"
        say "wrote $conf"
    fi
}

# Compiles and runs a program importing the standard library, from outside
# the source tree.
check_install() {
    tmp="$(mktemp -d)"
    cat >"$tmp/hello.aipl" <<'EOF'
(module hello
  (import io)
  (fn main [] -> i32
    (call io.println "AIPL is installed")
    0))
EOF
    (cd "$tmp" && "$AIPL_HOME/bin/aipl" compile hello.aipl -o hello.wasm >/dev/null && "$AIPL_HOME/bin/aipl-run" hello.wasm) ||
        err "the installed aipl could not compile and run a test program"
    rm -rf "$tmp"
    version="$("$AIPL_HOME/bin/aipl" --version 2>/dev/null)" || version="aipl (a build without --version)"
    say "installed $version"
}

uninstall() {
    [ -d "$AIPL_HOME" ] || err "nothing to remove: $AIPL_HOME does not exist"
    for file in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshenv"; do
        if [ -f "$file" ] && grep -qxF "$(source_line)" "$file"; then
            grep -vxF "$(source_line)" "$file" >"$file.aipl-tmp" || true
            cat "$file.aipl-tmp" >"$file"
            rm -f "$file.aipl-tmp"
            say "removed the AIPL line from $file"
        fi
    done
    rm -f "$(fish_conf)"
    rm -rf "$AIPL_HOME"
    say "removed $AIPL_HOME"
}

main() {
    parse_args "$@"
    if [ "$UNINSTALL" = yes ]; then
        uninstall
        return
    fi
    check_platform
    if [ "$FROM_SOURCE" = no ] && [ -z "$TARGET" ]; then
        FROM_SOURCE=yes
    fi
    mkdir -p "$AIPL_HOME"
    if [ "$FROM_SOURCE" = yes ]; then
        check_source_tools
        fetch_source
        build
        docs="$AIPL_HOME/src"
    else
        check_download_tools
        fetch_release
        docs="$AIPL_HOME/share"
    fi
    write_env
    if [ "$MODIFY_PATH" = yes ]; then
        modify_path
    fi
    check_install
    say "done. Open a new shell, or run: . \"$AIPL_HOME/env\""
    say "then try: aipl --help   (the language: $docs/README.md)"
}

main "$@"
