---
"@brink-lang/editor": patch
---

`detachedGutters` no longer loses its layout compensation mid-session.
The gutter width it pays back as the content's `padding-left` was tracked
in a per-plugin accumulator, which drifted the moment CodeMirror rewrote
the content's inline style wholesale (`updateAttrs` applies a style
attribute with `dom.style.cssText = …`, so a `tabSize` reconfigure or any
`contentAttributes` style change erases the padding while the plugin
survives believing it is still applied). With the gutter width itself
unchanged, the plugin then wrote nothing at all: the text sat one gutter
width to the left, under the floating gutter overlay, with nothing
overflowing — so horizontal scrolling could not bring it back and only a
reload recovered.

Every measure pass now recomputes the target from the gutter's actual
measured width and the content's actual state, with the compensation
recorded as a custom property in the same inline declaration as the
padding it pays for (so the two can only be present or absent together),
and writes whenever the DOM does not already say exactly that. Any drift
self-heals on the next layout. The plugin also syncs on a reconfigure, so
the erasure is repaired in the same frame rather than at the next
keystroke. The gutters still stay out of the scroller's flex/sticky flow —
the WebKit layout win is untouched.
