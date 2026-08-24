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
