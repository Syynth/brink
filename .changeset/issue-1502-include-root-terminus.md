---
"@brink-lang/web": patch
---

Fix #1502: in a multi-file project, the implicit final gather #1448 added is
now synthesized for the **entry file only**, not once per `INCLUDE`d file.

Brink lowers root-level content one chunk per file, so #1500 attached a
terminus to every file's chunk. A trailing weave in an `INCLUDE`d file
therefore ended the story silently at that file's last gather, swallowing
everything the entry file had after the `INCLUDE`. C# ink guards the implicit
gather with `if (isRootStory)` (`FlowBase.SplitWeaveAndSubFlowContent`): an
included file is parsed as `Story(isInclude: true)`, its root content becomes a
nested weave container, and running off the end of that container reports
"ran out of content. Do you need a `-> DONE` or `-> END`?" — the loud fault
brink now reports again.

Playground/editor projects that `INCLUDE` a file ending in an unterminated
weave get the same diagnosis the reference compiler gives instead of quietly
truncating; a trailing weave in the entry file still ends cleanly. Oracle
conformance: 5,599 → 5,603 passing episodes, 359 → 361 passing cases (two new
`tier3/includes` cases; no existing episode changed).
