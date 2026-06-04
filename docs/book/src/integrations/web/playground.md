# Playground

The full **[brink Studio](../studio/index.md)**, running live in your browser —
no install. Pick a demo from the binder, edit the ink on the left, and play it on
the right. It's the real authoring app (binder, screenplay editor, IDE features,
live player), built on the [Web & WASM](./index.md) bindings and compiled to
WebAssembly.

<!-- This page only: let the studio fill the content area (which already sits to
     the right of the TOC sidebar — so no full-bleed, no overlap) and hide the
     chapter-nav arrows that would otherwise float over the iframe. The studio is
     responsive, so it adapts to whatever width is available. The iframe target is
     staged by `just book-assets` (see the Web & WASM chapter). -->
<style>
#mdbook-content main { max-width: none; }
.nav-wide-wrapper, .nav-wrapper { display: none !important; }
.brink-studio-embed { display: block; width: 100%; height: 88vh; border: none; }
</style>

<iframe class="brink-studio-embed" src="../../playground/index.html" title="brink Studio"></iframe>
