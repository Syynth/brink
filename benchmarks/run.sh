#!/usr/bin/env bash
# Run cross-implementation benchmarks using hyperfine.
set -euo pipefail

cd "$(dirname "$0")/.."

BRINK_CLI="./target/release/brink-cli"
BRINK_LOOP="./benchmarks/drivers/brink-loop/target/release/brink-loop"
INKJS_DRIVER="./benchmarks/drivers/inkjs/driver.mjs"

# Resolve inklecate
INKLECATE="${INKLECATE:-}"
if [[ -z "$INKLECATE" ]]; then
    if command -v inklecate &>/dev/null; then
        INKLECATE="inklecate"
    elif [[ -x "$HOME/code/rs/s92-studio/reference/ink/inklecate/bin/Release/net8.0/inklecate" ]]; then
        INKLECATE="$HOME/code/rs/s92-studio/reference/ink/inklecate/bin/Release/net8.0/inklecate"
    fi
fi

for scenario_toml in benchmarks/scenarios/*/scenario.toml; do
    scenario_dir="$(dirname "$scenario_toml")"
    scenario_name="$(basename "$scenario_dir")"

    echo "=== Scenario: $scenario_name ==="

    # Parse scenario.toml (simple grep-based — no toml parser needed)
    story="$(grep '^story' "$scenario_toml" | sed 's/.*= *"//' | sed 's/".*//')"
    input="$(grep '^input ' "$scenario_toml" | sed 's/.*= *"//' | sed 's/".*//')"

    if [[ ! -f "$story" ]]; then
        echo "ERROR: story file not found: $story"
        continue
    fi
    if [[ ! -f "$input" ]]; then
        echo "ERROR: input file not found: $input"
        continue
    fi

    # Create 1-indexed input in temp file
    input_1=$(mktemp)
    trap 'rm -f "$input_1"' EXIT
    bash benchmarks/lib/convert-input.sh < "$input" > "$input_1"

    # Build hyperfine command
    cmds=()
    names=()

    # brink-cli (1-indexed input)
    if [[ -x "$BRINK_CLI" ]]; then
        cmds+=("$BRINK_CLI play $story --input $input_1")
        names+=("brink-cli")
    fi

    # brink-loop (0-indexed input, single iteration for hyperfine)
    if [[ -x "$BRINK_LOOP" ]]; then
        cmds+=("$BRINK_LOOP $story $input")
        names+=("brink-loop")
    fi

    # binkplayer (1-indexed, reads from stdin)
    if command -v binkplayer &>/dev/null; then
        cmds+=("binkplayer $story < $input_1")
        names+=("bladeink")
    fi

    # inklecate (1-indexed, reads from stdin)
    if [[ -n "$INKLECATE" ]]; then
        cmds+=("$INKLECATE -p $story < $input_1")
        names+=("inklecate")
    fi

    # inkjs (1-indexed input file)
    if [[ -x "$(command -v node)" ]] && [[ -d benchmarks/drivers/inkjs/node_modules ]]; then
        cmds+=("node $INKJS_DRIVER $story $input_1")
        names+=("inkjs")
    fi

    if [[ ${#cmds[@]} -eq 0 ]]; then
        echo "No implementations available — skipping"
        continue
    fi

    # Build hyperfine args
    hf_args=(--warmup 3 --min-runs 10)
    hf_args+=(--export-markdown "benchmarks/results_${scenario_name}.md")

    for i in "${!cmds[@]}"; do
        hf_args+=(--command-name "${names[$i]}" "${cmds[$i]}")
    done

    echo "Running hyperfine with ${#cmds[@]} implementations..."
    hyperfine "${hf_args[@]}"

    rm -f "$input_1"
    echo ""
done

echo "=== Done ==="
