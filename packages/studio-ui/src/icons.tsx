import { useId } from "react";
/**
 * The binder v2 icon language (#3037 — compare
 * `docs/design/binder-v2/{Main,Structure}.dc.html`): monochrome
 * `currentColor` SVGs, themed by whatever `color` the surrounding row
 * class sets (`binder.css`'s `.brink-binder-icon-*` tints keep working
 * unchanged). Replaces the glyph characters (◆ § ƒ ▶ 📄 …) the
 * maintainer ruled out 2026-08-23 ("the emoji icons are ugly and dumb").
 *
 * Shared components rather than inline markup so the Binder, tabs, and
 * Story Graph can converge on one vocabulary later.
 */

interface IconProps {
  size?: number;
}

const strokeProps = (size: number) =>
  ({
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 2,
    strokeLinecap: "round",
    strokeLinejoin: "round",
    "aria-hidden": true,
    focusable: false,
  }) as const;

/** The brink droplet — the brand mark's outline, for `.ink` files
 *  (maintainer note: "use the brink icon… or a modified version"). */
export function BrinkFileIcon({ size = 13 }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 100 100"
      fill="none"
      stroke="currentColor"
      strokeWidth={8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      focusable={false}
    >
      <path d="M50 6 C54 16 64 28 73 41 A28 28 0 1 1 27 41 C36 28 46 16 50 6 Z" />
    </svg>
  );
}

/**
 * The ink-file drop, DASHED — a draft (#3145, decision log 2026-08-27
 * "Draft status is an icon variant, not a text badge").
 *
 * Same path as {@link BrinkFileIcon} on purpose: a draft is the same kind
 * of thing as any other story file, drawn provisionally. The dash pattern
 * is tuned to the path's own length so the gaps land evenly around the
 * drop rather than bunching at the point.
 *
 * The colour comes from `.brink-file-icon-draft` rather than a `stroke`
 * here, so a theme can restate it — and so the icon still inherits
 * `currentColor` if that class is ever missing, degrading to a dashed
 * outline rather than to an invisible one.
 */
export function BrinkFileDraftIcon({ size = 13 }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 100 100"
      fill="none"
      stroke="currentColor"
      strokeWidth={8}
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeDasharray="14 11"
      className="brink-file-icon-draft"
      aria-hidden
      focusable={false}
    >
      <path d="M50 6 C54 16 64 28 73 41 A28 28 0 1 1 27 41 C36 28 46 16 50 6 Z" />
    </svg>
  );
}

/**
 * The entry file: the brink mark itself — the ink drop with the divert
 * carved out of the bowl.
 *
 * Geometry is lifted verbatim from `assets/brand/brink-glyph.svg`, the
 * brand asset made for exactly this ("the drop alone with the carve as
 * true negative space, for use on any background"), and pinned to that
 * file by `binder-entry-icon.test.tsx`. The carve is a STROKE through a
 * mask, not a filled arrow: the brand README states the construction —
 * one stroke weight of 7.5, mass-centred on the bowl — and that "the full
 * arrow is used at every size; there is no simplified small-size
 * variant", so this does not get a chunkier 13px version.
 *
 * The `<g>` transform maps the brand's bowl onto the SIBLING drop's bowl,
 * and is shared by the drop and the carve so the brand geometry moves as
 * one piece. Sibling bowl: arc endpoints (73,41)/(27,41) at r28, so its
 * centre is (50, 41+sqrt(28^2-23^2)) = (50, 56.97). Brand bowl: (50, 54)
 * at r30. Hence scale 28/30 with the centres matched — which makes the
 * brand silhouette coincide with the sibling drop almost exactly (tip at
 * 6.57 vs 6, same bottom), so the entry sits on the same footprint as
 * every other row and the arrow rides at the bowl centre, where the brand
 * puts it. Two earlier versions got this wrong in opposite directions:
 * height-matching shrank the mark visibly, and box-centring
 * (`translate(0 8)`) slid the whole glyph — arrow included — below its
 * neighbours' bowl line.
 *
 * Replaces the "entry" text badge (#3014/#3021), following the rule the
 * Binder already set for drafts: "a draft carries its status in its ICON"
 * (decision log 2026-08-27), taking no badge.
 */
