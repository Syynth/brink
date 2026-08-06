---
"@brink-lang/web": patch
---

Fix #1573: `Story::did_safe_exit` (and the new `FlowInstance::did_safe_exit`)
are promoted off the `testing`-only feature gate onto the production
runtime surface. A `Line::Done` is delivered both for an explicit
`-> DONE` and for a flow that ran out of content — until now the only way
to tell them apart outside the `testing` feature was to issue an extra
`continue`/`advance` call and catch `RuntimeError::RanOutOfContent`. Hosts
(bevy-brink, brink-web, brink-cli, brink-ide) can now read
`did_safe_exit()` directly after a `Line::Done` instead. No story
output/execution behavior changes — this only widens what was already
computed internally to be a normal `pub fn`.
