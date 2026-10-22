#!/usr/bin/env python3
"""Where the current (or last) local CI run is (tools/ci.sh): each step and
how long it took; during the tests, the binary running now, the tests in it
that are running and for how long, its memory now and that of the processes
it started; every binary so far with its result, time, and peak memory; the
slowest tests; the failures with the start of their output; and how long the
rest should take, from the last run's times.

    python3 tools/ci_status.py [CHECKOUT] [--watch [SECONDS]] [--all]

CHECKOUT defaults to the one this script is in. --watch redraws every two
seconds (or SECONDS) until the run ends; --all lists every binary, not only
the last few. It reads target/ci_steps.log (tools/ci.sh) and
target/ci_tests.jsonl (tools/ci_tests.py).
"""
import json
import os
import sys
import time

COLOR = sys.stdout.isatty()


def paint(code, text):
    return f"\033[{code}m{text}\033[0m" if COLOR else text


GREEN, RED, YELLOW, DIM, BOLD = (lambda t: paint('32', t)), (lambda t: paint('31', t)), \
    (lambda t: paint('33', t)), (lambda t: paint('2', t)), (lambda t: paint('1', t))


def dur(s):
    s = max(0, int(round(s)))
    if s < 60:
        return f"{s}s"
    if s < 3600:
        return f"{s // 60}m{s % 60:02d}s"
    return f"{s // 3600}h{s % 3600 // 60:02d}m"


def mb(kb):
    return f"{kb / 1024:.0f} MB" if kb else "-"


def steps(path):
    """(start time, [(step name, start time, notes)]) from ci_steps.log."""
    if not os.path.exists(path):
        return None, []
    lines = open(path).read().splitlines()
    if not lines:
        return None, []
    day = lines[0][:10]
    start = time.mktime(time.strptime(lines[0][:19], '%Y-%m-%d %H:%M:%S'))
    out = []
    for line in lines[1:]:
        if line.startswith(' '):
            if out:
                out[-1][2].append(line.strip())
            continue
        at = time.mktime(time.strptime(f"{day} {line[:8]}", '%Y-%m-%d %H:%M:%S'))
        if out and at < out[-1][1]:  # the run went past midnight
            at += 86400
        out.append([line[9:], at, []])
    return start, out


def events(path):
    if not os.path.exists(path):
        return []
    out = []
    for line in open(path):
        try:
            out.append(json.loads(line))
        except ValueError:
            pass  # a line being written
    return out


def rss_kb(pid):
    try:
        for line in open(f"/proc/{pid}/status"):
            if line.startswith('VmRSS:'):
                return int(line.split()[1])
    except OSError:
        pass
    return 0


def descendants(pid):
    """The processes started, directly or not, by pid."""
    parent = {}
    for d in os.listdir('/proc'):
        if d.isdigit():
            try:
                stat = open(f"/proc/{d}/stat").read()
                parent[int(d)] = int(stat[stat.rindex(')') + 2:].split()[1])
            except (OSError, ValueError):
                pass
    found, todo = [], [pid]
    while todo:
        p = todo.pop()
        kids = [c for c, pp in parent.items() if pp == p]
        found += kids
        todo += kids
    return found


def ci_running():
    for d in os.listdir('/proc'):
        if d.isdigit():
            try:
                cmd = open(f"/proc/{d}/cmdline", 'rb').read().replace(b'\0', b' ')
            except OSError:
                continue
            if b'tools/ci.sh' in cmd or b'ci_tests.py' in cmd:
                return True
    return False


