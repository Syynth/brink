---
"@brink-lang/studio": patch
---

Fix: screenplay indents (character / parenthetical / dialogue) no longer collapse to flush-left on browser engines without CSS container-query support (older Chromium-based embeds such as NW.js / CEF). The layout now degrades to viewport-relative scaling there, and keeps pane-relative scaling on engines that support container queries. (#188)
