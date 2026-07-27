# resolve_*_options Silent-Drop Sweep Results

**Date:** 2026-07-27
**Commit:** f96339698 (current origin/main)
**Issue:** #1382

## Executive Summary

Re-ran the comprehensive sweep of all `resolve_*_options` functions and their callers to verify no silent data drops have been introduced since the original fix in PR #1607. 

**Result: SWEEP CLEAN — No silent drops detected.**

## Functions Swept

### 1. `resolve_language_options` (crates/brink-lsp/src/backend.rs:915)

**Return type:** `(AnalysisOptions, ConfigLoadOutcome)`

**Call sites (2 total):**

| Line | Location | Usage | Status |
|------|----------|-------|--------|
| 428 | `reload_brink_toml` | `self.language.store(resolved)` + `publish_config_outcome(&outcome)` | ✓ Both parts threaded |
| 1063 | `initialize` | `self.language.store(resolved)` + stored `outcome` in mutex | ✓ Both parts threaded |

**Fields resolved by function:**
- `dialect` (via `apply_project_config` or override)
- `types` (via `apply_project_config` or override)
- `lints` (via `apply_project_config`)
- `host_manifest`, `external_check`, `semantic_type_check` (via `default()`)

**Warnings:** Correctly surfaced via `tracing::warn!` (not a silent drop)

### 2. `resolve_analysis_options` (crates/brink-cli/src/ide/project.rs:56)

**Return type:** `Result<AnalysisOptions, String>`

**Call sites (7 total: 3 production + 4 test):**

| Line | Location | Usage | Status |
|------|----------|-------|--------|
| 163 | `Project::load` | `driver.set_analysis_options(...)` | ✓ Threaded |
| 342 | `Project::introduced_diagnostics` | `driver.set_analysis_options(...)` | ✓ Threaded |
| 655 | `load_git_baseline` | `driver.set_analysis_options(...)` | ✓ Threaded |
| 1628, 1654, 1676 | Test modules | Assertions on result | ✓ Properly tested |

**Fields resolved by function:**
- Same as resolve_language_options

**Warnings:** Correctly surfaced via `stderr::writeln!` (not a silent drop)

### 3. `resolve_options` (crates/internal/brink-environment/src/lib.rs:374)

**Return type:** `Result<AnalysisOptions, LoadError>`

**Call sites (1 total):**

| Line | Location | Usage | Status |
|------|----------|-------|--------|
| 303 | `Environment::load` | Stored in `Environment` struct field | ✓ Threaded |

**Fields resolved by function:**
- `dialect`, `types`, `lints` (from config or CLI overrides)
- Calls `apply_lint_overrides` for full lint stack resolution
- `host_manifest`, `external_check`, `semantic_type_check` (via `default()`)

**Warnings:** Correctly surfaced via `tracing::warn!` at 3 sites (not silent drops):
1. Unknown-key warnings from config parse
2. Lint code validation warnings from `apply_project_config`
3. Lint override validation warnings from `apply_lint_overrides`

## Verification Method

For each function:
1. Located all callers via grep
2. Verified return value is bound (not discarded)
3. Traced every field through to its consumer
4. Confirmed warning collections are surfaced (not silently dropped)
5. Checked that all AnalysisOptions fields are handled:
   - Computed fields: dialect, types, lints
   - Defaulted fields: host_manifest, external_check, semantic_type_check

## Oracle Status

Ratchet constant verified at commit f96339698:
- `RATCHET_EPISODE_COUNT = 5577` (in `crates/internal/brink-test-harness/tests/oracle_snapshots.rs`)

## Conclusion

All `resolve_*_options` functions correctly thread their computed values to their consumers. No regressions introduced by:
- PR #1417 (lints tier changes)
- PR #1553/#1559 (propagation changes)
- PR #1526/#1547 (module identity changes)
- PR #1641 (CI work)

**The sweep is complete and clean. Issue #1382 loop is closed.**
