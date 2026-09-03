---
"@brink-lang/web": patch
---

`fix_all`'s batch report is no longer silently dropped when a caller
combines a diff-style preview with a dry run, or when the round cap is
hit — both now surface the report so a capped batch always explains
itself instead of exiting with nothing to show for it. The
`ProjectConfig` fix-policy bridge (`Off`/`Auto`/`Ask`-elides-to-no-override)
that decides which fixes batch is now one shared mapping
(`brink_ide::fix::policy::FixMode::from_config`) instead of two
independently hand-rolled copies, closing a drift risk between the CLI
and the wasm batch surface the Problems panel and fix-on-save read.
