---
"@brink-lang/web": patch
---

The `ProjectConfig` fix-policy bridge (`Off`/`Auto`/`Ask`-elides-to-no-override)
that decides which fixes batch is now one shared mapping
(`brink_ide::fix::policy::FixMode::from_config`) instead of two
independently hand-rolled copies, closing a drift risk between the CLI
and the wasm batch surface the Problems panel and fix-on-save read. This
is a policy-bridge refactor only — `fix_all`'s `FixReportJs` (including
`cap_hit`) is unchanged. (`brink fix`'s `--diff`/`--dry-run` composition
and cap_hit-report fix is CLI-only and does not touch `@brink-lang/web`.)
