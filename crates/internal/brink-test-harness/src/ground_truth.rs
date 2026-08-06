//! Corpus differential oracle (issue #898) — per-lane reference
//! implementations that share zero code with `brink-runtime`, used by
//! `tests/algorithms_ground_truth.rs` to cross-validate the algorithms
//! corpus (`tests/tier1-brink/algorithms/`, issue #822) against known-good
//! Rust ground truth rather than only against a hand-verified golden
//! transcript.
//!
//! Golden transcripts (`expected.txt`) prove *determinism* — a stably-wrong
//! quicksort passes them just as well as a correct one. This module is the
//! other half: independent implementations (`slice::sort`, direct DP
//! recurrences, a hand-rolled+unit-tested Dijkstra) that must agree with
//! what the brink VM actually computes. Anything perturbing *results* while
//! keeping the transcript internally stable (a VM arithmetic/collection/COW
//! bug) is caught here, not there — the brink-dialect analogue of the C#
//! oracle (`docs/oracle-spec.md`; `tests/tier{1,2,3}` for the ink-compat
//! side), same philosophy as the #870 effects ground-truth harness
//! (`tests/t2_ground_truth_effects.rs`).
//!
//! **Lane coverage (state per lane what is and isn't proven — issue #898's
//! own explicit ask):**
//! - `sort`: [`sort::reference_sort`] wraps `slice::sort` — exact multiset
//!   equality is proven for every sorting-lane case, fixed-input AND
//!   property mode (`tests/algorithms_ground_truth.rs`'s `proptest`-free
//!   bounded loop over [`pcg`]-seeded random arrays).
//! - `dp`: direct (non-memoized) recurrences for edit distance, 0/1
//!   knapsack, LCS (length + recovered subsequence text), and fibonacci —
//!   exact equality proven against each case's one fixed corpus input.
//!   Property mode is NOT wired up for this lane yet (tracked as follow-up;
//!   see the harness test file's module doc).
//! - `graph`: a hand-rolled, unit-tested Dijkstra over the shared weighted
//!   grid — exact path-COST equality proven for `dijkstra-grid` and
//!   `astar-grid`'s one fixed shared grid/start/goal. Does not attempt to
//!   validate the recovered path *shape* (only its cost) since more than
//!   one minimum-cost path can exist; does not cover A*'s node-visitation
//!   count claim (already exercised structurally by
//!   `algorithms_corpus.rs::astar_matches_dijkstra_cost_on_the_shared_
//!   weighted_grid`).
//! - `pcg`: a bit-for-bit Rust port of `tests/tier1-brink/algorithms/
//!   pcg-rng/pcg.ink`'s state-transition/output functions, using the same
//!   wrapping 32-bit arithmetic brink's `int` silently performs. Used ONLY
//!   to generate seeded property-mode inputs on the Rust side (so
//!   "randomized seeded input" in this harness and in the ink corpus share
//!   one seed-to-sequence mapping); this module makes no claim about the
//!   *statistical quality* of PCG-style output (that is `pcg-rng/story.
//!   ink`'s own documented ceiling — see that file's header).
//!
//! **Explicitly NOT covered by this module (scope of issue #898 left for
//! follow-up, not silently dropped):** the randomness lane's statistical
//! bounds (chi-squared over `weighted-loot-table`'s draw distribution) and
//! the procgen lane's invariant checks (BSP connectivity, cave open-ratio
//! bounds) are a different *kind* of oracle — bound-satisfaction, not exact
//! equality — and are left to a follow-up so this first slice (the issue's
//! own "start with the crisp lanes" instruction) lands as a reviewable,
//! honestly-scoped unit.

/// Sorting lane — `slice::sort` is the known-good reference every
/// sorting-lane corpus case (`quicksort`, `mergesort`, `insertion-sort`) is
/// cross-validated against. Multiset/order equality only (no stability
/// claim needed — every sorting-lane case sorts bare `int`s with no
/// secondary payload, so two implementations agreeing on final order is
/// exactly what "correctly sorted" means here).
pub mod sort {
    /// Sort `input` with Rust's `slice::sort` (introsort — unrelated to any
    /// partition/merge/insertion strategy brink's ports use) and return the
    /// result. The reference side of every sorting-lane comparison.
    #[must_use]
    pub fn reference_sort(input: &[i64]) -> Vec<i64> {
        let mut out = input.to_vec();
        out.sort_unstable();
        out
    }
}