def report(root, show_all):
    target = os.path.join(root, 'target')
    now = time.time()
    lines = []
    out = lines.append
    start, st = steps(os.path.join(target, 'ci_steps.log'))
    running = ci_running()
    if start is None:
        return "no CI run yet (target/ci_steps.log): run sh tools/ci.sh", True
    finished = bool(st) and st[-1][0] == 'done'
    state = GREEN('finished') if finished else (YELLOW('running') if running else RED('stopped (not running, not done)'))
    out(f"{BOLD('CI run')} started {time.strftime('%H:%M:%S', time.localtime(start))}, "
        f"{dur((st[-1][1] if finished else now) - start)} {'in all' if finished else 'so far'}: {state}")
    for k, (name, at, notes) in enumerate(st):
        if name == 'done':
            continue
        end = st[k + 1][1] if k + 1 < len(st) else (now if running else at)
        mark = YELLOW('>') if k == len(st) - 1 and not finished else ' '
        out(f" {mark} {name:<16} {dur(end - at):>7}  {'; '.join(notes)}")

    evs = events(os.path.join(target, 'ci_tests.jsonl'))
    if not evs:
        return '\n'.join(lines), finished or not running
    last = {}
    try:
        last = json.load(open(os.path.join(target, 'ci_last.json')))
    except (OSError, ValueError):
        pass
    total_bins = next((e['binaries'] for e in evs if e['type'] == 'run' and e['event'] == 'started'), 0)
    bins, order, tests = {}, [], {}
    for e in evs:
        if e['type'] == 'binary':
            b = bins.setdefault(e['name'], {'name': e['name']})
            if e['event'] == 'started':
                order.append(e['name'])
                b.update(start=e['t'], pid=e.get('pid'))
            else:
                b.update(end=e['t'], wall=e['wall'], rss=e.get('rss_kb', 0), passed=e['passed'], failed=e['failed'],
                         ignored=e.get('ignored', 0), status=e.get('status', 0))
        elif e['type'] == 'test':
            key = (e['bin'], e['name'])
            t = tests.setdefault(key, {'bin': e['bin'], 'name': e['name']})
            if e['event'] == 'started':
                t['start'] = e['t']
            elif e['event'] == 'slow':
                t['slow'] = True
            else:
                t.update(result=e['event'], time=e.get('time'), out=e.get('out', ''))
    done_bins = [bins[n] for n in order if 'end' in bins[n]]
    current = next((bins[n] for n in reversed(order) if 'end' not in bins[n]), None)
    passed = sum(1 for t in tests.values() if t.get('result') == 'ok') + sum(b['passed'] for b in done_bins if b['name'] == 'doc tests')
    failed = [t for t in tests.values() if t.get('result') == 'failed']
    crashed = [b for b in done_bins if b['failed'] and not any(t['bin'] == b['name'] for t in failed)]

    out('')
    head = f"{BOLD('tests')}: {len(done_bins)}/{total_bins or '?'} binaries, {GREEN(f'{passed} passed')}"
    head += f", {RED(f'{len(failed) + len(crashed)} failed') if failed or crashed else '0 failed'}"
    if current and last:
        left = sum(last.get(n, 0) for n in last if n not in bins)
        left += max(0.0, last.get(current['name'], 0) - (now - current['start']))
        so_far = sum(b['wall'] for b in done_bins) + (now - current['start'])
        frac = so_far / (so_far + left) if so_far + left else 0
        bar = '#' * int(frac * 24)
        head += f"  [{bar:<24}] about {dur(left)} left (last run's times)"
    out(head)

    if current and running:
        pid = current.get('pid')
        kids = descendants(pid) if pid else []
        mem = rss_kb(pid) if pid else 0
        kid_mem = sum(rss_kb(k) for k in kids)
        extra = f", {len(kids)} child process{'es' if len(kids) != 1 else ''} {mb(kid_mem)}" if kids else ''
        prev = last.get(current['name'])
        took = dur(now - current['start']) + (f" (last run {dur(prev)})" if prev else '')
        out(f"  {YELLOW('now')}: {BOLD(current['name'])}, {took}, memory {mb(mem)}{extra}")
        mine = [t for t in tests.values() if t['bin'] == current['name']]
        live = sorted((t for t in mine if 'start' in t and 'result' not in t), key=lambda t: t['start'])
        done_here = [t for t in mine if 'result' in t]
        for t in live[:12]:
            flag = RED(' (over 60s)') if t.get('slow') else ''
            out(f"      {YELLOW('running')} {dur(now - t['start']):>6}  {t['name']}{flag}")
        if len(live) > 12:
            out(f"      ... and {len(live) - 12} more running")
        ok_here = sum(1 for t in done_here if t['result'] == 'ok')
        bad_here = sum(1 for t in done_here if t['result'] == 'failed')
        out(f"      done here: {ok_here} passed" + (f", {RED(f'{bad_here} failed')}" if bad_here else ''))

    if failed or crashed:
        out('')
        out(BOLD(RED('failures')))
        for t in failed:
            out(f"  {RED('FAILED')} {t['bin']} :: {t['name']}  ({dur(t.get('time') or 0)})")
            body = [l for l in (t.get('out') or '').splitlines() if l.strip()]
            for l in body[:6]:
                out(DIM(f"      {l[:160]}"))
            if len(body) > 6:
                out(DIM(f"      ... {len(body) - 6} more lines in target/ci_tests.log"))
        for b in crashed:
            out(f"  {RED('FAILED')} {b['name']} stopped with status {b['status']} (target/ci_tests.log)")

    timed = sorted((t for t in tests.values() if t.get('time') is not None), key=lambda t: -t['time'])
    if timed:
        out('')
        out(BOLD('slowest tests'))
        for t in timed[:8]:
            out(f"  {dur(t['time']):>7}  {t['bin']} :: {t['name']}")

    if done_bins:
        out('')
        shown = done_bins if show_all else done_bins[-8:]
        title = 'binaries' if show_all or len(done_bins) <= 8 else f"binaries (last 8 of {len(done_bins)}; --all for every one)"
        out(BOLD(title) + DIM(f"   {'result':<9}{'tests':>6} {'time':>8} {'peak memory':>12}"))
        for b in shown:
            res = GREEN('ok') if not b['failed'] else RED('FAILED')
            pad = ' ' * (9 - (2 if not b['failed'] else 6))
            out(f"  {b['name']:<34} {res}{pad}{b['passed'] + b['failed']:>6} {dur(b['wall']):>8} {mb(b['rss']):>12}")
        heavy = max(done_bins, key=lambda b: b['rss'])
        out(DIM(f"  most memory: {heavy['name']}, {mb(heavy['rss'])} (a binary's tests share one process; "
                f"memory is per binary, not per test)"))
    return '\n'.join(lines), finished or not running


def main():
    args = sys.argv[1:]
    show_all = '--all' in args
    watch = None
    if '--watch' in args:
        i = args.index('--watch')
        watch = 2.0
        if i + 1 < len(args) and args[i + 1].replace('.', '', 1).isdigit():
            watch = float(args.pop(i + 1))
    rest = [a for a in args if not a.startswith('--')]
    root = os.path.abspath(rest[0]) if rest else os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    if watch is None:
        print(report(root, show_all)[0])
        return
    try:
        while True:
            text, over = report(root, show_all)
            sys.stdout.write("\033[H\033[2J" + text + "\n" + DIM(f"\n(every {watch:g}s; ctrl-c to stop)") + "\n")
            sys.stdout.flush()
            if over:
                break
            time.sleep(watch)
    except KeyboardInterrupt:
        pass


main()
