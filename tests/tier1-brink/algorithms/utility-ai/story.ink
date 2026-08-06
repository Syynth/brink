// ALGORITHMS CORPUS — AI-decision lane (issue #822)
// Utility AI: score every candidate action by a weighted average of named
// "considerations" (health, enemy distance, ammo), pick the highest —
// smoother than an FSM's hard state boundaries, simpler than full GOAP
// planning. Three scenarios (deterministic world-state, no randomness)
// each land on a different winning action.
//
// TYPES POLICY: gradual (default). `Consideration.score_fn: fn(): float`
// is the one shape worth calling out — a struct field typed as a function
// value works the same way under gradual as it does in behavior-tree's
// `BTNode`, so this file reuses that pattern rather than re-deriving it.
//
// ERGONOMICS-FINDINGS:
// - This is the CLEAN case the catalog predicted ("straightforward
//   map/fold over a list of scoring functions... good candidate for
//   proving out fn-values-as-first-class-values without much else
//   getting in the way") — and it held up exactly that way. Four
//   DIFFERENT named functions (`score_health_low`, `score_enemy_close`,
//   `score_ammo_available`, `score_baseline`), each with a different
//   body, get stored side by side in one `Array<Consideration>` and
//   called through the exact same line, `call(c.score_fn)`, inside
//   `score_action`'s loop — the loop has no idea which specific function
//   it's about to invoke, and doesn't need to. That is real polymorphism
//   over first-class function values, achieved with zero bound
//   arguments and zero partial application: contrast with
//   behavior-tree/story.ink next door, where the fn-value story is about
//   PARAMETERIZING one shared function differently per leaf; here it's
//   about calling DIFFERENT functions uniformly. Both are genuine
//   fn-value use cases and this corpus now has a clean example of each.
// - `call(c.score_fn)`, not `c.score_fn()` — see behavior-tree/story.ink
//   finding #3 for the full writeup of why: the direct form silently
//   fails to invoke the function (no diagnostic, wrong result) rather
//   than being rejected outright, so `call(...)` is used unconditionally
//   here too.
// - No compensation-problem handling. The classic utility-AI pitfall
//   (Dave Mark & Kevin Dill's GDC talk, cited per issue #822's catalog,
//   describes the technique without transcribing slides) is that a
//   weighted AVERAGE lets one high consideration compensate for another
//   being catastrophically low — e.g. full ammo alone can drag `attack`'s
//   score up even with the enemy nowhere close. This file uses a plain
//   weighted average on purpose (it is what "weighted scoring" in the
//   issue's own wording names), and picks scenario numbers that avoid
//   the pitfall rather than hide it — see the scenario comments below.
//   A production utility AI would reach for multiplicative combination
//   or a floor/veto consideration instead; that's out of scope for what
//   this port is demonstrating and is flagged here rather than silently
//   built in.
// - Float-printing noise: see value-noise-field/story.ink's header for
//   the full finding (`f32` rounding surfaces in `string()`/interpolation,
//   e.g. `0.7` prints as `0.70000005`) — this file's golden transcript
//   below carries the same noise. Still byte-identical every run
//   (deterministic `f32` arithmetic), just cosmetically busy; not
//   re-derived here, just confirmed to reproduce.
// - Same "collection sigil literals can't span multiple lines" gotcha
//   bfs-grid-path/dijkstra-grid already document — the `options`
//   array-of-structs-of-structs literal below is collapsed onto one
//   (long) line for that reason, same as every other multi-element
//   literal in this corpus.
// - `weight_total` is computed by summing each `Consideration`'s weight
//   in the same loop that sums the weighted scores, rather than assumed
//   to be `1.0` — this avoids a silent wrong-answer if a future edit adds
//   a consideration and forgets to keep the weights normalized; the loop
//   in `score_action` is the single place that invariant is enforced.

CONST MAX_DISTANCE = 10
CONST MAX_AMMO = 3

