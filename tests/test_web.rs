//! std/url, std/http, std/html, and json.get_path (AIPL_SPEC.md 12.6):
//! tests/aipl/url/vectors.aipl takes generated URLs apart as Python's
//! urlsplit does, resolves references as RFC 3986 does (its own examples,
//! and generated ones by its algorithm), and reads and builds query
//! strings as parse_qsl and urlencode do (tools/url_vectors.py);
//! tests/aipl/html/vectors.aipl unescapes and escapes as Python's html
//! module does (tools/html_vectors.py); tests/aipl/http/api.aipl reads
//! requests and responses, refuses what could be read two ways, and
//! writes one; tests/aipl/url/api.aipl uses query_get, a Url written back,
//! and get_path; each the same in the VM, under aipl-run, and natively.

use std::path::PathBuf;
use std::process::Command;

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The program's standard output in each backend, run in a scratch directory.
fn outputs(rel: &str, name: &str) -> Vec<(&'static str, String)> {
    let dir = std::env::temp_dir().join(format!("aipl_web_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = root().join(rel);
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    assert!(Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).status().unwrap().success());
    assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success());
    let runs = [
        ("VM", Command::new(AIPL).arg("eval").arg(&src).current_dir(&dir).output().unwrap()),
        ("aipl-run", Command::new(RUNNER).arg(&wasm).current_dir(&dir).output().unwrap()),
        ("native", Command::new(&exe).current_dir(&dir).output().unwrap()),
    ];
    let out = runs
        .into_iter()
        .map(|(b, o)| {
            assert!(o.status.success(), "{b}: {}", String::from_utf8_lossy(&o.stderr));
            (b, String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect())
        })
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    out
}

#[test]
fn std_url_splits_and_resolves_as_python_and_rfc_3986_do() {
    let want = std::fs::read_to_string(root().join("tests/aipl/url/vectors.expected")).unwrap();
    for (backend, got) in outputs("tests/aipl/url/vectors.aipl", "url") {
        for (k, (g, w)) in got.lines().zip(want.lines()).enumerate() {
            assert_eq!(g, w, "{backend}, case {}", k + 1);
        }
        assert_eq!(got.lines().count(), want.lines().count(), "{backend}");
    }
}

#[test]
fn std_html_escapes_and_unescapes_as_python_does() {
    let want = std::fs::read_to_string(root().join("tests/aipl/html/vectors.expected")).unwrap();
    for (backend, got) in outputs("tests/aipl/html/vectors.aipl", "html") {
        for (k, (g, w)) in got.lines().zip(want.lines()).enumerate() {
            assert_eq!(g, w, "{backend}, case {}", k + 1);
        }
        assert_eq!(got.lines().count(), want.lines().count(), "{backend}");
    }
}

#[test]
fn std_http_reads_strictly_and_writes() {
    let want = "GET /a?b=1 HTTP/1.1 host x.org body [hello] took 60\nDELETE /x HTTP/1.0 host y body [] took 28\n200 OK body [Wikipedia] took 91\n404 Not Found body [no such page] took 38\n304 Not Modified body [] took 49\nerr a space in or after a header name at 20\nerr a folded header line (obs-fold) at 22\nerr both Transfer-Encoding and Content-Length at 66\nerr two different Content-Lengths at 57\nerr a transfer coding other than chunked at 44\nerr the body ends early at 43\nerr the head ends early at 25\nerr an empty or spaced target at 4\nerr a version that is not HTTP/d.d at 6\nerr a status that is not three digits at 9\nerr more headers than the limit at 22\nerr a body over the limit at 37\nerr a head over the limit at 64\n201 Created body [<p>made</p>] took 95\nI'm a Teapot | Unknown\n";
    for (backend, got) in outputs("tests/aipl/http/api.aipl", "http") {
        assert_eq!(got, want, "{backend}");
    }
}

#[test]
fn std_url_queries_and_json_paths_work_alike_everywhere() {
    let want = "2 | café au lait | (none) | (err)\nhost ::1, port 8443, back HTTPS://user@[::1]:8443/a/./b/../c?x#y\n10.0 | deep | (none) | (none) | (other) | valid: false true\n";
    for (backend, got) in outputs("tests/aipl/url/api.aipl", "urlapi") {
        assert_eq!(got, want, "{backend}");
    }
}
