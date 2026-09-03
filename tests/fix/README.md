# Auto-fix `Safe` fixtures

One directory per diagnostic code. Each is a **pre-fix / post-fix source
pair** run through `brink_test_harness::fix::assert_safe_fix`, which is
`docs/autofix-spec.md` §3's `Safe` obligation made executable on top of the
observable-equivalence oracle (`docs/observable-semantics-spec.md` §2/§2.2).

```text
tests/fix/<code>/
  before.ink        the entry, pre-fix          (or before.brink)
  expected.ink      the entry, post-fix         (or expected.brink)
  brink.toml        optional — copied verbatim, so a fixture can set
                    `dialect` / `types` the way a real project does
  rewrites.txt      optional — line-identity changes the fix necessarily
                    makes, one `scope-id` or `scope-id#index` per line;
                    anything the diff reports that is not listed here fails
                    §2.2
  README.md         optional — ignored by the loader
  anything else     copied verbatim beside the entry (e.g. an INCLUDEd file)
```

Both sides compile from the **same** entry file name (`story.ink` /
`story.brink`) in two scratch directories, through the production road
(`brink_environment::compile`), so nothing in the comparison varies with the
file's own name and a fixture's `brink.toml` is honoured exactly as it would
be in a real project.

Who runs what:

- `crates/internal/brink-test-harness/tests/fix_safe_obligations.rs` sweeps
  every directory here, and separately requires every `Safe`-max fixer in
  `brink_ide::fix::FIXERS` to have one.
- `brink_ide::fix`'s own registry test demands the fixture *exists*; it
  cannot run it, because `brink-test-harness` depends on `brink-ide` and the
  dependency only goes one way.

## What is here today, and what each one proves

| Fixture | Fixer | Verdict |
|---|---|---|
| `E014` | none yet (§9 first-wave Safe) | `ObservablyEquivalent` |
| `E025` | `ImportFixer` (add import) | `NoPreImage` |
| `E063` | `ValueCallArityFixer` (trim call args) | `NoPreImage` |
| `E080` | `BindRefArgsFixer` (bind `ref` args) | `NoPreImage` |
| `E081` | `TrimFnLiteralArgsFixer` (trim `#fn` args) | `NoPreImage` |
| `E095` | `StaleWasFixer` (delete the stale `#@was`) | `ObservablyEquivalent` |

`NoPreImage` is the honest answer, not a gap in the fixture: all four
migrated fixers discharge a diagnostic that **prevents compilation**, so
there is no pre-fix program whose behaviour could be preserved and §2 says
nothing about them. (E063's base severity is `types`-policy-dependent, and
the policy under which it fires at all — `types = "strict"` — is the one that
makes it an error.) All four already declare `Applicability::Suggested`;
this is the mechanical confirmation that they could not declare `Safe` even
if someone wanted them to.

`E014` and `E095` are the positive cases — the ones that prove the oracle is
doing anything. `E014` has no fixer yet — that is its own sub-issue of
#3374 — and the registry obligation runs the other way round, so a fixture
without a fixer is fine while a `Safe` fixer without a fixture is not.
`E095`'s `StaleWasFixer` has both; its fixture is the one shape where
deletion is unconditionally safe (no attached declaration to read the same
line differently) — see `tests/fix/E095/README.md` and
`crates/internal/brink-ide/src/stale_was_fix.rs`'s module doc for the
narrowing that withholds the fix in the shapes that aren't.