/// DP lane — direct (bottom-up, non-memoized) recurrences. Each fn here is
/// intentionally the "textbook" recurrence, not a port of the brink file's
/// own memoization strategy — the point is an implementation that shares no
/// code or structure with what's under test.
pub mod dp {
    /// 0/1 knapsack: maximum total value of a subset of items (parallel
    /// `weights`/`values` slices) whose total weight fits within `capacity`.
    /// Classic `O(n * capacity)` bottom-up table.
    #[must_use]
    pub fn knapsack_01(weights: &[i64], values: &[i64], capacity: i64) -> i64 {
        assert_eq!(
            weights.len(),
            values.len(),
            "weights/values length mismatch"
        );
        let cap = usize::try_from(capacity).unwrap_or(0);
        let mut table = vec![0_i64; cap + 1];
        for (&w, &v) in weights.iter().zip(values.iter()) {
            let w = usize::try_from(w).unwrap_or(usize::MAX);
            if w > cap {
                continue;
            }
            // Iterate capacities downward — the 0/1 (not unbounded) knapsack's
            // standard space-optimized table update, each item considered once.
            for c in (w..=cap).rev() {
                table[c] = table[c].max(table[c - w] + v);
            }
        }
        table[cap]
    }

    /// Levenshtein edit distance between two token sequences (brink's
    /// corpus ports operate over `Array<string>` of single characters, not
    /// a native `string` type with indexing — accepting `&[&str]` here
    /// mirrors that shape exactly rather than assuming Rust `char`s).
    /// Classic bottom-up `O(len(a) * len(b))` table.
    #[must_use]
    pub fn edit_distance(a: &[&str], b: &[&str]) -> i64 {
        let (n, m) = (a.len(), b.len());
        let mut table = vec![vec![0_i64; m + 1]; n + 1];
        for (i, row) in table.iter_mut().enumerate() {
            row[0] = i64::try_from(i).unwrap_or(i64::MAX);
        }
        if let Some(first_row) = table.first_mut() {
            for (j, cell) in first_row.iter_mut().enumerate() {
                *cell = i64::try_from(j).unwrap_or(i64::MAX);
            }
        }
        for i in 1..=n {
            for j in 1..=m {
                table[i][j] = if a[i - 1] == b[j - 1] {
                    table[i - 1][j - 1]
                } else {
                    1 + table[i - 1][j]
                        .min(table[i][j - 1])
                        .min(table[i - 1][j - 1])
                };
            }
        }
        table[n][m]
    }

    /// Longest common subsequence: returns `(length, recovered_text)` where
    /// `recovered_text` is the subsequence's tokens concatenated with no
    /// separator — matching `longest-common-subsequence/story.ink`'s own
    /// `LCS: {lcsText}` print (its `lcsText` is built the same way, one
    /// `out = out + token` append per recovered character, no delimiter).
    /// Standard bottom-up table + backtrack.
    ///
    /// **`length` is a unique ground truth; `recovered_text` is NOT** — when
    /// more than one maximum-length common subsequence exists (true of this
    /// corpus's own fixture: `seqA=[A,G,C,A,T]`/`seqB=[G,A,C]` has both
    /// `"GA"` and `"AC"` as valid length-2 LCSes), which one comes out
    /// depends on the backtrack's tie-break rule, and this fn's tie-break
    /// (`>=` prefers stepping `a` back on a tie) has no reason to match
    /// `longest-common-subsequence/story.ink`'s own tie-break. Callers that
    /// want a ground-truth check should compare `length` only, the same way
    /// the graphs lane checks path COST but not path shape — see
    /// `tests/algorithms_ground_truth.rs`'s module doc.
    // Textbook DP notation (a/b sequences, n/m lengths, i/j indices) is
    // clearer here than invented longer names would be — allowed narrowly
    // on this one fn rather than renaming away from the standard notation.
    #[expect(clippy::many_single_char_names, reason = "textbook DP notation")]
    #[must_use]
    pub fn lcs(a: &[&str], b: &[&str]) -> (i64, String) {
        let (n, m) = (a.len(), b.len());
        let mut table = vec![vec![0_i64; m + 1]; n + 1];
        for i in 1..=n {
            for j in 1..=m {
                table[i][j] = if a[i - 1] == b[j - 1] {
                    table[i - 1][j - 1] + 1
                } else {
                    table[i - 1][j].max(table[i][j - 1])
                };
            }
        }
        let mut out = Vec::new();
        let (mut i, mut j) = (n, m);
        while i > 0 && j > 0 {
            if a[i - 1] == b[j - 1] {
                out.push(a[i - 1]);
                i -= 1;
                j -= 1;
            } else if table[i - 1][j] >= table[i][j - 1] {
                i -= 1;
            } else {
                j -= 1;
            }
        }
        out.reverse();
        (table[n][m], out.concat())
    }

