//! The studio's own icon set, and the asset source that serves it.
//!
//! ## Why this is not a table of SVG strings
//!
//! It used to be: 23 `&'static str` constants in the feature crate, painted
//! through `svg().data(…)`. That had three costs. They lived above the
//! shell, so nothing in the shell — the view switcher, the title bar, the
//! status cells — could use them. They produced a bare [`gpui::Svg`] and not
//! a [`gpui_component::Icon`], so none of the kit's widgets would take one:
//! a `Button`, a tab title, a menu item and a list row all want
//! `impl Into<Icon>`, which is why "put an icon on that tab" kept turning
//! into "hand-build that widget". And adding one meant editing Rust.
//!
//! [`gpui_component::icon_named!`] removes all three. It reads the
//! directory at COMPILE time and writes one PascalCase variant per file,
//! with an [`IconNamed`] impl — the trait the kit documents as the way to
//! make "a drop-in replacement for other UI components". So an icon is a
//! file, a new icon is a new file, and every kit widget accepts one.
//!
//! ## Which set to draw from
//!
//! `gpui-kit-assets` already ships 101 lucide icons reachable as
//! [`gpui_component::IconName`]. **Generic chrome should come from there** —
//! play, pause, settings, panels, chevrons — and only the DOMAIN belongs
//! here: the droplet family, the knot, the stitch, the function marks. Those
//! were ported from `packages/studio-ui/src/icons.tsx` deliberately verbatim
//! (see the geometry note in the app's former `icons` module) so that both
//! studios read the same, and re-drawing them by eye would throw that away.
//!
//! ## One namespace, two sources
//!
//! [`icon_named!`](gpui_component::icon_named) hardcodes each path as
//! `icons/<filename>`, whatever directory it read — so our files and the
//! kit's share one namespace, and a name used by both would silently shadow
//! theirs (including inside kit internals we do not control). Hence
//! `drop.svg` rather than `file.svg`, `find.svg` rather than `search.svg`,
//! and [`tests::our_icon_names_never_shadow_the_kits`], which checks the
//! whole set rather than trusting the next person to remember.

use std::borrow::Cow;

use gpui::{
    AnyElement, App, AssetSource, Hsla, IntoElement, Pixels, RenderOnce, Result, SharedString,
    Styled as _, Svg, Window, svg,
};
use gpui_component::{Icon, IconNamed, icon_named};

// One variant per file in `assets/icons`, sorted, PascalCase.
icon_named!(BrinkIcon, "assets/icons", [Copy, Debug, PartialEq, Eq]);

// The macro derives `IntoElement`, which wants a `RenderOnce`; the kit
// writes these two by hand for its own `IconName` and they are what let a
// variant be handed straight to a widget.
impl RenderOnce for BrinkIcon {
    fn render(self, _: &mut Window, _cx: &mut App) -> impl IntoElement {
        Icon::from(self)
    }
}

impl From<BrinkIcon> for AnyElement {
    fn from(icon: BrinkIcon) -> Self {
        Icon::from(icon).into_any_element()
    }
}

/// One icon as a bare [`Svg`], at an exact pixel size and colour.
///
/// The kit's [`Icon`] is the right thing to hand a widget, and takes a
/// `Size`. This is for the places that size to the text around them — the
/// Binder's rows at 13px, its diagnostic marks at 8 and 10 — which is every
/// caller the inline-string set had. gpui paints an SVG as a monochrome
/// mask tinted by the element's text colour, so the source file's own
/// colours are placeholders and only the alpha its shapes cover matters;
/// that is why an outline file stays an outline and `opacity` /
/// `stroke-dasharray` survive.
pub fn icon(name: BrinkIcon, size: Pixels, color: Hsla) -> Svg {
    svg().size(size).text_color(color).path(name.path())
}

/// The studio's assets: our icons, then the kit's.
///
/// gpui takes ONE [`AssetSource`] for the whole application, and the kit's
/// icons have to keep resolving — every `IconName` in every kit widget goes
/// through it, and registering nothing is what once made all of them draw an
/// empty box. So this serves our directory and delegates everything else.
#[derive(rust_embed::RustEmbed)]
#[folder = "assets"]
#[include = "icons/**/*.svg"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        if let Some(file) = Self::get(path) {
            return Ok(Some(file.data));
        }
        gpui_kit_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut found: Vec<SharedString> = Self::iter()
            .filter_map(|p| p.starts_with(path).then(|| p.into()))
            .collect();
        found.extend(gpui_kit_assets::Assets.list(path)?);
        found.sort();
        found.dedup();
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two sets share the `icons/` namespace, and ours is looked up
    /// first — so a name in both would replace the kit's own icon wherever
    /// the kit itself draws it, silently and everywhere. Checked
    /// mechanically because the failure is invisible: the wrong glyph is
    /// still a glyph.
    #[test]
    fn our_icon_names_never_shadow_the_kits() {
        let ours: Vec<String> = Assets::iter().map(|p| p.to_string()).collect();
        let theirs: Vec<String> = gpui_kit_assets::Assets::iter()
            .map(|p| p.to_string())
            .collect();
        let clash: Vec<&String> = ours.iter().filter(|p| theirs.contains(p)).collect();
        assert!(
            clash.is_empty(),
            "these shadow the kit's own icons — rename them: {clash:?}"
        );
    }

    /// The delegation is the whole point of this source and the only part
    /// with a runtime failure mode: serve ours and drop the kit's, and every
    /// `IconName` in every kit widget goes back to drawing nothing — which
    /// is exactly the bug that shipped once already, silently, because a
    /// missing icon looks like a design choice.
    #[test]
    fn the_source_serves_both_sets() {
        let ours = Assets
            .load("icons/knot.svg")
            .expect("ours resolves")
            .expect("ours is present");
        assert!(ours.starts_with(b"<svg"), "ours is an svg document");

        let theirs = Assets
            .load("icons/play.svg")
            .expect("the kit's resolves through the fallback")
            .expect("the kit's is present");
        assert!(theirs.starts_with(b"<svg"), "the kit's is an svg document");

        assert!(
            Assets.list("icons/").expect("listing works").len() > 100,
            "the listing is both sets, not just ours"
        );
    }

    /// The macro reads a directory at compile time, so an empty or
    /// mis-pathed one degrades to an enum with no variants rather than a
    /// build error. This is what notices.
    #[test]
    fn every_icon_file_became_a_variant() {
        let files = Assets::iter().count();
        assert_eq!(
            files, 24,
            "expected the ported set plus `infinity`; found {files}"
        );
        assert_eq!(BrinkIcon::Knot.path(), "icons/knot.svg");
        assert_eq!(BrinkIcon::Drop.path(), "icons/drop.svg");
    }
}
