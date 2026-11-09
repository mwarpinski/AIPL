//! std/math's transcendental functions (exp, expm1, ln, log10, log2, log1p, pow_f64, sin, cos, tan, asin,
//! acos, atan, atan2, sinh, cosh, tanh, hypot, cbrt): ports of fdlibm, in AIPL, so that every backend
//! gives the same bits. Each is checked bit for bit against the Rust `libm` crate (musl's fdlibm, the same
//! algorithms) on generated inputs: random bit patterns (every exponent, subnormals, infinities, NaNs),
//! values in the ranges where each function does its work, the arguments each special case names, and, for
//! the trigonometric functions, the neighbourhoods of multiples of pi/2. A NaN is any NaN (WebAssembly
//! leaves its sign and payload open); a zero has its sign. The same inputs run as wasm under aipl-run, as a
//! native executable, and (the first few hundred) in the interpreter. glibc rounds some results differently
//! from fdlibm in the last place, so the system's functions (Rust's f64 methods) are only a check that each
//! answer is within 1 ulp (2 for tanh): `within_an_ulp_of_the_system_libm`. cbrt is checked against a copy of
//! the FreeBSD cube root the `libm` crate used before it went to a correctly rounded one.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const AIPL: &str = env!("CARGO_BIN_EXE_aipl");
const RUNNER: &str = env!("CARGO_BIN_EXE_aipl-run");

fn root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("aipl_mathf_{}_{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A small generator (xorshift64*), so that the cases are the same on every run.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
    /// Uniform in [lo, hi).
    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * ((self.next() >> 11) as f64 / (1u64 << 53) as f64)
    }
}

type Unary = fn(f64) -> f64;
type Binary = fn(f64, f64) -> f64;

const UNARY: [(&str, Unary); 16] = [
    ("exp", libm::exp),
    ("expm1", libm::expm1),
    ("ln", libm::log),
    ("log10", libm::log10),
    ("log2", libm::log2),
    ("log1p", libm::log1p),
    ("sin", libm::sin),
    ("cos", libm::cos),
    ("tan", libm::tan),
    ("asin", libm::asin),
    ("acos", libm::acos),
    ("atan", libm::atan),
    ("sinh", libm::sinh),
    ("cosh", libm::cosh),
    ("tanh", libm::tanh),
    ("cbrt", cbrt_fdlibm),
];

/// FreeBSD's s_cbrt.c (musl's cbrt), as the `libm` crate carried it until it moved to a correctly rounded
/// cube root (MIT or Apache-2.0, like the crate): the one the AIPL port is made from, error under 0.667
/// ulp. The crate's current `cbrt` is correctly rounded, so it differs from this in the last place for
/// about 0.2% of arguments.
#[allow(clippy::excessive_precision)]
fn cbrt_fdlibm(x: f64) -> f64 {
    const B1: u32 = 715094163;
    const B2: u32 = 696219795;
    const P0: f64 = 1.87595182427177009643;
    const P1: f64 = -1.88497979543377169875;
    const P2: f64 = 1.621429720105354466140;
    const P3: f64 = -0.758397934778766047437;
    const P4: f64 = 0.145996192886612446982;
    let x1p54 = f64::from_bits(0x4350000000000000);
    let mut ui: u64 = x.to_bits();
    let mut hx: u32 = (ui >> 32) as u32 & 0x7fffffff;
    if hx >= 0x7ff00000 {
        return x + x;
    }
    if hx < 0x00100000 {
        ui = (x * x1p54).to_bits();
        hx = (ui >> 32) as u32 & 0x7fffffff;
        if hx == 0 {
            return x;
        }
        hx = hx / 3 + B2;
    } else {
        hx = hx / 3 + B1;
    }
    ui &= 1 << 63;
    ui |= (hx as u64) << 32;
    let mut t = f64::from_bits(ui);
    let mut r = (t * t) * (t / x);
    t *= (P0 + r * (P1 + r * P2)) + ((r * r) * r) * (P3 + r * P4);
    ui = t.to_bits();
    ui = (ui + 0x80000000) & 0xffffffffc0000000;
    t = f64::from_bits(ui);
    let s = t * t;
    r = x / s;
    let w = t + t;
    r = (r - t) / (w + r);
    t + t * r
}

const BINARY: [(&str, Binary); 3] = [("pow_f64", libm::pow), ("atan2", libm::atan2), ("hypot", libm::hypot)];

