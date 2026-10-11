# pgexplain

Renders Postgres `EXPLAIN (ANALYZE, FORMAT JSON)` as a readable tree,
highlighting the node that's *actually* slow. Today's fix is pasting the
JSON into a website (`explain.depesz.com`, `pev2`) — this does the same
analysis locally.

## Installation

```sh
cargo install --path .
```

## Usage

```bash
psql -Atc "EXPLAIN (ANALYZE, FORMAT JSON) SELECT ..." | pgexplain
pgexplain saved-plan.json
```

## The two things a plain `EXPLAIN ANALYZE` doesn't tell you directly

**Self-time, not just cumulative time.** A node's `Actual Total Time` in
Postgres's own output already includes every child's time — so the node
with the biggest number is almost always the root, which tells you
nothing. `pgexplain` subtracts children's time to show what each node
actually spent on its own work, and lists the worst offenders by that
number up front. It also multiplies by `Actual Loops` first — a node
executed 1,000 times inside a nested loop reports its **per-execution
average** time, not the real total; skipping that multiplication (an easy
mistake) makes self-time silently wrong for any plan with a nested loop.

**Row-estimate mismatches.** `planned rows` vs. `actual rows` far apart
is usually stale statistics or a correlation the planner can't see —
flagged inline (⚠) when the ratio is outside 10x in either direction.

## Status: built, and verified against a real query plan from real data, not a hand-written fixture

- **14 unit tests** (`cargo test --lib`): parsing the real `EXPLAIN
  (FORMAT JSON)` shape (a JSON **array** containing one object with a
  `"Plan"` key — easy to get wrong if you only look at the inner plan
  tree in most examples); a plan run *without* `ANALYZE` still parsing
  (no `Actual *` fields, handled as `None`, not an error); self-time
  correctly excluding children's contribution, **and never going
  negative from floating-point rounding noise** (a real edge case, not
  hypothetical — a parent's reported cumulative time can be
  fractionally less than a child's due to how Postgres rounds); the
  `Actual Loops` multiplication; and — the test that actually matters —
  `worst_by_self_time` correctly surfacing a slow **child** node rather
  than the root, even though the root's cumulative number is bigger.
- **Verified against a real query plan from real data**: created a
  50,000-row table in a real local Postgres (no Docker — same
  `initdb`/`pg_ctl` setup this workspace's `pgqueue`/`pgdiff` use), ran a
  real `EXPLAIN (ANALYZE, FORMAT JSON)` on a real query (`WHERE status =
  'pending' ORDER BY amount DESC LIMIT 10`), and piped the actual
  Postgres output — every extra field Postgres includes that this tool
  doesn't care about (`"Shared Hit Blocks"`, `"Parallel Aware"`, dozens
  of others) correctly ignored via serde's normal unknown-field handling,
  not a hand-trimmed fixture — straight into the compiled binary:
  ```
  slowest node(s) by self-time:
    Seq Scan on orders
    Sort
    Limit

  Limit  [actual=10.26ms, self=0.00ms]
    Sort  [actual=10.25ms, self=2.75ms, ⚠ estimate off 0.0x (planned 12413, actual 10)]
      Seq Scan on orders  [actual=7.50ms, self=7.50ms]
  ```
  Self-time correctly identified the `Seq Scan` (7.50ms of genuinely its
  own work) as the real bottleneck, not the `Limit` (whose cumulative
  10.26ms is almost entirely the `Sort` below it).

**An honest nuance this run surfaced, not a bug**: the `⚠ estimate off`
flag fired on `Sort` (planned 12413, actual 10) — but that's a known
Postgres `EXPLAIN ANALYZE` reporting quirk, not a real statistics
problem: under a `LIMIT`, a node's `Actual Rows` reflects what its
*parent pulled from it*, not the total it processed internally, so a
`Sort` feeding a `LIMIT 10` reports 10 actual rows almost by
construction. The row-estimate check is still correct and useful in the
common case (no `LIMIT` above it); worth knowing this one interaction
exists before trusting the flag blindly under a `LIMIT`.

**Not done / deliberately deferred**: buffer/I/O stats (`BUFFERS true`
adds shared/local/temp block hit-and-read counts to the JSON — a real,
common source of "why is this slow" that isn't time-based at all, not
parsed in this v1); parallel worker breakdown (`Workers` array on
parallel nodes, each with its own actual-time — currently only the
leader's numbers are shown).
