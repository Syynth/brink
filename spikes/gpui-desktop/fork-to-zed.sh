#!/usr/bin/env bash
# Repoint this spike from longbridge's `gpui-pre` republish onto Zed's OWN
# `gpui`, straight from Zed's repository.
#
# WHY THIS SCRIPT EXISTS
#
# `gpui-component`/`gpui-base` hard-wire `gpui = { version = "0.3.1",
# package = "gpui-pre" }`. A `[patch.crates-io]` cannot redirect that: the
# patch resolves, and Cargo then discards it because Zed's `gpui` is version
# **0.2.2**, which does not satisfy `^0.3.1`. `gpui-pre` renumbered to 0.3.x.
# So substitution requires vendoring and editing manifests — a fork.
#
# It is a small fork. `gpui-pre` 0.3.3 is a snapshot of `zed@5b055fa`
# (2026-09-03) and 89 of its 90 source files are byte-identical to Zed's; the
# lone difference is `action.rs`, where the `actions!` macro uses
# `$crate::Action` instead of `gpui::Action` so the crate works under a new
# name. Nothing here rewrites logic — every edit below is about NAMES.
#
# `vendor/` is gitignored — this script rebuilds it from the registry cache.
# Run it once after checkout, then `cargo build`. The committed `Cargo.toml`
# already points at `vendor/`, so nothing else has to change.

set -euo pipefail

REV="5b055fa789a8b8d38ac951a6e0cde272f66b4495"   # the commit gpui-pre 0.3.3 snapshots
GIT="https://github.com/zed-industries/zed"
HERE="$(cd "$(dirname "$0")" && pwd)"
VENDOR="$HERE/vendor"

# Locate the registry cache the crates were unpacked into.
SRC="$(dirname "$(find "${CARGO_HOME:-$HOME/.cargo}/registry/src" -maxdepth 2 -type d -name 'gpui-base-0.6.0' | head -1)")"
if [ -z "$SRC" ]; then
  echo "gpui-base-0.6.0 is not in the registry cache; run 'cargo fetch' first." >&2
  exit 1
fi

rm -rf "$VENDOR"
mkdir -p "$VENDOR"
for crate in gpui-base gpui-component gpui-kit-assets gpui-component-macros; do
  cp -R "$SRC/$crate-0.6.0" "$VENDOR/$crate"
  chmod -R u+w "$VENDOR/$crate"
  rm -f "$VENDOR/$crate/.cargo-ok"
done

REV="$REV" GIT="$GIT" VENDOR="$VENDOR" python3 - <<'PY'
import os, pathlib, re

REV, GIT, VENDOR = os.environ["REV"], os.environ["GIT"], pathlib.Path(os.environ["VENDOR"])

# Every `gpui-pre*` package is a rename of a crate in Zed's repo.
MAP = {
    "gpui-pre": "gpui",
    "gpui-pre-macros": "gpui_macros",
    "gpui-pre-sum-tree": "sum_tree",
    "gpui-pre-platform": "gpui_platform",
    "gpui-pre-reqwest-client": "reqwest_client",
}

def rewrite(path, local_paths):
    """Point `gpui-pre*` dependency tables at Zed's git, and sibling
    vendored crates at their local paths."""
    blocks, current = [], []
    for line in path.read_text().splitlines():
        if line.startswith("["):
            if current:
                blocks.append(current)
            current = [line]
        else:
            current.append(line)
    if current:
        blocks.append(current)

    out, changed = [], 0
    for block in blocks:
        header = block[0] if block and block[0].startswith("[") else ""
        is_dep = ".dependencies." in header or header.startswith(
            ("[dependencies.", "[dev-dependencies.", "[build-dependencies.")
        )
        pkg = next(
            (m.group(1) for m in (re.match(r'\s*package = "([^"]+)"', l) for l in block) if m),
            None,
        )
        if is_dep and pkg in MAP:
            body = [l for l in block[1:] if not re.match(r"\s*(version|package) = ", l)]
            block = [header, f'git = "{GIT}"', f'rev = "{REV}"', f'package = "{MAP[pkg]}"'] + body
            changed += 1
        elif is_dep:
            name = header.split(".")[-1].rstrip("]")
            if name in local_paths:
                body = [l for l in block[1:] if not re.match(r"\s*version = ", l)]
                block = [header, f'path = "{local_paths[name]}"'] + body
                changed += 1
        out.extend(block)
    path.write_text("\n".join(out) + "\n")
    return changed