    /// Direct fibonacci recurrence (a plain bottom-up loop — no memo map),
    /// `fibonacci(0) = 0`, `fibonacci(1) = 1`.
    #[must_use]
    pub fn fibonacci(n: u32) -> i64 {
        let (mut a, mut b) = (0_i64, 1_i64);
        for _ in 0..n {
            let next = a + b;
            a = b;
            b = next;
        }
        a
    }
}

/// Graphs lane — a hand-rolled, unit-tested Dijkstra (no new workspace
/// dependency: no `petgraph`, per issue #898's own "no new deps unless
/// workspace already has one"). Cross-validates path COST only (see module
/// doc for why not path shape).
pub mod graph {
    use std::collections::BTreeSet;

    /// `-1` is the corpus's impassable-cell sentinel
    /// (`dijkstra-grid`/`astar-grid`'s shared `story.ink` header); every
    /// other cell value is its entry movement cost.
    const IMPASSABLE: i64 = -1;

    /// Cheapest-path cost from `start` to `goal` on a weighted grid with
    /// obstacles, 4-directionally connected (matching `dijkstra-grid`/
    /// `astar-grid`'s own `dr`/`dc` neighbor order and cost model exactly:
    /// entering a cell costs that cell's grid value, `start` itself costs
    /// nothing). Returns `None` if unreachable. Plain array-backed
    /// Dijkstra — `O(rows*cols)` nodes, linear scan for the next unvisited
    /// minimum (the grids this lane uses are small fixed fixtures; this
    /// intentionally does not reach for a binary heap, mirroring the
    /// corpus's own documented "no heap type in brink" finding rather than
    /// out-engineering the thing it's meant to cross-check).
    #[must_use]
    pub fn dijkstra_cost(
        grid: &[Vec<i64>],
        start: (usize, usize),
        goal: (usize, usize),
    ) -> Option<i64> {
        let rows = grid.len();
        if rows == 0 {
            return None;
        }
        let cols = grid[0].len();
        let unreachable = i64::MAX;
        let mut dist = vec![vec![unreachable; cols]; rows];
        let mut visited = vec![vec![false; cols]; rows];
        dist[start.0][start.1] = 0;

        // BTreeSet of (priority, r, c) — deterministic tie-break by
        // coordinate (never a HashMap/HashSet here; house rule: no
        // iteration order ambiguity where output could depend on it).
        let mut pq: BTreeSet<(i64, usize, usize)> = BTreeSet::new();
        pq.insert((0, start.0, start.1));

        while let Some(&(d, r, c)) = pq.iter().next() {
            pq.remove(&(d, r, c));
            if visited[r][c] {
                continue;
            }
            visited[r][c] = true;
            if (r, c) == goal {
                return Some(dist[r][c]);
            }
            for (dr, dc) in [(-1_i64, 0_i64), (0, 1), (1, 0), (0, -1)] {
                let nr = i64::try_from(r).unwrap_or(i64::MAX) + dr;
                let nc = i64::try_from(c).unwrap_or(i64::MAX) + dc;
                let (Ok(nr), Ok(nc)) = (usize::try_from(nr), usize::try_from(nc)) else {
                    continue;
                };
                if nr >= rows || nc >= cols {
                    continue;
                }
                if grid[nr][nc] == IMPASSABLE {
                    continue;
                }
                let new_dist = dist[r][c] + grid[nr][nc];
                if new_dist < dist[nr][nc] {
                    dist[nr][nc] = new_dist;
                    pq.insert((new_dist, nr, nc));
                }
            }
        }
        None
    }

    #[cfg(test)]
    mod tests {
        use super::dijkstra_cost;

        #[test]
        fn straight_line_costs_the_sum_of_entered_cells() {
            let grid = vec![vec![1, 1, 1]];
            assert_eq!(dijkstra_cost(&grid, (0, 0), (0, 2)), Some(2));
        }

        #[test]
        fn start_equals_goal_costs_zero() {
            let grid = vec![vec![5, 1], vec![1, 1]];
            assert_eq!(dijkstra_cost(&grid, (0, 0), (0, 0)), Some(0));
        }

        #[test]
        fn impassable_cell_forces_a_detour() {
            // Center blocked: only route around costs more than straight
            // through would have.
            let grid = vec![vec![1, 1, 1], vec![1, -1, 1], vec![1, 1, 1]];
            assert_eq!(dijkstra_cost(&grid, (0, 0), (2, 2)), Some(4));
        }

