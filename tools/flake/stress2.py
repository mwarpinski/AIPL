#!/usr/bin/env python3
"""The first request to a freshly started VM server, as tests/test_serve.rs's
exchange() makes it (connect, retrying every 20 ms until it is accepted; send;
shut down writing; read to the end), many times, under CPU load.
    python3 stress2.py ROUNDS PARALLEL HOGS"""
import os, signal, socket, subprocess, sys, threading, time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def kill_group(p):
    """Kills a server and everything it started (its own process group)."""
    try:
        os.killpg(p.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
from collections import Counter

rounds, parallel, hogs = int(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3])
AIPL = os.path.join(ROOT, 'target/release/aipl')
PROG = os.path.join(ROOT, 'tests/aipl/net/serve_keep.aipl')
env = dict(os.environ, AIPL_NET_DEBUG='1')
hog_procs = [subprocess.Popen([sys.executable, '-c', 'while True: pass']) for _ in range(hogs)]
# the hogs go however the script ends
import atexit
atexit.register(lambda: [h.kill() for h in hog_procs])
results = Counter()
bad = []
lock = threading.Lock()
next_port = [21000 + (os.getpid() % 50) * 100]

def one():
    with lock:
        port = next_port[0]; next_port[0] += 1
    at = f'127.0.0.1:{port}'
    server = subprocess.Popen([AIPL, 'eval', '--listen', at, PROG, '--', at],
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, text=True, start_new_session=True)
    t0 = time.time()
    attempts = 0
    while True:
        attempts += 1
        try:
            s = socket.create_connection(('127.0.0.1', port))
            break
        except OSError:
            if server.poll() is not None:
                return ('exited', '', server.communicate()[1])
            time.sleep(0.02)
    connected_after = time.time() - t0
    s.settimeout(60)
    s.sendall(b'GET /first HTTP/1.1\r\n\r\n')
    s.shutdown(socket.SHUT_WR)
    got = b''
    err = None
    try:
        while True:
            d = s.recv(65536)
            if not d:
                break
            got += d
    except OSError as e:
        err = repr(e)
    s.close()
    kill_group(server)
    out, serr = server.communicate()
    kind = 'ok' if got.endswith(b'/first') else ('empty' if not got else 'other')
    return (kind, f'attempts {attempts}, connected after {connected_after:.2f}s, client error {err}', serr)

def worker(n):
    for _ in range(n):
        kind, info, serr = one()
        with lock:
            results[kind] += 1
            if kind != 'ok':
                bad.append((kind, info, serr.strip()[-600:]))

per = rounds // parallel
ts = [threading.Thread(target=worker, args=(per,)) for _ in range(parallel)]
for t in ts: t.start()
for t in ts: t.join()
for h in hog_procs: h.kill()
print(dict(results))
for b in bad[:8]:
    print('---', b[0], b[1]); print(b[2])
