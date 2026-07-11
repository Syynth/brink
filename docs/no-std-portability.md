# `no_std` + `alloc` portability (#434)

## Goal

`brink-runtime` and `brink-format` — the promoted, embeddable runtime core —
should build without the standard library (`core` + `alloc` only), so the
core is maximally portable/embeddable (bare-metal hosts, WASM without a
`wasm32-unknown-unknown` std shim, etc.). This is a portability goal, not a
current product requirement: nothing in the workspace ships a `no_std`
build today, and the oracle/runtime never runs one. The default build (the
`std` feature, on by default) is unchanged — byte-for-byte, in every case
that matters — by everything below.

## What landed

Both crates gained a `std` feature (**default-enabled**) and:

```rust
#![cfg_attr(not(feature = "std"), no_std)]
extern crate alloc;
```

With `std` on (the default — every existing consumer, every test, the
oracle gate), nothing changed:

- `crates/brink-runtime/src/collections.rs` defines `Map`, aliased to
  `std::collections::HashMap` under `std` — the literal type the runtime
  always used. It only becomes `alloc::collections::BTreeMap` when `std` is
  off. Every map key already used in this crate (`DefinitionId`, `String`,
  `u32`) is `Ord`, so the `no_std` fallback is a drop-in — and it also
  satisfies the project's "determinism matters" rule (no more
  hash-iteration-order footguns) for any future `no_std` build.
- `Arc`/`RefCell`/`PhantomData`/`VecDeque`/`BTreeMap`/`fmt`/`ops::Range`
  moved from `std::` to `core::`/`alloc::` paths — these are the same types
  either way (`std::sync::Arc` *is* `alloc::sync::Arc`), so this is a
  no-op under `std`.
- `serde` and `thiserror` are pulled in with `default-features = false` and
  the crate's own `std` feature re-enables `serde/std` /
  `brink-format/std` — so feature resolution is identical to before when
  `std` is on.

With `std` off, `cargo check -p brink-format --no-default-features` and
`cargo check -p brink-runtime --no-default-features` both pass.

## The one real behavioral fork: `FLOOR()` / `CEILING()` / `POW()`

`core` has no transcendental/rounding math (`f32::floor`, `f32::ceil`,
`f32::powf`) — those need a `libm`-backed implementation, which is
std-only in the standard library (embedded targets don't all have one).
Everything else `Value`'s arithmetic touches (`abs`, `min`, `max`, integer
ops) is a `core` intrinsic and needed no fork.

Rather than pull in a new `libm` dependency unasked — that's an
architectural call for a future pass, not something to slip into a
portability-groundwork change — `Opcode::Floor`/`Opcode::Ceiling` (in
`vm.rs`) and `BinaryOp::Pow` (in `value_ops.rs`) are `#[cfg]`-forked:

- `std`: identical to before (`f.floor()`/`f.ceil()`/`a.powf(b)`).
- `no_std`: `RuntimeError::Unimplemented(..)` — an honest, typed error
  instead of a silent wrong answer or a panic (`panic!`/`unwrap`/`expect`
  stay denied in production code either way).

This is the "coherent subset" the tracking issue anticipated: the crate is
`no_std`-checkable today, with one narrow, explicitly-flagged gap (ink's
`FLOOR()`/`CEILING()`/`POW()` under a hypothetical `no_std` build) that a
future pass can close by wiring in a `libm`-family crate — a decision for
the user, not something to default into silently.

## What's still open (deferred, not done here)

- **No actual `no_std` consumer yet.** This lands the capability
  (`cargo check --no-default-features` is the proof), not a shipped
  `no_std` build target. `bevy-brink` and every other consumer keep using
  the default `std` feature.
- **`libm` for `FLOOR()`/`CEILING()`/`POW()` under `no_std`** — see above.
  A future pass should raise this as its own design decision (which crate,
  license, whether it's optional) rather than assume `libm` specifically.
- **Locks.** The issue also flagged `std::sync::Mutex`/`RwLock` →
  `alloc`/`spin`. Neither crate uses a lock anywhere today (checked: zero
  `Mutex`/`RwLock` sites), consistent with the scoped-flow-state
  single-owner + step-scoped `&mut` ownership model the issue's
  "constraint on current work" section describes. Nothing to convert.
- **`inkt`/`inkt-write` (`brink-format`)** — the `.inkt` text format used
  by the intl pipeline — stay `std`-oriented (`pest` is a std-only parser
  generator) and are independent of the `std` feature. Not part of the
  no_std surface; not touched here.
- **`content_hash`** (`brink-format::definition`) keeps its exact
  `std::collections::hash_map::DefaultHasher` output under `std` (nothing
  anywhere compares hashes across a `std` and a `no_std` build, so this
  was a free, zero-risk choice) and falls back to a small FNV-1a
  implementation under `no_std`.
- **Downstream crates** (`brink-compiler`, `brink-analyzer`,
  `brink-codegen-inkb`, `brink-ir`, `brink-converter`, `bevy-brink`, …)
  were not touched and are not `no_std`. Only the two crates named in the
  issue (the promoted runtime core) were in scope.
