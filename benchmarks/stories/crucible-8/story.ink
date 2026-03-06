// ══════════════════════════════════════════════════════════════════════════════
// THE CRUCIBLE — Runtime Torture Test
// ══════════════════════════════════════════════════════════════════════════════
// DEPTH = 8 → ~150k–200k opcodes
// 5 phases: arithmetic, lists, threads, sequences, tunnels
// 28 choice points (all select index 0)

EXTERNAL crucible_external(x)

VAR DEPTH = 8

VAR checksum1 = 0
VAR checksum2 = 0
VAR checksum3 = 0
VAR checksum4 = 0
VAR checksum5 = 0

LIST Elements = fire, water, earth, air, lightning, ice, shadow, light
LIST Metals = copper, tin, iron, silver, gold, platinum, mithril, adamant

VAR forge_state = ()
VAR alloy_state = ()

VAR arena_round = 0
VAR tunnel_depth = 0
VAR tunnel_var = -> tunnel_relay

~ SEED_RANDOM(42)

# crucible
# torture_test
# depth_8

The Crucible awaits. {DEPTH} trials in each domain.
-> phase1_gate

// ═════════════════════════════════════════════════════════════════════════════
// PHASE 1: Arithmetic Trials
// Stress: call/return, deep recursion, all arithmetic ops, float ops
// ═════════════════════════════════════════════════════════════════════════════

=== phase1_gate ===
Phase 1: Arithmetic Trials
~ temp i = 0
- (p1_loop)
{
    - i >= DEPTH:
        -> p1_done
}
~ temp f = fib(i + 12)
~ temp g = gcd(f, (i + 1) * 7)
~ temp p = pow_mod(f, i + 2, 97)
~ checksum1 = checksum1 + f + g + p
Trial {i + 1}: fib={f} gcd={g} pow={p}
~ i = i + 1
-> p1_loop
- (p1_done)
Phase 1 checksum: {checksum1}
+   [Enter the Forge] -> phase2_gate

=== function fib(n) ===
{
    - n <= 1:
        ~ return n
    - else:
        ~ return fib(n - 1) + fib(n - 2)
}

=== function gcd(a, b) ===
{
    - b == 0:
        ~ return a
    - else:
        ~ return gcd(b, a mod b)
}

=== function pow_mod(base, exp, m) ===
{
    - exp == 0:
        ~ return 1
    - exp mod 2 == 0:
        ~ temp half = pow_mod(base, exp / 2, m)
        ~ return (half * half) mod m
    - else:
        ~ return (base * pow_mod(base, exp - 1, m)) mod m
}

// ═════════════════════════════════════════════════════════════════════════════
// PHASE 2: Forge of Elements
// Stress: all list opcodes, ref params
// ═════════════════════════════════════════════════════════════════════════════

=== phase2_gate ===
Phase 2: Forge of Elements
~ forge_state = LIST_ALL(Elements)
~ alloy_state = ()
~ temp j = 0
- (p2_loop)
{
    - j >= DEPTH * 3:
        -> p2_sort
}
~ list_cascade(j)
~ j = j + 1
-> p2_loop
- (p2_sort)
~ list_sort_step(forge_state, checksum2)
Forge checksum: {checksum2}
+   [Enter the Arena] -> arena_loop

=== function list_cascade(iter) ===
~ temp all_e = LIST_ALL(Elements)
~ temp all_m = LIST_ALL(Metals)
~ temp count = LIST_COUNT(forge_state)
~ temp lo = LIST_MIN(all_e)
~ temp hi = LIST_MAX(all_e)
~ temp rng = LIST_RANGE(all_m, 2, 5)
~ temp inv = LIST_INVERT(forge_state)
~ temp val = LIST_VALUE(lo)
~ temp from_int_item = Elements(val)
~ temp rnd_elem = LIST_RANDOM(all_e)
// Rotate: extract min from forge, add to alloy
{
    - count > 0:
        ~ temp cur_min = LIST_MIN(forge_state)
        ~ alloy_state += cur_min
        ~ forge_state -= cur_min
    - else:
        ~ forge_state = all_e
}
// Contains / not-contains checks
{
    - forge_state ? fire:
        ~ checksum2 = checksum2 + 1
}
{
    - forge_state !? shadow:
        ~ checksum2 = checksum2 + 2
}
~ checksum2 = checksum2 + val + LIST_COUNT(rng) + LIST_VALUE(from_int_item) + LIST_VALUE(rnd_elem)

=== function list_sort_step(lst, ref out) ===
// Recursive min-extraction sort via ref param
{
    - LIST_COUNT(lst) > 0:
        ~ temp m = LIST_MIN(lst)
        ~ out = out + LIST_VALUE(m)
        ~ list_sort_step(lst - m, out)
}

