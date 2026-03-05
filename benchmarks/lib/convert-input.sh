#!/usr/bin/env bash
# Convert 0-indexed choice inputs to 1-indexed.
# Usage: convert-input.sh < input_0based.txt > input_1based.txt

while IFS= read -r line; do
    n="${line// /}"
    if [[ -z "$n" ]]; then
        echo ""
    else
        echo $(( n + 1 ))
    fi
done
