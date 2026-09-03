---
"@brink-lang/studio": minor
---

Settings › Player gains **Reading** and **Reading aids** (#3438): a font
picker with a live specimen list (a curated set of reading faces on the
web, plus a family you type; the desktop app can supply the machine's
fonts through the new `systemFonts` mount option), line spacing and
measure steppers, and toggles for the go-to-source button and the
choice markers. All app scope, persisted with the Player settings, and
applied through CSS variables (`--bs-player-font-family`,
`--bs-player-line-height`, `--bs-player-measure`) the same way the
Player font size already is.
