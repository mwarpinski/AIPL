#!/usr/bin/env python3
"""Runs the Rust test suite as `cargo test --release --no-fail-fast` does (every
test binary, then the doc tests), recording each test as it starts and ends,
for tools/ci_status.py:

- target/ci_tests.jsonl: one event per line, each with its time: a binary
  starting (with its process id) and ending (counts, wall time, peak memory),
  a test starting and ending (result, time, and a failure's output);
- standard output: cargo's own report (`Running ...`, `test x ... ok`,
  failures, `test result: ...`), which tools/ci.sh keeps as
  target/ci_tests.log and reads its summary from;
- target/ci_last.json at the end: each binary's wall time, for the next
  run's estimate of what is left.

A test's own time comes from libtest's JSON output, which stable Rust gives
only to a binary run with RUSTC_BOOTSTRAP=1; it is set for the test binaries
alone, never for compiling them. Each binary gets the environment cargo would
give it: .cargo/config.toml's [env] (RUST_MIN_STACK: without it the deep
tests overflow their stacks) and CARGO_MANIFEST_DIR. Memory is per binary: a binary's tests run
as threads of one process, so no test has a peak of its own.

    python3 tools/ci_tests.py        (from anywhere; it works in its checkout)
"""
import json
import os
import subprocess
import sys
import time
import tomllib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGET = os.path.join(ROOT, 'target')
EVENTS = os.path.join(TARGET, 'ci_tests.jsonl')
LAST = os.path.join(TARGET, 'ci_last.json')


def test_env():
    """The environment cargo runs a test binary in: ours, with .cargo/config.toml's
    [env] (an entry does not replace a variable already set unless it says
    force = true, as in cargo) and CARGO_MANIFEST_DIR."""
    env = dict(os.environ, RUSTC_BOOTSTRAP='1', CARGO_MANIFEST_DIR=ROOT)
    path = os.path.join(ROOT, '.cargo', 'config.toml')
    if os.path.exists(path):
        with open(path, 'rb') as f:
            table = tomllib.load(f).get('env', {})
        for name, v in table.items():
            value, force = (v, False) if isinstance(v, str) else (v['value'], v.get('force', False))
            if isinstance(v, dict) and v.get('relative'):
                value = os.path.join(ROOT, value)
            if force or name not in os.environ:
                env[name] = value
    return env


def say(text):
    sys.stdout.write(text)
    sys.stdout.flush()


class Events:
    def __init__(self, path):
        self.f = open(path, 'w')

    def emit(self, **ev):
        ev['t'] = round(time.time(), 3)
        self.f.write(json.dumps(ev) + '\n')
        self.f.flush()


def test_binaries():
    """(label, source file, executable) of every test binary, built first."""
    built = subprocess.run(['cargo', 'test', '--release', '--no-run', '--message-format=json'],
                           cwd=ROOT, stdout=subprocess.PIPE, text=True)
    if built.returncode != 0:
        say('error: the tests did not build (cargo test --no-run failed)\n')
        sys.exit(built.returncode)
    found = []
    for line in built.stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get('reason') != 'compiler-artifact' or not m.get('executable') or not m.get('profile', {}).get('test'):
            continue
        t = m['target']
        kind = t['kind'][0]
        src = os.path.relpath(t['src_path'], ROOT)
        label = t['name'] if kind == 'test' else f"{t['name']} (unit tests)"
        found.append(({'lib': 0, 'bin': 1}.get(kind, 2), src, label, m['executable']))
    found.sort()
    return [(label, src, exe) for _, src, label, exe in found]


