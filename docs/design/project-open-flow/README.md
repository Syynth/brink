# Project-open flow — design source

Artboard sources for the **file-anchored project open model** ruled 2026-08-23
(`docs/decision-log.md`, "A project is anchored on a FILE"). These are the
markup the design canvas renders, kept here so the intended UI is diffable
against what gets built — a screenshot in a chat log is not reviewable.

| File | Screen |
|------|--------|
| `Flow.dc.html` | The model: two doors, and what each guarantees |
| `Main.dc.html` | Startup — New Project / Open, recents, reopen toggle |
| `NewProject.dc.html` | New Project dialog → `main.ink` + `brink.toml` |
| `Conflict.dc.html` | Conflict warning + the walk-up discovery trace |
| `Binder.dc.html` | Binder scope marks — entry, not-included, outside-project |
| `ScopeBanner.dc.html` | Editor banner for a file outside the analyzer's scope |
| `canvas.json` | Canvas layout, artboard sizes, annotations |

## What these are matched against

Values were lifted from the shipped app, not eyeballed — **Catppuccin Mocha**
under the `--bs-*` semantic layer (`packages/studio-shell/src/styles/themes/`),
`system-ui`, `--binder-row-height: 26px`, `--binder-indent: 18px`, accent
`--ctp-blue #89b4fa`, and the app's own `--bs-conflict-banner-bg #43404b` for
the warning. The startup screen inlines `assets/brand/brink-icon-night.svg`
(the Dock icon), keeping brand blue `#7E96FF` exact rather than harmonising it
to the UI accent.

If a token changes in the app, these drift — they are a design record of a
moment, not a live component library. Check the source of truth before
treating a value here as current.

## Format

`.dc.html` (Design Components). Each file is one artboard; `canvas.json`
places them. They render on a canvas published as an Artifact — the link
lives on the epic issue rather than here, since artifact URLs are per-account
and can be re-published.
