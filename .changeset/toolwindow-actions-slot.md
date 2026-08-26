---
"@brink-lang/studio": minor
---

Tool windows can contribute controls to their chrome header:
`ToolWindowDescriptor.actions` takes a component, rendered between the
panel title and the close button. It follows the existing `badge`
contract — the registering app supplies the component, so it subscribes to
that app's own store and stays reactive without the shell depending on any
app store. The header's uppercase, letter-spaced title styling is reset
inside the slot so action components render with ordinary control
typography.