export function BrinkFileEntryIcon({ size = 13 }: IconProps) {
  // The mask needs a document-unique id: the icon renders once per entry
  // row, and duplicate ids would make every instance resolve the first.
  const maskId = useId();
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 100 100"
      fill="currentColor"
      className="brink-file-icon-entry"
      aria-hidden
      focusable={false}
    >
      <mask id={maskId}>
        <rect x="0" y="0" width="100" height="100" fill="white" />
        <g transform="translate(3.333 6.569) scale(0.93333)">
          <path
            d="M36 54 L56 54 M50 43 L62 54 L50 65"
            fill="none"
            stroke="black"
            strokeWidth={7.5}
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </g>
      </mask>
      <g transform="translate(3.333 6.569) scale(0.93333)">
        <path
          d="M50 0 C54 10 65.6 23.4 74.94 37.34 A30 30 0 1 1 25.06 37.34 C34.4 23.4 46 10 50 0 Z"
          mask={`url(#${maskId})`}
        />
      </g>
    </svg>
  );
}

/**
 * The entry file, EXPANDED (or with nothing inside it).
 *
 * The Binder's fill rule is "filled = collapsed with content inside;
 * outline = expanded or a leaf" (ruled 2026-08-23), and the entry has to
 * obey it like every other row — an entry that stayed a solid mark while
 * its neighbours hollowed out on expansion would read as a different kind
 * of thing rather than the same file, opened.
 *
 * So this is the siblings' outline drop with the divert INLAID — stroked
 * inside the bowl — rather than the brand mark's knockout, which needs a
 * fill to cut a hole out of. Same two strokes either way.
 *
 * The carve is the brand's own path under the SAME bowl-to-bowl transform
 * {@link BrinkFileEntryIcon} uses (brand bowl (50,54) r30 onto sibling
 * bowl (50,56.97) r28 — see its derivation), so the arrow does not move a
 * pixel when a row swaps between the two variants on expand/collapse. The
 * scale carries the stroke width with it, keeping the brand's "one stroke
 * weight" relation to the bowl. The first version of this transform used a
 * mis-derived bowl centre of (50,47) — subtracting the half-chord offset
 * instead of adding it — which parked the arrow visibly high in the drop.
 */
export function BrinkFileEntryOutlineIcon({ size = 13 }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 100 100"
      fill="none"
      stroke="currentColor"
      strokeWidth={8}
      strokeLinecap="round"
      strokeLinejoin="round"
      className="brink-file-icon-entry"
      aria-hidden
      focusable={false}
    >
      <path d="M50 6 C54 16 64 28 73 41 A28 28 0 1 1 27 41 C36 28 46 16 50 6 Z" />
      <g transform="translate(3.333 6.569) scale(0.93333)">
        <path d="M36 54 L56 54 M50 43 L62 54 L50 65" strokeWidth={7.5} />
      </g>
    </svg>
  );
}

/** A generic document — non-story files (brink.toml uses GearIcon). */
export function DocIcon({ size = 13 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6" />
    </svg>
  );
}

export function FolderIcon({ size = 13 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </svg>
  );
}

/** An OPEN folder — the expansion state icon (Zed-style: the icon IS the
 *  chevron, ruled 2026-08-23). */
export function FolderOpenIcon({ size = 13 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M5 19l2.7-6.3A2 2 0 0 1 9.5 11H21l-2.8 6.8a2 2 0 0 1-1.8 1.2z" />
      <path d="M5 19V7a2 2 0 0 1 2-2h3l2 2h7a2 2 0 0 1 2 2v2" />
    </svg>
  );
}

/** A filled closed folder — collapsed WITH content ("something folded
 *  inside"; ruled 2026-08-23: filled = expandable-and-closed, outline =
 *  expanded or leaf). */
export function FolderFilledIcon({ size = 13 }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="currentColor"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinejoin="round"
      aria-hidden
      focusable={false}
    >
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </svg>
  );
}

/** The filled droplet — a collapsed file WITH knots (same fill rule). */
export function BrinkFileFilledIcon({ size = 13 }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 100 100"
      fill="currentColor"
      stroke="currentColor"
      strokeWidth={8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      focusable={false}
    >
      <path d="M50 6 C54 16 64 28 73 41 A28 28 0 1 1 27 41 C36 28 46 16 50 6 Z" />
    </svg>
  );
}

/** The filled diamond — a collapsed knot WITH stitches (same fill rule). */
export function KnotFilledIcon({ size = 12 }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden focusable={false}>
      <path d="M12 3l7 9-7 9-7-9z" />
    </svg>
  );
}

/** The Library section (mounted stdlib) — a closed book. */
export function LibraryIcon({ size = 13 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
      <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
    </svg>
  );
}

/** A knot — the diamond outline. */
export function KnotIcon({ size = 12 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M12 3l7 9-7 9-7-9z" />
    </svg>
  );
}

/** A stitch — the branch arrow. */
export function StitchIcon({ size = 12 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M6 4v8a4 4 0 0 0 4 4h8" />
      <path d="M14 12l4 4-4 4" />
    </svg>
  );
}

