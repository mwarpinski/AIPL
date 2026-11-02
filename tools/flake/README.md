# Stress scripts for tests/test_serve.rs's intermittent failures

Written 2026-10-21 while diagnosing the empty first answers (docs/HANDOFF.md,
Open problems). Each starts servers the way tests/test_serve.rs does, under
CPU load, and counts answers that come back empty; the servers run with
`AIPL_NET_DEBUG=1` (src/net.rs and aipl-run --serve-fresh log what the
host saw, while that temporary logging is on the branch). Build first:
`cargo build --release`.

- `stress.py SECONDS THREADS HOGS` - one VM server (`aipl eval --listen`,
  tests/aipl/net/serve_demo.aipl) and many clients at once.
- `stress2.py ROUNDS PARALLEL HOGS` - a fresh VM server per round
  (serve_keep.aipl), its first request as `exchange()` makes it.
- `stress3.py ROUNDS PARALLEL HOGS` - the same with `aipl serve --fresh`
  (fresh_demo.aipl), whose aipl-run child is why each server runs in its
  own process group and is killed with it.

HOGS is how many busy processes run beside them (nproc is a fair load).
On 2026-10-21: stress.py 10,979 requests and stress2.py 4,000 rounds, no
empty answer; the failure has not been reproduced outside the test file.
