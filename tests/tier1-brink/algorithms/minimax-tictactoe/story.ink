// ALGORITHMS CORPUS — AI-decision lane (issue #822)
// Minimax with alpha-beta pruning on tic-tac-toe: exhaustive game-tree
// search (9! ply at most, well inside the VM step budget at this board
// size) picks the optimal move for whichever player is to act, pruning
// branches that can't change the outcome. Three fixed positions exercise
// an immediate win, a forced block, and a trivial one-cell finish.
//
// TYPES POLICY: strict. Unlike behavior-tree/utility-ai next door (which
// stayed gradual specifically to isolate the fn-values-as-values
// question), this file has no fn-value indirection at all — board state
// is a plain `array<int>`, every function boundary is a small closed set
// of `int`/`bool` params, and the recursion is genuinely tree-shaped
// (`minimax` calling itself twice per non-terminal node). That is exactly
// dijkstra-grid's "strict earns its keep" shape, not its "three
// monomorphic copies of make_grid" cost — see the findings below for
// where that held and where it didn't.
//
// ERGONOMICS-FINDINGS:
// - Strict mode cost here was genuinely small, unlike dijkstra-grid: every
//   helper (`check_winner`, `minimax`, `best_move`, `board_to_string`,
//   `cell_char`) only ever operates on `array<int>`/`int`/`bool` — there
//   is no "one generic helper reused at three element types" shape for
//   strict's monomorphism rule to split apart, because this file never
//   needed a generic container helper in the first place. Every function
//   boundary below was written with a full `(param: T): R` signature from
//   the start (this corpus's convention regardless of policy), so the
//   ONLY strict-specific fix this file actually needed was three `temp`
//   annotations inside `check_winner` (`temp a: int = line[0]`, `b`, `c`
//   — reading three `int` elements out of an already-`array<int>`-typed
//   `line`). That is the identical gap dijkstra-grid's header documents
//   at length (indexing a known-element-type array into a `temp` doesn't
//   propagate the element type on its own; the annotation has to be
//   repeated at the binding) — confirming that finding generalizes past
//   the grid lane's specific case, but at a MUCH smaller scale here: 3
//   annotations total, not 3 duplicated function bodies. The difference
//   is exactly the one the two files' headers point at each other for:
//   dijkstra-grid's cost came from needing three monomorphic copies of a
//   *generic* helper; this file never needed one, so strict's real
//   marginal cost — once boundaries are annotated anyway — turned out to
//   be three one-word annotations, not a design-level tax.
// - Array **value semantics were verified empirically before writing the
//   backtracking search**, not assumed from the spec: `temp b = a; b[0]
//   = 99` leaves `a[0]` unchanged, and mutating an `array<int>` parameter
//   inside a callee never reaches the caller's copy (confirmed with a
//   throwaway two-line repro of each before committing to this file's
//   design). That is what makes classic minimax backtracking — "place a
//   move directly into `board[i]`, recurse, then reset `board[i]` back to
//   `EMPTY` before trying the next candidate" — safe to write at all:
//   every call frame owns an independent copy of `board`, so there is
//   nothing to alias and nothing extra to copy defensively. This is a
//   pleasant surprise relative to the catalog's own framing ("structs for
//   game state" implicitly worried about copy cost) — the copy-on-write
//   value model makes tree search over mutable-looking local state free
//   to write in the most natural imperative style, with no manual
//   snapshot/restore bookkeeping beyond the single undo line the
//   algorithm needed anyway.
// - `break` cleanly exits the alpha-beta prune out of a `while` loop
//   (`if beta <= alpha { break }`) — this is the "clean early-exit-from-
//   recursion test" the catalog predicted, and it really is clean: no
//   sentinel flag, no `return` bypassing an outer accumulation step, just
//   `break` at exactly the point the classic pseudocode says to prune.
//   The one adjustment from from textbook pseudocode: this file's loop
//   guard is `while i < 9` with the prune check as a **body** `if`/`break`
//   rather than folded into the loop condition itself, to sidestep the
//   `and`/`or` non-short-circuit trap (`bfs-grid-path`, `dijkstra-grid`)
//   the same way `pq_insert` (`dijkstra-grid`) and `binary-search`
//   already do — a condition like `while i < 9 and beta > alpha` would
//   have been the natural pseudocode transliteration and would have been
//   just as wrong here as everywhere else this trap has been documented.
// - Depth-scored terminal values (`10 - depth` for an X win, `depth - 10`
//   for an O win) double as the step-limit-friendly bound the catalog
//   predicted: the search is naturally finite (at most 9 ply, strictly
//   decreasing empty-cell count every recursive call), so no explicit
//   depth cap was needed on top of the board running out of empty cells
//   — flagged here anyway per this project's "guard against unbounded
//   growth" house rule: the bound is real and enforced structurally (the
//   loop only recurses into cells it just filled), not merely assumed.