def run_binary(ev, label, src, exe):
    """Runs one test binary; returns (passed, failed)."""
    say(f"     Running {src} ({os.path.relpath(exe, ROOT)})\n")
    start = time.time()
    proc = subprocess.Popen([exe, '-Z', 'unstable-options', '--format', 'json', '--report-time'],
                            cwd=ROOT, env=test_env(),
                            stdout=subprocess.PIPE, stderr=sys.stdout, text=True, bufsize=1)
    ev.emit(type='binary', event='started', name=label, src=src, pid=proc.pid)
    counts = {'passed': 0, 'failed': 0, 'ignored': 0}
    failures = []
    seen_suite = False
    for line in proc.stdout:
        try:
            m = json.loads(line)
        except ValueError:
            m = None
        if not isinstance(m, dict) or 'type' not in m:
            say(line)  # something a test printed outside libtest's capture
            continue
        kind, event = m.get('type'), m.get('event')
        if kind == 'suite' and event == 'started':
            say(f"\nrunning {m.get('test_count', 0)} tests\n")
        elif kind == 'suite':
            seen_suite = True
        elif kind == 'test' and event == 'started':
            ev.emit(type='test', event='started', bin=label, name=m['name'])
        elif kind == 'test' and event == 'timeout':
            say(f"test {m['name']} has been running for over 60 seconds\n")
            ev.emit(type='test', event='slow', bin=label, name=m['name'])
        elif kind == 'test':
            result = {'ok': 'ok', 'failed': 'FAILED', 'ignored': 'ignored'}.get(event, event)
            counts[{'ok': 'passed', 'failed': 'failed'}.get(event, 'ignored')] += 1
            say(f"test {m['name']} ... {result}\n")
            out = m.get('stdout', '') if event == 'failed' else ''
            if event == 'failed':
                failures.append((m['name'], out))
            ev.emit(type='test', event=event, bin=label, name=m['name'], time=m.get('exec_time'), out=out[:4000])
    # the process's own resource use: its peak memory (not the processes it starts)
    _, status, usage = os.wait4(proc.pid, 0)
    proc.returncode = os.waitstatus_to_exitcode(status)
    wall = time.time() - start
    if proc.returncode != 0 and not seen_suite:
        say(f"error: {label} stopped with status {proc.returncode} before it finished\n")
        counts['failed'] = max(counts['failed'], 1)
    if failures:
        say("\nfailures:\n")
        for name, out in failures:
            say(f"\n---- {name} stdout ----\n{out}")
        say("\nfailures:\n" + ''.join(f"    {name}\n" for name, _ in failures))
    verdict = 'ok' if counts['failed'] == 0 else 'FAILED'
    say(f"\ntest result: {verdict}. {counts['passed']} passed; {counts['failed']} failed; "
        f"{counts['ignored']} ignored; 0 measured; 0 filtered out; finished in {wall:.2f}s\n\n")
    ev.emit(type='binary', event='finished', name=label, wall=round(wall, 3), rss_kb=usage.ru_maxrss,
            status=proc.returncode, **counts)
    return wall, counts['failed']


def run_doc_tests(ev):
    """The doc tests, as cargo test runs them last (no JSON for these)."""
    label = 'doc tests'
    start = time.time()
    proc = subprocess.Popen(['cargo', 'test', '--release', '--doc'], cwd=ROOT,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
    ev.emit(type='binary', event='started', name=label, src='(doc comments)', pid=proc.pid)
    passed = failed = 0
    for line in proc.stdout:
        say(line)
        if line.startswith('test result:'):
            words = line.replace(';', '').split()
            passed, failed = int(words[3]), int(words[5])
    proc.wait()
    wall = time.time() - start
    ev.emit(type='binary', event='finished', name=label, wall=round(wall, 3), rss_kb=0,
            status=proc.returncode, passed=passed, failed=failed, ignored=0)
    return wall, failed


def main():
    os.makedirs(TARGET, exist_ok=True)
    ev = Events(EVENTS)
    bins = test_binaries()
    ev.emit(type='run', event='started', binaries=len(bins) + 1)
    walls, failed = {}, 0
    for label, src, exe in bins:
        walls[label], f = run_binary(ev, label, src, exe)
        failed += f
    walls['doc tests'], f = run_doc_tests(ev)
    failed += f
    ev.emit(type='run', event='finished', failed=failed)
    with open(LAST, 'w') as out:
        json.dump(walls, out, indent=1)
    sys.exit(1 if failed else 0)


main()