VAR health = 0
VAR enemy_distance = 0
VAR ammo = 0

VAR summaries = #[]

STRUCT Consideration = #{
    weight: float,
    score_fn: fn(): float,
}

STRUCT ActionOption = #{
    name: string,
    considerations: Array<Consideration>,
}

~ {
    temp options = #[ActionOption#{name: "heal", considerations: #[Consideration#{weight: 1.0, score_fn: #fn(score_health_low)}]}, ActionOption#{name: "attack", considerations: #[Consideration#{weight: 0.6, score_fn: #fn(score_enemy_close)}, Consideration#{weight: 0.4, score_fn: #fn(score_ammo_available)}]}, ActionOption#{name: "retreat", considerations: #[Consideration#{weight: 0.7, score_fn: #fn(score_health_low)}, Consideration#{weight: 0.3, score_fn: #fn(score_enemy_close)}]}, ActionOption#{name: "patrol", considerations: #[Consideration#{weight: 1.0, score_fn: #fn(score_baseline)}]}]

    // Scenario A: near-full health, enemy far away, dry on ammo — every
    // consideration reads low, so only the constant `patrol` baseline has
    // anything to say. Nothing here can compensate `attack` upward: both
    // of its considerations (`enemy_close`, `ammo_available`) are ~0.
    health = 95
    enemy_distance = 10
    ammo = 0
    push(summaries, run_scenario(options, "A (healthy, enemy far, no ammo)"))

    // Scenario B: badly hurt, enemy adjacent, dry on ammo — `heal` and
    // `retreat` both score high, and `retreat` edges it out because it
    // also weighs the close enemy; `attack` can't compensate for zero
    // ammo just by the enemy being close (its ammo term is still 0).
    health = 15
    enemy_distance = 1
    ammo = 0
    push(summaries, run_scenario(options, "B (hurt, enemy adjacent, no ammo)"))

    // Scenario C: comfortably healthy, enemy close, fully stocked — this
    // is the one scenario where `attack`'s two considerations are BOTH
    // genuinely high at once, so it wins outright rather than by
    // compensation.
    health = 60
    enemy_distance = 2
    ammo = 3
    push(summaries, run_scenario(options, "C (mid health, enemy close, full ammo)"))
}

{summaries[0]}
{summaries[1]}
{summaries[2]}
-> END

=== function score_health_low(): float ===
~ return (100.0 - float(health)) / 100.0

=== function score_enemy_close(): float ===
~ return 1.0 - (float(enemy_distance) / float(MAX_DISTANCE))

=== function score_ammo_available(): float ===
~ return float(ammo) / float(MAX_AMMO)

=== function score_baseline(): float ===
~ return 0.2

// Map/fold over `opt.considerations`: a weighted average, not assumed
// pre-normalized (see the header's `weight_total` note).
=== function score_action(opt: ActionOption): float ===
~ {
    temp i = 0
    temp weighted_sum = 0.0
    temp weight_total = 0.0
    while i < len(opt.considerations) {
        temp c = opt.considerations[i]
        temp s = call(c.score_fn)
        weighted_sum = weighted_sum + c.weight * s
        weight_total = weight_total + c.weight
        i = i + 1
    }
    return weighted_sum / weight_total
}

=== function best_action_index(options: Array<ActionOption>): int ===
~ {
    temp best_idx = 0
    temp best_score = score_action(options[0])
    temp i = 1
    while i < len(options) {
        temp s = score_action(options[i])
        if s > best_score {
            best_score = s
            best_idx = i
        }
        i = i + 1
    }
    return best_idx
}

=== function run_scenario(options: Array<ActionOption>, label: string): string ===
~ {
    temp line = label + ": "
    temp i = 0
    while i < len(options) {
        temp opt = options[i]
        temp s = score_action(opt)
        line = line + opt.name + "=" + string(s) + " "
        i = i + 1
    }
    temp winner = options[best_action_index(options)]
    line = line + "-> winner=" + winner.name
    return line
}