/// The arguments every function is tried with.
fn specials() -> Vec<f64> {
    let mut v = vec![
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        2.0,
        -2.0,
        10.0,
        0.1,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        f64::MAX,
        f64::MIN,
        f64::MIN_POSITIVE,
        -f64::MIN_POSITIVE,
        5e-324,
        -5e-324,
        f64::EPSILON,
        1.0 + f64::EPSILON,
        1.0 - f64::EPSILON / 2.0,
        std::f64::consts::PI,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::FRAC_PI_4,
        std::f64::consts::E,
        std::f64::consts::LN_2,
        709.782712893384,
        709.79,
        -708.3964185322641,
        -745.1332191019411,
        -745.2,
        710.0,
        -1e-300,
        1e-300,
        1e300,
        -1e300,
        2.0f64.powi(1023),
        2.0f64.powi(-1022),
        2.0f64.powi(-28),
        2.0f64.powi(-54),
        2.0f64.powi(-1074),
        2.0f64.powi(53),
        2.0f64.powi(54),
        2.0f64.powi(60),
        2.0f64.powi(100),
    ];
    for k in 1..=40 {
        v.push(k as f64 * std::f64::consts::FRAC_PI_2);
        v.push(-(k as f64) * std::f64::consts::FRAC_PI_2);
    }
    v
}

/// The inputs of one unary function: specials, then bit patterns, then ranges, then the neighbourhoods
/// of multiples of pi/2 for the trigonometric ones.
fn unary_inputs(name: &str, n: usize) -> Vec<f64> {
    let mut rng = Rng(0x9E3779B97F4A7C15 ^ name.bytes().fold(7u64, |h, b| h.wrapping_mul(131) ^ b as u64));
    let mut v = specials();
    for _ in 0..n / 4 {
        v.push(f64::from_bits(rng.next()));
    }
    let ranges: &[(f64, f64)] = match name {
        "exp" | "expm1" => &[(-1.0, 1.0), (-40.0, 40.0), (-750.0, 710.0), (-1e-8, 1e-8), (-0.7, 0.7)],
        "ln" | "log10" | "log2" => &[(0.0, 3.0), (0.5, 2.0), (0.99, 1.01), (0.0, 1e6), (0.0, 1e-300), (1e300, 1.7e308)],
        "log1p" => &[(-1.0, 3.0), (-0.3, 0.5), (-1e-8, 1e-8), (0.0, 1e6), (1e-30, 1e-10)],
        "sin" | "cos" | "tan" => &[(-1.0, 1.0), (-10.0, 10.0), (-1e3, 1e3), (-1e6, 1e6), (-1e10, 1e10), (-1e300, 1e300), (-1e-9, 1e-9)],
        "asin" | "acos" => &[(-1.0, 1.0), (-0.5, 0.5), (0.9, 1.0), (-1.0, -0.9), (-1.2, 1.2), (-1e-9, 1e-9)],
        "atan" => &[(-3.0, 3.0), (-50.0, 50.0), (-1e10, 1e10), (-1e-9, 1e-9), (-1e300, 1e300)],
        "sinh" | "cosh" | "tanh" => &[(-1.0, 1.0), (-30.0, 30.0), (-720.0, 720.0), (-1e-9, 1e-9), (-0.6, 0.6)],
        _ => &[(-10.0, 10.0), (-1e6, 1e6), (-1e-6, 1e-6), (-1e300, 1e300), (0.0, 1e12)],
    };
    for k in 0..(n * 3 / 4) {
        let (lo, hi) = ranges[k % ranges.len()];
        v.push(rng.uniform(lo, hi));
    }
    if matches!(name, "sin" | "cos" | "tan") {
        // neighbourhoods of multiples of pi/2, where reduction cancels most
        for k in 0..n / 8 {
            let m = (rng.next() % 2000) as f64 + (k % 7) as f64 * 1e5;
            let near = m * std::f64::consts::FRAC_PI_2;
            let mut x = near;
            for _ in 0..(rng.next() % 6) {
                x = f64::from_bits(x.to_bits() + 1);
            }
            v.push(x);
            v.push(-x);
        }
        // huge multiples of pi/2: the large-argument reduction
        for k in 0..n / 16 {
            v.push(rng.uniform(1.0, 2.0) * 2.0f64.powi(20 + (k % 1000) as i32));
        }
    }
    v
}

