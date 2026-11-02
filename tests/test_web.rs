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

/// Texts for std/ip: IPv4 and IPv6 shapes (groups of every length, `::`
/// anywhere, an IPv4 tail, leading zeros, numbers past the limits), ports,
/// brackets, zone indexes, and random strings of the characters involved.
fn ip_texts() -> Vec<String> {
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let mut out: Vec<String> = ["", "0.0.0.0", "255.255.255.255", "::", "::1", "1::", "::ffff:1.2.3.4", "1:2:3:4:5:6:7:8", "1:2:3:4:5:6:1.2.3.4", "1:2:3:4:5:6:7::", "::2:3:4:5:6:7:8", "1:2:3:4:5:6:7:8::", "[::1]:80", "127.0.0.1:0", "127.0.0.1:65535", "127.0.0.1:65536", "[::1%3]:80", "::1%3", "01.2.3.4", "1.2.3.04"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let num = |next: &mut dyn FnMut(u64) -> u64, radix: u64| -> String {
        // mostly 1 to 4 digits; sometimes none, or 5
        let digits = if next(8) == 0 { next(6) } else { 1 + next(4) };
        let mut s = String::new();
        for _ in 0..digits {
            let c = std::char::from_digit(next(radix) as u32, radix as u32).unwrap();
            s.push(if next(4) == 0 { c.to_ascii_uppercase() } else { c });
        }
        s
    };
    for _ in 0..4000 {
        let v4 = |next: &mut dyn FnMut(u64) -> u64| -> String {
            let parts = if next(5) == 0 { 3 + next(3) } else { 4 };
            (0..parts).map(|_| match next(8) { 0 => num(next, 10), 1 => next(300).to_string(), _ => next(256).to_string() }).collect::<Vec<_>>().join(".")
        };
        let v6 = |next: &mut dyn FnMut(u64) -> u64| -> String {
            let n = if next(4) == 0 { next(10) } else { 1 + next(8) };
            let mut g: Vec<String> = (0..n).map(|_| num(next, 16)).collect();
            if next(2) == 0 {
                let at = next(n + 1) as usize;
                g.insert(at, String::new());
                if at == 0 || at == g.len() - 1 {
                    g.insert(at, String::new());
                }
            }
            let mut s = g.join(":");
            if next(4) == 0 {
                s += if s.is_empty() || s.ends_with(':') { "" } else { ":" };
                s += &v4(next);
            }
            s
        };
        let port = |next: &mut dyn FnMut(u64) -> u64| -> String { if next(3) == 0 { num(next, 10) } else { next(70000).to_string() } };
        let t = match next(6) {
            0 => v4(&mut next),
            1 | 2 => v6(&mut next),
            3 => format!("{}:{}", v4(&mut next), port(&mut next)),
            4 => format!("[{}]:{}", v6(&mut next), port(&mut next)),
            _ => {
                let chars = b"0123456789abcdefABCDEF.:[]%";
                (0..next(24)).map(|_| chars[next(chars.len() as u64) as usize] as char).collect()
            }
        };
        out.push(t);
    }
    out
}

/// std/ip reads every generated text as Rust's IpAddr and SocketAddr
/// parsers do (refusing zone indexes, which Rust reads), in every backend.
#[test]
fn std_ip_reads_addresses_as_rust_does() {
    use std::net::{IpAddr, SocketAddr};
    let bytes = |ip: IpAddr| -> String {
        let b: Vec<u8> = match ip {
            IpAddr::V4(a) => a.octets().to_vec(),
            IpAddr::V6(a) => a.octets().to_vec(),
        };
        b.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ")
    };
    let (mut input, mut want) = (String::new(), Vec::new());
    for t in ip_texts() {
        input += &format!("A {t}\nE {t}\n");
        want.push(match t.parse::<IpAddr>() {
            Ok(ip) if !t.contains('%') => bytes(ip),
            _ => "-".to_string(),
        });
        want.push(match t.parse::<SocketAddr>() {
            Ok(sa) if !t.contains('%') => format!("{} port {}", bytes(sa.ip()), sa.port()),
            _ => "-".to_string(),
        });
    }
    let accepted = want.iter().filter(|w| *w != "-").count();
    assert!(accepted > 1200, "the generator should make many valid addresses: {accepted}");
    let dir = std::env::temp_dir().join(format!("aipl_ip_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = root().join("tests/aipl/net/ip_vectors.aipl");
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    assert!(Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).status().unwrap().success());
    assert!(Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).status().unwrap().success());
    let run = |mut c: Command| -> String {
        use std::io::Write;
        let mut child = c.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let text = input.clone();
        let feeder = std::thread::spawn(move || stdin.write_all(text.as_bytes()).unwrap());
        let o = child.wait_with_output().unwrap();
        feeder.join().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).split_inclusive('\n').filter(|l| !l.starts_with("[AIPL")).collect()
    };
    let mut vm = Command::new(AIPL);
    vm.arg("eval").arg(&src);
    let mut rt = Command::new(RUNNER);
    rt.arg(&wasm);
    for (backend, got) in [("VM", run(vm)), ("aipl-run", run(rt)), ("native", run(Command::new(&exe)))] {
        let texts: Vec<&str> = input.lines().collect();
        for (k, (g, w)) in got.lines().zip(&want).enumerate() {
            assert_eq!(g, w, "{backend}: {:?}", texts[k]);
        }
        assert_eq!(got.lines().count(), want.len(), "{backend}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
