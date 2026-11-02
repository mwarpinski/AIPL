#!/usr/bin/env python3
"""Stress one VM server (aipl eval --listen) the way tests/test_serve.rs talks
to it, under CPU load, and count empty answers.
    python3 stress.py SECONDS CLIENT_THREADS HOGS [program.aipl]"""
import os, signal, socket, subprocess, sys, threading, time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def kill_group(p):
    """Kills a server and everything it started (its own process group)."""
    try:
        os.killpg(p.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass

secs, threads, hogs = float(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3])
prog = sys.argv[4] if len(sys.argv) > 4 else os.path.join(ROOT, 'tests/aipl/net/serve_demo.aipl')
AIPL = os.path.join(ROOT, 'target/release/aipl')
port = 20000 + os.getpid() % 5000
at = f'127.0.0.1:{port}'
env = dict(os.environ, AIPL_NET_DEBUG='1')
server = subprocess.Popen([AIPL, 'eval', '--listen', at, prog, '--', at, '100000000'],
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, text=True, start_new_session=True)
hog_procs = [subprocess.Popen([sys.executable, '-c', 'while True: pass']) for _ in range(hogs)]
# the hogs go however the script ends
import atexit
atexit.register(lambda: [h.kill() for h in hog_procs])

# wait for it to listen
t0 = time.time()
while True:
    try:
        socket.create_connection(('127.0.0.1', port), timeout=1).close()
        break
    except OSError:
        if time.time() - t0 > 60:
            sys.exit('never listened')
        time.sleep(0.02)

stats = {'ok': 0, 'empty': 0, 'other': 0}
lock = threading.Lock()
stop = time.time() + secs

def client():
    while time.time() < stop:
        s = socket.create_connection(('127.0.0.1', port))
        s.settimeout(60)
        s.sendall(b'GET / HTTP/1.1\r\nHost: x\r\n\r\n')
        s.shutdown(socket.SHUT_WR)
        got = b''
        try:
            while True:
                d = s.recv(65536)
                if not d:
                    break
                got += d
        except OSError as e:
            got = got or b''
        s.close()
        k = 'ok' if got.endswith(b'hello\n') else ('empty' if not got else 'other')
        with lock:
            stats[k] += 1

ts = [threading.Thread(target=client) for _ in range(threads)]
for t in ts: t.start()
for t in ts: t.join()
for h in hog_procs: h.kill()
kill_group(server)
out, err = server.communicate()
print(stats)
lines = [l for l in err.splitlines() if 'net debug' in l]
from collections import Counter
print(Counter(l.split(':', 1)[1].strip()[:90] for l in lines).most_common(10))