// ═════════════════════════════════════════════════════════════════════════════
// PHASE 3: Arena of Threads
// Stress: thread forks, snapshots, RANDOM, TURNS_SINCE, CHOICE_COUNT
// ═════════════════════════════════════════════════════════════════════════════

=== arena_loop ===
Phase 3: Arena of Threads
- (arena_top)
{
    - arena_round >= DEPTH * 3:
        -> phase3_choice
}
~ arena_round = arena_round + 1
Round {arena_round}:
<- combatant(1)
<- combatant(2)
<- combatant(3)
<- combatant(4)
<- combatant(5)
<- combatant(6)
-> DONE

= combatant(id)
    ~ temp dmg = RANDOM(1, 10)
    ~ temp turns = TURNS_SINCE(-> arena_loop)
    ~ checksum3 = checksum3 + dmg
    +   [Fighter {id} strikes for {dmg}]
        Hit! Fighter {id} dealt {dmg} (turn {turns}, choices: {CHOICE_COUNT()})
    -> arena_top

=== phase3_choice ===
Arena checksum: {checksum3}
+   [Enter the Scriptorium] -> phase4_gate

// ═════════════════════════════════════════════════════════════════════════════
// PHASE 4: Scriptorium
// Stress: all 4 sequence types, nested sequences, type coercion in output
// ═════════════════════════════════════════════════════════════════════════════

=== phase4_gate ===
Phase 4: Scriptorium
~ temp k = 0
- (p4_loop)
{
    - k >= DEPTH * 4:
        -> p4_done
}
Verse {k + 1}: {seq_stopping()}, {seq_cycle()}, {seq_once()}, {seq_shuffle()}
Nested: {nested_seq()}
Voice: The narrator {narrator_voice()}.
Types: int={k + 1} float={k * 1.5} bool={k > 0} list={LIST_MIN(Elements)} str={"ink"}
~ checksum4 = checksum4 + k + 1
~ k = k + 1
-> p4_loop
- (p4_done)
Scriptorium checksum: {checksum4}
+   [Descend to the Tunnels] -> phase5_gate

=== function seq_stopping() ===
{ stopping:
    - alpha
    - beta
    - gamma
    - omega
}

=== function seq_cycle() ===
{ cycle:
    - red
    - blue
    - green
}

=== function seq_once() ===
{ once:
    - first
    - second
    - third
}

=== function seq_shuffle() ===
{ shuffle:
    - ace
    - king
    - queen
    - jack
}

=== function narrator_voice() ===
{ stopping:
    - speaks softly
    - whispers
    - booms
    - is silent
}

=== function nested_seq() ===
{ stopping:
    - The {inner_cycle()} begins
    - The {inner_once()} watch
    - eternal rest
}

=== function inner_cycle() ===
{ cycle:
    - day
    - night
}

=== function inner_once() ===
{ once:
    - first
    - second
    - third
}

// ═════════════════════════════════════════════════════════════════════════════
// PHASE 5: Deep Tunnels
// Stress: tunnel depth, alternating normal/variable tunnel dispatch,
//         external function with ink fallback
// ═════════════════════════════════════════════════════════════════════════════

=== phase5_gate ===
Phase 5: Deep Tunnels
~ tunnel_depth = 0
~ tunnel_var = -> tunnel_relay
-> tunnel_descend ->
Tunnel checksum: {checksum5}
-> finale

=== tunnel_descend ===
{
    - tunnel_depth >= DEPTH:
        ->->
}
~ temp d = tunnel_depth
~ tunnel_depth = tunnel_depth + 1
~ checksum5 = checksum5 + d + 1
{
    - d mod 2 == 0:
        Tunnel depth {d}: normal
        -> tunnel_relay ->
        ->->
}
Tunnel depth {d}: variable
-> tunnel_var ->
->->

=== tunnel_relay ===
-> tunnel_descend ->
->->

// ── External function with ink fallback body ────────────────────────────────

=== function crucible_external(x) ===
~ return x * 2

// ═════════════════════════════════════════════════════════════════════════════
// FINALE
// ═════════════════════════════════════════════════════════════════════════════

=== finale ===
~ temp ext_result = crucible_external(checksum1)
External result: {ext_result}
~ checksum5 = checksum5 + ext_result
Final checksums:
Phase 1 (Arithmetic): {checksum1}
Phase 2 (Lists): {checksum2}
Phase 3 (Threads): {checksum3}
Phase 4 (Sequences): {checksum4}
Phase 5 (Tunnels): {checksum5}
Total: {checksum1 + checksum2 + checksum3 + checksum4 + checksum5}
The Crucible is complete.
-> END
