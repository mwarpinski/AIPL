#!/usr/bin/env python3
"""Requests a second of an AIPL server, per backend and per way of serving (docs/design/NET_PLAN.md step 7).

    cargo build --release
    python3 tools/serve_bench.py                  # every backend, best of 3 runs
    python3 tools/serve_bench.py --runs 5
    python3 tools/serve_bench.py --only native    # backends whose name contains this
    python3 tools/serve_bench.py --quick          # a few hundred requests each, to see that it works

Each server answers `GET /` with a line of text. Two ways to serve (NET_PLAN.md section 2):

  A. a long-running server (tests/aipl/net/serve_demo.aipl, `serve.run`): the program owns the socket
     and answers each request on an arena, in the VM, under aipl-run, as a native executable, and as a
     launcher bundle (`--target wasm`);
  B. a fresh instance per request (tests/aipl/net/fresh_demo.aipl, `serve.once`): `aipl serve --fresh`
     (aipl-run, an instance per connection), and executables made with `--serve` (a fork per connection
     natively; the bundle runs aipl-run's host).

The client is tools/loadgen.c: threads that each connect, send a request with `Connection: close`, and
read the answer to the end (these servers answer one request per connection), over 127.0.0.1. With
1 client the figure is a request's round trip, with 8 it is how fast a server that takes one connection at
a time can be kept busy. Python's http.server is measured the same way, for scale. A run is a warm-up and
then the timed requests, the server started fresh for each run; the table gives the best run's requests a
second (and the median and 99th percentile of its requests, in milliseconds, at 1 client).
"""
import argparse, os, signal, subprocess, sys, tempfile, time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
AIPL = os.path.join(ROOT, "target", "release", "aipl")
RUNNER = os.path.join(ROOT, "target", "release", "aipl-run")
SERVE = os.path.join(ROOT, "tests", "aipl", "net", "serve_demo.aipl")
FRESH = os.path.join(ROOT, "tests", "aipl", "net", "fresh_demo.aipl")
PORT = [18400]


def port():
    PORT[0] += 1
    return PORT[0]


def compile(args, src, out):
    r = subprocess.run([AIPL, "compile"] + args + [src, "-o", out], capture_output=True, text=True,
                       env=dict(os.environ, AIPL_RUNNER=RUNNER))
    if r.returncode != 0:
        sys.exit(f"compile {args} {src}: {r.stderr}")
    return out


def backends(work):
    """(name, way, requests, start) where start(at, n) gives the server's command (n requests, or None: no end)."""
    wasm = compile([], SERVE, os.path.join(work, "serve.wasm"))
    out = []
    out.append(("VM (`aipl eval`)", "A", 3000, lambda at, n: [AIPL, "eval", "--listen", at, SERVE, "--", at, str(n)]))
    out.append(("aipl-run", "A", 20000, lambda at, n: [RUNNER, "--listen", at, wasm, at, str(n)]))

    def exe(name, args, src):
        return lambda at, n: [compile(args + (["--listen", at] if src == SERVE else ["--serve", at]), src, os.path.join(work, f"{name}_{at.split(':')[1]}")), *([at, str(n)] if src == SERVE else [])]

    out.append(("native executable", "A", 20000, exe("a_native", ["--exe"], SERVE)))
    out.append(("bundle (`--target wasm`)", "A", 20000, exe("a_bundle", ["--exe", "--target", "wasm"], SERVE)))
    out.append(("`aipl serve --fresh` (aipl-run)", "B", 2000, lambda at, n: [AIPL, "serve", "--fresh", FRESH, "--listen", at]))
    out.append(("native executable `--serve`", "B", 3000, exe("b_native", ["--exe"], FRESH)))
    out.append(("bundle `--serve`", "B", 2000, exe("b_bundle", ["--exe", "--target", "wasm"], FRESH)))
    py = ("import http.server,sys\n"
          "class H(http.server.BaseHTTPRequestHandler):\n"
          "    def do_GET(self):\n"
          "        b=b'hello\\n'; self.send_response(200); self.send_header('Content-Length',str(len(b))); self.send_header('Connection','close'); self.end_headers(); self.wfile.write(b)\n"
          "    def log_message(self,*a): pass\n"
          "http.server.HTTPServer.request_queue_size=128\n"
          "http.server.HTTPServer(('127.0.0.1',int(sys.argv[1])),H).serve_forever()\n")
    out.append(("Python http.server (for scale)", "-", 5000, lambda at, n: ["python3", "-c", py, at.split(":")[1]]))
    return out


def measure(cmd, at, total, clients, loadgen):
    """Starts the server, runs the load, stops it; the loadgen line as a dict."""
    server = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    host, p = at.split(":")
    try:
        warm = max(20, min(200, total // 10))
        subprocess.run([loadgen, host, p, "/", str(warm), "1"], capture_output=True, text=True, timeout=300)
        r = subprocess.run([loadgen, host, p, "/", str(total), str(clients)], capture_output=True, text=True, timeout=600)
        line = r.stdout.strip().split("\n")[-1]
        return dict(kv.split("=") for kv in line.split())
    finally:
        try:
            os.killpg(server.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        server.wait()


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--only", default="")
    ap.add_argument("--quick", action="store_true")
    args = ap.parse_args()
    for f in (AIPL, RUNNER):
        if not os.path.exists(f):
            sys.exit("build first: cargo build --release")
    work = tempfile.mkdtemp(prefix="serve_bench_")
    loadgen = compile_c(work)
    print(f"| way | server | requests per run | 1 client: req/s | p50 ms | p99 ms | 8 clients: req/s |")
    print("|---|---|---|---|---|---|---|")
    for name, way, total, start in backends(work):
        if args.only and args.only not in name:
            continue
        total = 300 if args.quick else total
        best = {}
        for clients in (1, 8):
            runs = []
            for _ in range(args.runs):
                at = f"127.0.0.1:{port()}"
                # a server that stops after n requests answers the warm-up and the timed ones
                n = total + max(20, min(200, total // 10))
                try:
                    res = measure(start(at, n), at, total, clients, loadgen)
                except subprocess.TimeoutExpired:
                    continue
                if int(res["errors"]) == 0:
                    runs.append(res)
            if runs:
                best[clients] = max(runs, key=lambda r: float(r["rps"]))
        if 1 in best and 8 in best:
            b1, b8 = best[1], best[8]
            print(f"| {way} | {name} | {total} | {float(b1['rps']):,.0f} | {b1['p50_ms']} | {b1['p99_ms']} | {float(b8['rps']):,.0f} |", flush=True)
        else:
            print(f"| {way} | {name} | {total} | failed | | | |", flush=True)


def compile_c(work):
    out = os.path.join(work, "loadgen")
    r = subprocess.run(["gcc", "-O2", "-pthread", "-o", out, os.path.join(ROOT, "tools", "loadgen.c")], capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(r.stderr)
    return out


if __name__ == "__main__":
    main()