print("gpui-base            ", rewrite(VENDOR / "gpui-base/Cargo.toml", {}))
print("gpui-kit-assets      ", rewrite(VENDOR / "gpui-kit-assets/Cargo.toml", {}))
print(
    "gpui-component       ",
    rewrite(
        VENDOR / "gpui-component/Cargo.toml",
        {
            "gpui-base": "../gpui-base",
            "gpui-kit-assets": "../gpui-kit-assets",
            "gpui-component-macros": "../gpui-component-macros",
        },
    ),
)

# `IntoPlot` resolves the GPUI path by looking up a dependency whose PACKAGE
# is literally `gpui-kit` or `gpui-pre` — which makes the macro unusable
# against the crate those two are snapshots of. Renaming the dependency does
# not help: proc-macro-crate matches on the real package name, not the alias.
crate_path = VENDOR / "gpui-component-macros/src/crate_path.rs"
s = crate_path.read_text()
s = s.replace(
    '        Err(kit_error) => crate_name("gpui-pre")',
    '        // VENDOR EDIT: accept Zed\'s own `gpui` package too.\n'
    '        Err(kit_error) => crate_name("gpui-pre")\n'
    '            .or_else(|_| crate_name("gpui"))',
)
crate_path.write_text(s)
print("gpui-component-macros 1 (IntoPlot crate lookup)")

# `Editor` has no size of its own, so `Input` renders it at `Size::default()`
# (Medium) and pads it by 8px top and bottom. In the stacked Continuous view
# that reads as half a line of dead space above every file, and it makes a
# section impossible to size exactly — which matters, because a section that
# can scroll at all swallows the wheel instead of scrolling the manuscript.
# Giving `Editor` a size lets a section ask for `XSmall`, whose padding is 0.
editor = VENDOR / "gpui-component/src/input/editor.rs"
e = editor.read_text()
e = e.replace(
    "use crate::{ActiveTheme as _, RoleOverride, StyledExt as _};",
    "use crate::{ActiveTheme as _, RoleOverride, Sizable as _, StyledExt as _};",
)
e = e.replace(
    """pub struct Editor {
    state: Entity<EditorState>,
    style: StyleRefinement,""",
    """pub struct Editor {
    state: Entity<EditorState>,
    style: StyleRefinement,
    /// VENDOR EDIT: drives `Input`'s vertical padding (`Size::input_py()`).
    size: crate::Size,""",
)
e = e.replace(
    """            state: state.clone(),
            style: StyleRefinement::default(),""",
    """            state: state.clone(),
            style: StyleRefinement::default(),
            size: crate::Size::default(),""",
)
e = e.replace(
    "        Input::from_state(self.state.clone())",
    "        Input::from_state(self.state.clone())\n            .with_size(self.size)",
)
e = e.replace(
    "impl Styled for Editor {",
    """impl crate::Sizable for Editor {
    fn with_size(mut self, size: impl Into<crate::Size>) -> Self {
        self.size = size.into();
        self
    }
}

impl Styled for Editor {""",
)
editor.write_text(e)
print("gpui-component        1 (Editor: Sizable)")

PY

echo
echo "Done. \`cargo build\` now builds against Zed's own gpui at $REV."
echo "Revert with: git checkout -- Cargo.toml && rm -rf vendor"
