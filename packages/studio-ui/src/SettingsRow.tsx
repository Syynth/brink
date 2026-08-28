/**
 * One settings row — the shape every section uses (#3174).
 *
 * Title and description on the left, control on the right, a rule between
 * rows. That is Zed's row, and the reason to copy it is that it reads at a
 * glance: the eye runs down the left edge for *what*, and down the right
 * edge for *what it is set to*. The previous `settings-field` was a bare
 * flex row with the label and control adjacent and no room for a
 * description, so every explanation had to become a paragraph above the
 * group instead of belonging to the setting it explained.
 *
 * `indent` is for a row whose meaning depends on the one above it — Zed
 * indents Theme Name under Theme Mode for exactly this. Use it when the row
 * is genuinely subordinate, not to decorate a group.
 */

import type { ReactNode } from "react";

export function SettingsRow({
  title,
  description,
  htmlFor,
  indent = false,
  children,
}: {
  title: ReactNode;
  description?: ReactNode;
  /** Makes the title a real `<label>` for the control it names. */
  htmlFor?: string;
  indent?: boolean;
  children: ReactNode;
}) {
  const Title = htmlFor === undefined ? "span" : "label";
  return (
    <div className={"settings-row" + (indent ? " indented" : "")}>
      <div className="settings-row-text">
        <Title className="settings-row-title" htmlFor={htmlFor}>
          {title}
        </Title>
        {description !== undefined && (
          <span className="settings-row-desc">{description}</span>
        )}
      </div>
      <div className="settings-row-control">{children}</div>
    </div>
  );
}

/**
 * A bounded integer with − / + on either side.
 *
 * A stepper rather than a free number field because every numeric setting
 * here has real bounds (indent 1–16, font sizes their own): a text input
 * would let an author type a value the setting then silently clamps or
 * refuses, which is the class of silent no-op this surface exists to avoid.
 */
export function SettingsStepper({
  value,
  min,
  max,
  onChange,
  label,
  suffix,
}: {
  value: number;
  min: number;
  max: number;
  onChange: (next: number) => void;
  label: string;
  suffix?: string;
}) {
  const clamp = (n: number): number => Math.min(max, Math.max(min, n));
  return (
    <div className="settings-stepper">
      <button
        type="button"
        aria-label={`Decrease ${label}`}
        disabled={value <= min}
        onClick={() => onChange(clamp(value - 1))}
      >
        &minus;
      </button>
      <span className="settings-stepper-value">
        {value}
        {suffix !== undefined && <span className="settings-stepper-suffix">{suffix}</span>}
      </span>
      <button
        type="button"
        aria-label={`Increase ${label}`}
        disabled={value >= max}
        onClick={() => onChange(clamp(value + 1))}
      >
        +
      </button>
    </div>
  );
}

/** A row group — a small mono heading over a run of rows. */
export function SettingsGroup({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="settings-group">
      <div className="settings-group-label">{title}</div>
      <div className="settings-rows">{children}</div>
    </div>
  );
}

/**
 * A boolean, as a switch rather than a native checkbox.
 *
 * The native box is 13px of OS chrome that ignores every token in the theme
 * — beside a 26px stepper and a 26px select it reads as an unstyled hole in
 * the row. This is the same input element underneath (`peer`-styled), so it
 * keeps the label association, the focus ring and the keyboard behaviour.
 */
export function SettingsToggle({
  id,
  checked,
  onChange,
  label,
}: {
  id?: string;
  checked: boolean;
  onChange: (next: boolean) => void;
  label?: string;
}) {
  return (
    <span className="settings-toggle">
      <input
        id={id}
        type="checkbox"
        checked={checked}
        aria-label={label}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span className="settings-toggle-track" aria-hidden />
    </span>
  );
}
