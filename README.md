# arc-swap-210-repro

Status: T1 landed (standalone crate + hammer); T2–T4 results below as they land.

Standalone reproducer attempt for
[vorner/arc-swap#210](https://github.com/vorner/arc-swap/issues/210): a `Guard`
from `ArcSwap::load()` observing a version that went **backwards** through a
single-writer monotonic protocol, or a **coherent foreign snapshot** (a complete
`Inner` matching the *other* instance's fill pattern).

## Shape (mirrors the original tests exactly)

- `Slot` = one `ArcSwap<Inner>`, `Inner { version: u64, a: Vec<f32> (32), b: Vec<f32> (64) }`.
- `update()` = `load().version + 1` → `store(Arc::new(..))`. **One writer per instance.**
- Two tests in **one libtest binary**, run with `--test-threads=2`:
  - `concurrent_lora_update_read` — 1 000 updates, fill `a == b == i`.
  - `concurrent_lora_no_torn_read` — 100 000 updates, fill `a == i, b == 2i`
    (so `a == b` there can only be the sibling's data).
- Each reader holds a `Guard` across its asserts. Failure messages are tagged
  `MYSTERY` (backwards / torn / foreign) or `MUNDANE` (reader starved by the writer).
- arc-swap pinned to `=1.9.1`, default (hybrid) strategy, **system allocator**
  (`--features tracking_alloc` re-adds the thin TLS-counting `System` wrapper
  the original binary carried — a bisect knob, off by default).

## Run

```sh
cargo test --lib -- --test-threads=2           # one shot
scripts/hammer.sh -n 300                       # loop under self-generated load (./load)
scripts/hammer.sh -n 300 -l none               # control: no load
scripts/hammer.sh -n 300 -l /path/to/big/ws    # use any cargo workspace as load
scripts/hammer.sh -n 300 -f tracking_alloc     # bisect knob
```

Load = two loops of **fresh-target** cargo builds (`check` and `test --no-run`)
of `./load` (a pure-Rust dependency graph that never touches arc-swap) running
concurrently with the hammer. Every iteration re-invokes cargo — the recipe the
original fires came from. Results land in `hammer-out/hammer.log`, one
`fire-<iter>.log` per failing iteration.

Miri (reduced iterations under `cfg(miri)`):

```sh
cargo +nightly miri test --lib -- --test-threads=2
```

## Results

See [RESULTS.md](RESULTS.md).