        #[test]
        fn fully_walled_off_goal_is_unreachable() {
            let grid = vec![vec![1, -1, 1]];
            assert_eq!(dijkstra_cost(&grid, (0, 0), (0, 2)), None);
        }

        #[test]
        fn cheaper_detour_beats_a_costlier_direct_route() {
            // Direct path through row 0 costs entering (0,1)=9 then
            // (0,2)=1, total 10. The detour down column 0, across row 2,
            // and back up column 2 is six cost-1 moves, total 6 — Dijkstra
            // must prefer the cheaper detour over the shorter-hop-count
            // direct route.
            let grid = vec![vec![1, 9, 1], vec![1, 9, 1], vec![1, 1, 1]];
            assert_eq!(dijkstra_cost(&grid, (0, 0), (0, 2)), Some(6));
        }

        /// Matches `dijkstra-grid`/`astar-grid`'s exact shared fixture
        /// grid/start/goal (both `story.ink` headers) — pins this
        /// module's own reference implementation against the corpus's
        /// documented golden cost (`expected.txt`: `Total cost: 12`)
        /// independent of the parsing/harness plumbing in
        /// `tests/algorithms_ground_truth.rs`.
        #[test]
        fn matches_the_shared_corpus_fixture_grid() {
            let grid: Vec<Vec<i64>> = vec![
                vec![1, 1, 3, -1, 1, 1],
                vec![1, -1, 3, -1, 1, 1],
                vec![1, -1, 1, 1, 1, -1],
                vec![1, -1, -1, -1, 1, 1],
                vec![1, 1, 2, 2, 1, 1],
                vec![-1, -1, -1, -1, 2, 1],
            ];
            assert_eq!(dijkstra_cost(&grid, (0, 0), (5, 5)), Some(12));
        }
    }
}

/// A bit-for-bit Rust port of `tests/tier1-brink/algorithms/pcg-rng/
/// pcg.ink`'s state-transition/output functions, using the exact same
/// constants and the same wrapping 32-bit arithmetic brink's `int`
/// performs silently on overflow. Used only to generate deterministic
/// seeded property-mode inputs on the Rust side of the differential oracle
/// — see the module doc's `pcg` bullet for what this does and does not
/// prove.
pub mod pcg {
    const MULT: i32 = 1_664_525;
    const INC: i32 = 1_013_904_223;
    const MIX: i32 = -1_640_531_535;
    /// Mirrors `pcg.ink`'s `PCG_OUTPUT_RANGE`.
    pub const OUTPUT_RANGE: i32 = 1_000_000_007;

    /// `pcg_nonneg_mod` — C-style truncating `%` can return negative;
    /// normalize to `[0, m)` via the "mod twice" idiom, exactly as
    /// `pcg.ink::pcg_nonneg_mod` does.
    fn nonneg_mod(x: i32, m: i32) -> i32 {
        let r = x.wrapping_rem(m);
        r.wrapping_add(m).wrapping_rem(m)
    }

    /// `pcg_advance` — one 32-bit LCG step.
    fn advance(state: i32) -> i32 {
        state.wrapping_mul(MULT).wrapping_add(INC)
    }

    /// `pcg_output` — the LCG-state-to-draw mixing step.
    fn output(state: i32) -> i32 {
        let mixed = state.wrapping_mul(MIX);
        nonneg_mod(mixed, OUTPUT_RANGE)
    }

    /// `pcg_seed` — mix an arbitrary caller seed into the LCG's state space.
    #[must_use]
    pub fn seed(seed: i32) -> i32 {
        advance(seed)
    }

    /// `pcg_next` — advance one step, returning `(value, next_state)`.
    #[must_use]
    pub fn next(state: i32) -> (i32, i32) {
        let new_state = advance(state);
        (output(new_state), new_state)
    }

    /// `pcg_below` — a draw reduced to `[0, bound)`, alongside next state.
    #[must_use]
    pub fn below(state: i32, bound: i32) -> (i32, i32) {
        let (value, next_state) = next(state);
        (nonneg_mod(value, bound), next_state)
    }

    #[cfg(test)]
    mod tests {
        use super::{below, next, seed};