fn binary_inputs(name: &str, n: usize) -> Vec<(f64, f64)> {
    let mut rng = Rng(0xC2B2AE3D27D4EB4F ^ name.bytes().fold(11u64, |h, b| h.wrapping_mul(131) ^ b as u64));
    let sp = specials();
    let mut v = Vec::new();
    for (i, &a) in sp.iter().enumerate() {
        for &b in sp.iter().skip(i % 3).step_by(3) {
            v.push((a, b));
            v.push((b, a));
        }
    }
    for _ in 0..n / 4 {
        v.push((f64::from_bits(rng.next()), f64::from_bits(rng.next())));
    }
    for k in 0..(n * 3 / 4) {
        let pair = match (name, k % 5) {
            ("pow_f64", 0) => (rng.uniform(0.0, 10.0), rng.uniform(-10.0, 10.0)),
            ("pow_f64", 1) => (rng.uniform(-10.0, 10.0), (rng.next() % 41) as f64 - 20.0),
            ("pow_f64", 2) => (rng.uniform(0.5, 1.5), rng.uniform(-1e5, 1e5)),
            ("pow_f64", 3) => (rng.uniform(0.0, 1e300), rng.uniform(-3.0, 3.0)),
            ("pow_f64", _) => (2.0f64.powi((rng.next() % 200) as i32 - 100), rng.uniform(-30.0, 30.0)),
            ("atan2", 0) => (rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)),
            ("atan2", 1) => (rng.uniform(-1e10, 1e10), rng.uniform(-1e10, 1e10)),
            ("atan2", 2) => (rng.uniform(-1e-300, 1e-300), rng.uniform(-1.0, 1.0)),
            ("atan2", 3) => (rng.uniform(-1e300, 1e300), rng.uniform(-1e300, 1e300)),
            ("atan2", _) => (rng.uniform(-5.0, 5.0), rng.uniform(-5.0, 5.0)),
            (_, 0) => (rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)),
            (_, 1) => (rng.uniform(-1e300, 1e300), rng.uniform(-1e300, 1e300)),
            (_, 2) => (rng.uniform(-1e-300, 1e-300), rng.uniform(-1e-300, 1e-300)),
            (_, 3) => (rng.uniform(-1e5, 1e5), rng.uniform(-1e-3, 1e-3)),
            (_, _) => (rng.uniform(-10.0, 10.0), rng.uniform(-10.0, 10.0)),
        };
        v.push(pair);
    }
    v
}

/// The test's lines (`NAME BITS [BITS]`) and the libm crate's answers' bits, in step.
fn cases(per_function: usize) -> (String, Vec<f64>) {
    let mut lines = String::new();
    let mut want = Vec::new();
    // MATH_ONLY=exp,ln runs just those (while a port is being written)
    let only = std::env::var("MATH_ONLY").unwrap_or_default();
    let wanted = |name: &str| only.is_empty() || only.split(',').any(|n| n == name);
    for (name, f) in UNARY.into_iter().filter(|(n, _)| wanted(n)) {
        for x in unary_inputs(name, per_function) {
            lines += &format!("{name} {}\n", x.to_bits() as i64);
            want.push(f(x));
        }
    }
    for (name, f) in BINARY.into_iter().filter(|(n, _)| wanted(n)) {
        for (x, y) in binary_inputs(name, per_function) {
            lines += &format!("{name} {} {}\n", x.to_bits() as i64, y.to_bits() as i64);
            want.push(f(x, y));
        }
    }
    (lines, want)
}

fn run(cmd: &mut Command, input: &[u8]) -> std::process::Output {
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let data = input.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    });
    let out = child.wait_with_output().unwrap();
    writer.join().unwrap();
    out
}

/// Compares the answers with libm's, bit for bit (any NaN is any NaN); the first failures, named.
fn compare(how: &str, lines: &str, got: &str, want: &[f64]) {
    let inputs: Vec<&str> = lines.lines().collect();
    let answers: Vec<&str> = got.lines().filter(|l| !l.starts_with("[AIPL")).collect();
    assert_eq!(answers.len(), want.len().min(inputs.len()), "{how}: one answer a line");
    let mut bad = Vec::new();
    for (k, (a, w)) in answers.iter().zip(want).enumerate() {
        let g = f64::from_bits(a.parse::<i64>().unwrap() as u64);
        let same = (g.is_nan() && w.is_nan()) || g.to_bits() == w.to_bits();
        if !same {
            let args: Vec<String> = inputs[k].split(' ').skip(1).map(|b| format!("{:e}", f64::from_bits(b.parse::<i64>().unwrap() as u64))).collect();
            bad.push(format!("{} ({}): got {:e} ({:#x}), libm {:e} ({:#x})", inputs[k].split(' ').next().unwrap(), args.join(", "), g, g.to_bits(), w, w.to_bits()));
        }
    }
    assert!(bad.is_empty(), "{how}: {} of {} differ, the first:\n{}", bad.len(), answers.len(), bad.iter().take(12).cloned().collect::<Vec<_>>().join("\n"));
}

