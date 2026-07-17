# Parallelism curve — BH-3 parallel Step vs serial batch (PROVISIONAL / in-wave)

BH-3 (#927) night-shift data rule: a scenario-harness parallelism-curve run for
the parallel Step phase (`advance_batch_parallel`), measured against the
serial batch driver over the same one-batch-turn workload.

**These numbers are PROVISIONAL / in-wave** — captured on the build machine
while the rest of the suite shares the process, not in a quiet window. The
canonical denominators are `serial-driver.csv` (the multi-frame headless serial
per-flow runner, issue #900) and `batch-serial-driver.csv` (the quiet-window
BH-2 batch-serial baseline, #929); the canonical *parallel* numbers are
re-measured in a quiet window separately, per the 2026-07-16 baseline-first
ruling. This table exists to bound the parallel path's cost against the serial
one *now*, not as a throughput target.

Both rows drive one batch turn over N flows of a story that writes one global
and ends (`VAR g = 0 … ~ g = g + 1 -> END`), invoking the real production entry
points directly:

- serial: `advance_batch::<M>` (main-thread Step loop)
- parallel: `advance_batch_parallel::<M>` (`ComputeTaskPool` + `UnsafeWorldCell`)

Regenerate:

```sh
cargo test -p bevy-brink --release batch_serial_scenario_numbers   -- --ignored --nocapture
cargo test -p bevy-brink --release batch_parallel_scenario_numbers -- --ignored --nocapture
```

| flows | serial turn (ms) | parallel turn (ms) | serial µs/flow | parallel µs/flow | speedup |
|------:|-----------------:|-------------------:|---------------:|-----------------:|--------:|
| 1     | 0.321            | 0.034              | 321.29         | 33.92            | 9.47×   |
| 64    | 0.265            | 0.165              | 4.15           | 2.58             | 1.61×   |
| 512   | 1.089            | 0.743              | 2.13           | 1.45             | 1.47×   |
| 4096  | 6.538            | 6.442              | 1.60           | 1.57             | 1.02×   |

Reading it honestly: the parallel Step wins clearly at low-to-mid flow counts
and converges toward parity at 4096, where the per-flow **frame-start clone**
(BH-2's known serial cost — §12.2's "borrow, don't copy" is the later
optimization) and the serial flow-id-ordered Apply dominate the turn. The curve
is the parallel-vs-serial denominator for that follow-up; it is *not* a claim
that the clone-based Step is throughput-optimal.