        /// Pins this Rust port against `pcg-rng/story.ink`'s own golden
        /// transcript (`expected.txt`) — same seed, same first-N draws,
        /// both raw and bounded — so a future drift in either the ink
        /// port's constants or this Rust port's arithmetic shows up here
        /// directly, independent of any story compiled through the VM.
        #[test]
        fn matches_the_pcg_rng_corpus_golden_transcript() {
            // Fixture values transcribed verbatim from
            // `tests/tier1-brink/algorithms/pcg-rng/expected.txt` (that
            // case's own golden transcript, seed `20260716`) — this test
            // proves this Rust port reproduces the exact same
            // seed/state/draw sequence the compiled ink program does,
            // independent of any compile-and-run plumbing.
            let s = seed(20_260_716);
            assert_eq!(s, 1_398_995_931);

            let mut raw = Vec::new();
            let mut state = s;
            for _ in 0..10 {
                let (v, next_state) = next(state);
                raw.push(v);
                state = next_state;
            }
            assert_eq!(
                raw,
                vec![
                    345_753_644,
                    782_536_508,
                    446_423_406,
                    969_076_125,
                    385_528_306,
                    178_271_495,
                    295_721_051,
                    542_694_899,
                    443_485_453,
                    191_793_931,
                ]
            );

            let mut below_values = Vec::new();
            let mut state2 = s;
            for _ in 0..10 {
                let (v, next_state) = below(state2, 100);
                below_values.push(v);
                state2 = next_state;
            }
            assert_eq!(below_values, vec![44, 8, 6, 25, 6, 95, 51, 99, 53, 31]);
        }
    }
}

/// A tiny parser for brink's `#[...]` array literal syntax restricted to
/// (possibly nested) integer arrays — just enough to pull a fixed corpus
/// case's `VAR grid = #[#[...], ...]` literal straight out of its
/// `story.ink` source text, so the graphs lane's reference grid can never
/// silently drift from what the VM actually compiled (no hand-copied
/// duplicate grid constant to keep in sync by hand).
pub mod ink_literal {
    /// Parse a brink `#[...]` integer/nested-array literal (as found in a
    /// `VAR ... = #[...]` declaration) into a flat `Vec<i64>` (for a
    /// one-dimensional literal) — see [`parse_int_grid`] for the nested
    /// (grid) case used by the graphs lane. `Err` (a message naming the
    /// bad token) rather than a panic on a malformed literal — this is a
    /// production-code module (`cargo clippy`'s deny list applies), and a
    /// caller in a test file is free to `.expect()` the `Result` (tests
    /// are exempt from the unwrap/expect deny per `clippy.toml`).
    pub fn parse_int_array(literal: &str) -> Result<Vec<i64>, String> {
        parse_tokens(literal)
    }

    /// Parse a brink `#[#[...], #[...], ...]` nested integer array literal
    /// into `Vec<Vec<i64>>` — used to pull `dijkstra-grid`/`astar-grid`'s
    /// `VAR grid = ...` literal directly out of `story.ink`'s source text.
    pub fn parse_int_grid(literal: &str) -> Result<Vec<Vec<i64>>, String> {
        let trimmed = literal.trim();
        let inner = trimmed
            .strip_prefix("#[")
            .and_then(|s| s.strip_suffix(']'))
            .unwrap_or(trimmed);
        let mut rows = Vec::new();
        let mut depth = 0_i32;
        let mut start = 0_usize;
        for (i, ch) in inner.char_indices() {
            match ch {
                '[' => depth += 1,
                ']' => depth -= 1,
                ',' if depth == 0 => {
                    rows.push(parse_int_array(&inner[start..i])?);
                    start = i + 1;
                }
                _ => {}
            }
        }
        let tail = inner[start..].trim();
        if !tail.is_empty() {
            rows.push(parse_int_array(tail)?);
        }
        Ok(rows)
    }

    /// Split a flat `#[1, 2, -3, ...]` literal on top-level commas and
    /// parse each token as `i64` (skips the surrounding `#[`/`]`).
    fn parse_tokens(literal: &str) -> Result<Vec<i64>, String> {
        let trimmed = literal.trim();
        let inner = trimmed
            .strip_prefix("#[")
            .and_then(|s| s.strip_suffix(']'))
            .unwrap_or(trimmed);
        inner
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                s.parse::<i64>()
                    .map_err(|e| format!("not an int literal: `{s}` ({e})"))
            })
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::{parse_int_array, parse_int_grid};

        #[test]
        fn parses_a_flat_int_array() {
            assert_eq!(
                parse_int_array("#[5, 2, 9, 1, 5, 6, -3, 0]").unwrap(),
                vec![5, 2, 9, 1, 5, 6, -3, 0]
            );
        }

        #[test]
        fn parses_a_nested_grid() {
            let grid = parse_int_grid("#[#[1, 1, 3], #[1, -1, 3]]").unwrap();
            assert_eq!(grid, vec![vec![1, 1, 3], vec![1, -1, 3]]);
        }
    }
}