#[test]
fn std_math_rounds_as_fdlibm_does() {
    let dir = scratch("bits");
    let src = root().join("tests/aipl/math/transcendental.aipl");
    let (wasm, exe) = (dir.join("p.wasm"), dir.join("p"));
    let o = Command::new(AIPL).arg("compile").arg(&src).arg("-o").arg(&wasm).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let (lines, want) = cases(2000);
    let o = run(Command::new(RUNNER).arg(&wasm), lines.as_bytes());
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    compare("aipl-run", &lines, &String::from_utf8_lossy(&o.stdout), &want);
    let o = run(&mut Command::new(&exe), lines.as_bytes());
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    compare("native", &lines, &String::from_utf8_lossy(&o.stdout), &want);
    // the interpreter is slower: every 40th case
    let few: Vec<usize> = (0..want.len()).step_by(40).collect();
    let all: Vec<&str> = lines.lines().collect();
    let head: String = few.iter().map(|&k| format!("{}\n", all[k])).collect();
    let head_want: Vec<f64> = few.iter().map(|&k| want[k]).collect();
    let o = run(Command::new(AIPL).arg("eval").arg(&src), head.as_bytes());
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    compare("VM", &head, &String::from_utf8_lossy(&o.stdout), &head_want);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The distance in units in the last place between two finite numbers of the same sign (0 if both are NaN
/// or the same infinity), or u64::MAX when one is not a number, an infinity, or of the other sign.
fn ulps(a: f64, b: f64) -> u64 {
    if a.is_nan() && b.is_nan() || a == b {
        return 0;
    }
    if !a.is_finite() || !b.is_finite() || (a.is_sign_negative() != b.is_sign_negative() && a != 0.0 && b != 0.0) {
        return u64::MAX;
    }
    let (x, y) = (a.to_bits() & !(1 << 63), b.to_bits() & !(1 << 63));
    if a.is_sign_negative() == b.is_sign_negative() { x.abs_diff(y) } else { x + y }
}

/// glibc (the system's libm, which Rust's own f64 methods call) rounds some results differently from
/// fdlibm in the last place; both are close to the true value, so each answer must be within 1 ulp of
/// the system's wherever that is finite; tanh, whose fdlibm form is a little looser, may be 2 apart.
#[test]
fn within_an_ulp_of_the_system_libm() {
    let dir = scratch("ulps");
    let src = root().join("tests/aipl/math/transcendental.aipl");
    let exe = dir.join("p");
    let o = Command::new(AIPL).args(["compile", "--exe"]).arg(&src).arg("-o").arg(&exe).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let (lines, _) = cases(1500);
    let o = run(&mut Command::new(&exe), lines.as_bytes());
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let got = String::from_utf8_lossy(&o.stdout).to_string();
    let system: [(&str, Unary); 16] = [
        ("exp", f64::exp),
        ("expm1", f64::exp_m1),
        ("ln", f64::ln),
        ("log10", f64::log10),
        ("log2", f64::log2),
        ("log1p", f64::ln_1p),
        ("sin", f64::sin),
        ("cos", f64::cos),
        ("tan", f64::tan),
        ("asin", f64::asin),
        ("acos", f64::acos),
        ("atan", f64::atan),
        ("sinh", f64::sinh),
        ("cosh", f64::cosh),
        ("tanh", f64::tanh),
        ("cbrt", f64::cbrt),
    ];
    let mut worst: std::collections::BTreeMap<String, u64> = Default::default();
    for (line, answer) in lines.lines().zip(got.lines()) {
        let f: Vec<&str> = line.split(' ').collect();
        let arg = |k: usize| f64::from_bits(f[k].parse::<i64>().unwrap() as u64);
        let g = f64::from_bits(answer.parse::<i64>().unwrap() as u64);
        let sys = match f.len() {
            2 => system.iter().find(|(n, _)| *n == f[0]).map(|(_, h)| h(arg(1))).unwrap(),
            _ => match f[0] {
                "pow_f64" => arg(1).powf(arg(2)),
                "atan2" => arg(1).atan2(arg(2)),
                _ => arg(1).hypot(arg(2)),
            },
        };
        // where the system gives an overflow, an underflow, or a NaN, the answers agree on being the same kind
        if !sys.is_finite() || sys == 0.0 || g == 0.0 || !g.is_finite() {
            continue;
        }
        let d = ulps(g, sys);
        let w = worst.entry(f[0].to_string()).or_default();
        *w = (*w).max(d);
        let allowed = if f[0] == "tanh" { 2 } else { 1 };
        assert!(d <= allowed, "{line}: AIPL {g:e}, system {sys:e}, {d} ulps apart");
    }
    eprintln!("the worst difference from the system's libm, in ulps: {worst:?}");
    let _ = std::fs::remove_dir_all(&dir);
}
