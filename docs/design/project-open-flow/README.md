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

## Implementing? Start here

Open the `.dc.html` in a browser (they are plain HTML — the `support.js`
line is inert outside the canvas) or just read the markup. Each row names
the region to compare your build against.

| Issue | File | Compare against |
|-------|------|-----------------|
| #3010 | `Flow.dc.html` | the three-column model — what each door guarantees, and the footnotes on precedence |
| #3010 | `Conflict.dc.html` | the banner (top, on `#43404b`) **and** the "How the config was found" walk-up trace below it |
| #3012 | `NewProject.dc.html` | the whole dialog; the "Will create" panel is the part that makes the `main.ink` + `brink.toml` guarantee visible |
| #3015 | `NewProject.dc.html` | the Location / Entry file fields — the form's core case, since a typo'd entry reproduces #3010 |
| #3016 | `Main.dc.html` | the recents list (INK/TOML kind badges) and the "Reopen last project on launch" checkbox beneath it |
| #3017 | `ScopeBanner.dc.html` | the banner strip under the tab, and the "— file not analyzed" note in the status bar |
| #3014 | `Binder.dc.html` | the row treatments and the legend at the bottom: entry / not-included / outside-project |

`Main.dc.html` also carries the startup lockup (app icon + name) if you are
touching that screen for any reason.

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