CONST EMPTY = 0
CONST X = 1
CONST O = 2
CONST DRAW = 3

VAR win_lines: array<array<int>> = #[#[0, 1, 2], #[3, 4, 5], #[6, 7, 8], #[0, 3, 6], #[1, 4, 7], #[2, 5, 8], #[0, 4, 8], #[2, 4, 6]]

VAR reports: array<string> = #[]

~ {
    // Position 1: X completes column (0, 3, 6) — an immediate win is on
    // the board for X to take.
    temp board1: array<int> = #[X, EMPTY, EMPTY, X, EMPTY, EMPTY, EMPTY, EMPTY, O]
    push(reports, describe(board1, X, "Position 1 (X can win now)"))

    // Position 2: O threatens column (0, 3, 6) — X must block at 6 or
    // lose next turn.
    temp board2: array<int> = #[O, EMPTY, EMPTY, O, X, EMPTY, EMPTY, EMPTY, X]
    push(reports, describe(board2, X, "Position 2 (X must block O)"))

    // Position 3: exactly one empty cell left, no winner yet — the only
    // legal move finishes the board in a draw.
    temp board3: array<int> = #[X, O, X, X, O, O, O, X, EMPTY]
    push(reports, describe(board3, X, "Position 3 (one cell left, drawn finish)"))
}

{reports[0]}
{reports[1]}
{reports[2]}
-> END

=== function check_winner(board: array<int>): int ===
~ {
    temp i = 0
    while i < len(win_lines) {
        temp line: array<int> = win_lines[i]
        temp a: int = line[0]
        temp b: int = line[1]
        temp c: int = line[2]
        if board[a] != EMPTY {
            if board[a] == board[b] {
                if board[b] == board[c] {
                    return board[a]
                }
            }
        }
        i = i + 1
    }
    temp j = 0
    temp any_empty = false
    while j < 9 {
        if board[j] == EMPTY {
            any_empty = true
        }
        j = j + 1
    }
    if any_empty {
        return 0
    }
    return DRAW
}

=== function minimax(board: array<int>, depth: int, maximizing: bool, alpha: int, beta: int): int ===
~ {
    temp winner = check_winner(board)
    if winner == X {
        return 10 - depth
    }
    if winner == O {
        return depth - 10
    }
    if winner == DRAW {
        return 0
    }

    temp a = alpha
    temp b = beta
    if maximizing {
        temp best = -1000
        temp i = 0
        while i < 9 {
            if board[i] == EMPTY {
                board[i] = X
                temp score = minimax(board, depth + 1, false, a, b)
                board[i] = EMPTY
                if score > best {
                    best = score
                }
                if best > a {
                    a = best
                }
                if b <= a {
                    break
                }
            }
            i = i + 1
        }
        return best
    }

    temp best = 1000
    temp i = 0
    while i < 9 {
        if board[i] == EMPTY {
            board[i] = O
            temp score = minimax(board, depth + 1, true, a, b)
            board[i] = EMPTY
            if score < best {
                best = score
            }
            if best < b {
                b = best
            }
            if b <= a {
                break
            }
        }
        i = i + 1
    }
    return best
}

=== function best_move(board: array<int>, player: int): int ===
~ {
    temp best_idx = -1
    temp best_score = -1000
    if player == O {
        best_score = 1000
    }
    temp i = 0
    while i < 9 {
        if board[i] == EMPTY {
            board[i] = player
            temp opponent_maximizing = true
            if player == X {
                opponent_maximizing = false
            }
            temp score = minimax(board, 1, opponent_maximizing, -1000, 1000)
            board[i] = EMPTY
            if player == X {
                if score > best_score {
                    best_score = score
                    best_idx = i
                }
            } else {
                if score < best_score {
                    best_score = score
                    best_idx = i
                }
            }
        }
        i = i + 1
    }
    return best_idx
}

=== function cell_char(v: int): string ===
~ {
    if v == X {
        return "X"
    }
    if v == O {
        return "O"
    }
    return "."
}

=== function board_to_string(board: array<int>): string ===
~ {
    temp out = ""
    temp i = 0
    while i < 9 {
        out = out + cell_char(board[i])
        i = i + 1
        if i == 3 {
            out = out + "/"
        }
        if i == 6 {
            out = out + "/"
        }
    }
    return out
}

=== function describe(board: array<int>, player: int, label: string): string ===
~ {
    temp before = board_to_string(board)
    temp move = best_move(board, player)
    board[move] = player
    temp after = board_to_string(board)
    temp mover = cell_char(player)
    return label + ": [" + before + "] " + mover + " plays " + string(move) + " -> [" + after + "]"
}