/** A function knot — parentheses. */
export function FunctionIcon({ size = 12 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M8 4c-2 0-3 1-3 4v3c0 2-1 3-2 3 1 0 2 1 2 3v3c0 2 1 4 3 4" />
      <path d="M16 4c2 0 3 1 3 4v3c0 2 1 3 2 3-1 0-2 1-2 3v3c0 2-1 4-3 4" />
    </svg>
  );
}

/** The expand/collapse twisty. Rotation is CSS's job (the existing
 *  `.brink-binder-chevron.collapsed` rule); this is the glyph only. */
export function ChevronIcon({ size = 11 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M6 9l6 6 6-6" />
    </svg>
  );
}

export function GrabHandleIcon({ size = 12 }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden focusable={false}>
      <circle cx="9" cy="6" r="1.6" />
      <circle cx="15" cy="6" r="1.6" />
      <circle cx="9" cy="12" r="1.6" />
      <circle cx="15" cy="12" r="1.6" />
      <circle cx="9" cy="18" r="1.6" />
      <circle cx="15" cy="18" r="1.6" />
    </svg>
  );
}

export function FilePlusIcon({ size = 13 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6" />
      <path d="M12 12v6M9 15h6" />
    </svg>
  );
}

export function FolderPlusIcon({ size = 13 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
      <path d="M12 10v6M9 13h6" />
    </svg>
  );
}

export function ExpandAllIcon({ size = 14 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M7 8l5 5 5-5" />
      <path d="M7 14l5 5 5-5" opacity=".45" />
    </svg>
  );
}

export function CollapseAllIcon({ size = 14 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M7 16l5-5 5 5" />
      <path d="M7 10l5-5 5 5" opacity=".45" />
    </svg>
  );
}

/** The Files half of the mode toggle (#3036). */
export function FilesModeIcon({ size = 13 }: IconProps) {
  return <DocIcon size={size} />;
}

/** The Structure half of the mode toggle (#3036). */
export function StructureModeIcon({ size = 13 }: IconProps) {
  return <KnotIcon size={size} />;
}

/** brink.toml's pinned-row icon (#3042). */
export function GearIcon({ size = 13 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1 1.55V21a2 2 0 1 1-4 0v-.09a1.7 1.7 0 0 0-1-1.55 1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.7 1.7 0 0 0 .34-1.87 1.7 1.7 0 0 0-1.55-1H3a2 2 0 1 1 0-4h.09a1.7 1.7 0 0 0 1.55-1 1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.7 1.7 0 0 0 1.87.34 1.7 1.7 0 0 0 1-1.55V3a2 2 0 1 1 4 0v.09a1.7 1.7 0 0 0 1 1.55 1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.7 1.7 0 0 0-.34 1.87 1.7 1.7 0 0 0 1.55 1H21a2 2 0 1 1 0 4h-.09a1.7 1.7 0 0 0-1.55 1z" />
    </svg>
  );
}

/** Diagnostics marks (#3041). */
export function WarningMarkIcon({ size = 11 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z" />
    </svg>
  );
}

export function ErrorMarkIcon({ size = 10 }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden focusable={false}>
      <circle cx="12" cy="12" r="9" />
    </svg>
  );
}

/** The binder search toggle (#3040). */
export function SearchIcon({ size = 14 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <circle cx="11" cy="11" r="7" />
      <path d="M21 21l-4.3-4.3" />
    </svg>
  );
}

/** Row-actions menu (the mockup's ⋯). */
export function DotsIcon({ size = 13 }: IconProps) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden focusable={false}>
      <circle cx="5" cy="12" r="1.7" />
      <circle cx="12" cy="12" r="1.7" />
      <circle cx="19" cy="12" r="1.7" />
    </svg>
  );
}

/** Structure-mode creation (+knot): a smaller diamond so the plus reads
 *  at row size (maintainer: the + was too small). */
export function KnotPlusIcon({ size = 13 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M9 9l4 5.5-4 5.5-4-5.5z" />
      <path d="M17 4v8M13 8h8" />
    </svg>
  );
}

/** Structure-mode creation (+stitch). */
export function StitchPlusIcon({ size = 13 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M4 9v6a4 4 0 0 0 4 4h5" />
      <path d="M17 4v8M13 8h8" />
    </svg>
  );
}

/** Funnel — FILTERS a list (as opposed to SearchIcon, which searches the
 *  project). Used by the Problems panel's header. */
export function FilterIcon({ size = 13 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M21 4H3l7 8.4V19l4 2v-8.6L21 4z" />
    </svg>
  );
}

/** Group-by-file toggle (Problems panel header). */
export function GroupByFileIcon({ size = 13 }: IconProps) {
  return (
    <svg {...strokeProps(size)}>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </svg>
  );
}
