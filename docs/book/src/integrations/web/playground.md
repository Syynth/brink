# Playground

The full **[brink Studio](../studio/index.md)**, running live in your browser —
no install. Pick a demo from the binder, edit the ink on the left, and play it on
the right. It's the real authoring app (binder, screenplay editor, IDE features,
live player), built on the [Web & WASM](./index.md) bindings and compiled to
WebAssembly.

<!-- Full-bleed so the studio's three panels get viewport width rather than the
     book's centered content column. The iframe target is staged by
     `just book-assets` (see the Web & WASM chapter). -->
<div style="position:relative;left:50%;right:50%;margin-left:-50vw;margin-right:-50vw;width:100vw;">
  <iframe src="../../playground/index.html" title="brink Studio" style="width:100%;height:88vh;border:none;"></iframe>
</div>
