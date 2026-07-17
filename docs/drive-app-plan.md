# Drive app plan — "The Compound"

Status: **plan ratified in ideation with the maintainer, 2026-07-18** —
build queues behind BH-4 (#973). Companions: `docs/effects-spec.md`
§12–§13, `docs/flow-suspension-spec.md` §10, charters #905 (FSMs),
#901 (lambdas), icebox #827 (vec types).

## 1. Purpose (three jobs, one app)

1. **The drive-it gate.** Human hands on the BH-2..4 API surface —
   schedulers have UX that harness data cannot prove. The maintainer
   drives; findings become issues.
2. **The standing rig.** Committed to the repo; every future BH/FS
   slice grows a corner here (BH-5 prefetch, FS-3 `await`, BH-v2).
3. **The evidence machine.** Code-only entity scripting in today's
   ink, with a **friction journal**: every awkwardness is filed as an
   issue labeled `drive-it` and cross-referenced to the charter it
   feeds — #905 (knots-as-states friction), #901 (schedule-as-data
   composition limits), #827 (ink-side vector math). The corpus-lanes
   → lambda-dossier pattern, applied to entity scripting.

## 2. Game shape

Stealth-lite with a round structure: sneak the compound, don't trip
the alarm; rounds punctuated by a shop intermission. Top-down, colored
shapes + gizmos, bevy_ui text. No art dependencies.

**The thesis stress-test (maintainer's framing):** most entities are
NOT doing dialogue. The demo deliberately over-weights code-only
behavior to find where "only knots are async" (tunnels await,
functions never) gets awkward in a pure-behavior world.

## 3. The cast — entity layer

| Entity | Behavior (code-only unless noted) | Seam + evidence |
|---|---|---|
| **Guards** (dozens) | patrol → suspicious → alert → search → return | THE statechart archetype (#905 evidence); `wake_when(player_in_cone)`; `#@local` suspicion; `-> goto(post) ->` tunnel-as-async-fn test |
| **Cameras** | rotate, detect, raise alarm | pure logic loop + `#[derive(BrinkCommand)]` commands |
| **Doors/switches** | locked until switch flipped | minimal reactive entity — await on a global/local |
| **Alarm system** | escalation level all read | World-policy writes; frame-start consistency made visible |
| **Reinforcement spawner** | dormant until alarm ≥ 2, then waves | **dormant spawn** (BH-4) + dynamic flow spawning |
| **Rats** (hundreds–thousands) | scurry loops | batch/parallel throughput spectacle |
| **Quartermaster** (1) | real dialogue + choices + bounty quest | the serial seam — deliberately the exception |

Guard barks are one-line garnish (tags exercised, not the point).

## 4. Systems layer

- **Game clock**: a flow writing `game_hour` (World policy).
  **Quantized-writes pattern**: write only on change, never per frame
  — a per-frame clock write dirties every schedule condition every
  frame. The demo measures both (BH-B change-pressure axis) and the
  result becomes a book authoring guideline.
- **NPC schedules**, built BOTH ways side-by-side as an honest
  comparison:
  1. *Transition-loop*: knot per phase, `await game_hour >= H`
     between — statechart-ish (#905 evidence).
  2. *Data-driven interpreter*: schedule as a map of hour→post, one
     generic follower knot — expected to hit fn-value composition
     limits (#901 evidence).
  Shift changes compose schedule + `goto` tunnel + statechart.
- **Round manager**: intermission (shop) → wave N → cleared →
  intermission. The top-level FSM evidence case; tunnels into the
  shop.

## 5. Commerce & RPG layer

- **Shop = weave-as-menu, not dialogue**: `* {gold >= 50} [Buy
  medkit — 50g]` is affordability-gated UI for free; `+` sticky vs
  `*` once-only is restocking semantics.
- **Items/inventory**: `STRUCT Item = #{name, price, heal}`, map of
  id→Item, gold global — the T1 value surface under `types = strict`.
- **Combat math as pure ink functions** called from the engine via
  `begin_function_eval`: bevy detects the hit, ink's
  `resolve_hit(attacker, defender)` returns damage. The engine→ink
  rules-engine seam; effects rows prove purity at compile time.
- **XP + bounty quest** from the quartermaster ("disable 3 cameras"):
  host reports events, ink tracks state.

## 6. Division of labor — deliberate

**Ink decides, bevy executes**: ink picks states/targets; steering,
animation, collision stay host-side. ONE deliberate exception: a
single archetype does its movement math in ink (x/y struct fields) —
the friction it generates is #827's evidence.

## 7. Instrumentation & interaction

- HUD: live `BrinkBatchReport` (stepped/awaiting/parked/errored),
  frame p50, current mode.
- Keybinds: `1/2/3` serial→batch→parallel LIVE (the determinism law
  as a demo moment: nothing visible may change but frame time); `+`
  spawn 1,000 rats; `F5/F9` save/load (drives SaveState today,
  parked-flow persistence the day FS-3 lands).
- **One deliberate ugly corner**: an NPC whose manifest capability is
  missing under one marker — showcasing the #912 hard load error, the
  errored counter (#914 lineage), and the rehydration report on
  load-after-recompile. Failure surfaces are part of the demo.

## 8. Where it lives + sequencing

- `crates/bevy-brink/examples/compound/` (main.rs + a few modules,
  `assets/*.ink`: compound.ink, schedules.ink, shop.ink). Example-dir
  form forces API-surface honesty: if the example needs scaffolding,
  the API is too raw — that finding IS the gate.
- Build after BH-4 (#973) lands so the first drive covers the whole
  spine including wakes. v1 content budget: ~4 guard posts, ~3 shop
  items, 1 bounty.
- Friction journal: `drive-it` label (create at build time); one
  issue per finding, cross-ref the charter it feeds.

## 9. Phasing — RULED 2026-07-18 (pure-bevy first, migrate to ink)

- **Phase 0 — pure-bevy Compound** (dispatchable immediately, no
  brink APIs): all behavior in Rust — guards as enum FSMs, cameras,
  doors, alarm, spawner, rats, rounds + shop (bevy_ui), stats/combat.
  The **control group**: baseline LOC, authoring shape, and
  µs-per-entity numbers. Ends in an early low-stakes drive session
  to tune game feel before any API is on the line.
- **Phase 1 — entity-by-entity migration** (after BH-4 #973 +
  FS-3w #978): each entity's move to ink is one friction-journal
  entry + a measured delta (LOC, feel, cost). The same guard written
  as Rust-enum FSM → ink knots → (future) #905 FSM syntax is the
  charter's three-column evidence.
- **Mixed world is the END STATE, not a phase**: both archetypes
  stay spawnable side-by-side (rust-guards vs ink-guards keybinds) —
  500 of each in one frame gives adopters the number they always
  ask for (scripting-layer cost over native), and the demo doubles
  as the incremental-adoption story real bevy games follow.
- **CI constraint**: the demo must NOT inflate required CI (full
  bevy rendering stack). It lives as a **workspace-excluded crate**
  (`demos/compound/`, path-deps on the brink crates) rather than an
  example dir — built locally, not by --all-targets/--all-features
  jobs. Bit-rot guard: an optional manual/scheduled CI job may come
  later. (Amends §8's examples/ placement — the API-honesty check
  moves to Phase 1's migration diffs, which are better evidence
  anyway.)
