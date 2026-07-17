/**
 * Host-registered argument handlers for PRIMITIVE base types (#990). Three
 * deliverables under one issue:
 *
 * 1. Base-type registration — `setHostWidgets([{ type: "bool", … }])` matches
 *    every `bool`-typed slot via `matchHostWidget`'s existing `type_name`
 *    fallback (argument-widget-spec §3.1). Blessed, tested, documented here —
 *    not new matching logic.
 * 2. Form field precedence — `buildField` now checks `hostWidget` before
 *    `values` (color → hostWidget → values → text), so a host widget is never
 *    shadowed by a plain values dropdown over the same type.
 * 3. `editor.surface: "inline"` — a host widget mounts directly in the Form
 *    row (no summary chip + Edit toggle), the right shape for a primitive
 *    control that IS the field.
 *
 * The final suite reproduces the issue's celeris scenario end-to-end through
 * `openCallForm` — the exact function wired to the in-editor call-level glyph
 * and the hover-card "edit arguments" action (see `hover.ts`,
 * `argument-widgets.ts`'s `FormGlyphWidget` / `argumentWidgetsExtension`).
 */

import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import {
  getHostWidget,
  matchHostWidget,
  openArgumentForm,
  openCallForm,
  setHostWidgets,
  type FormField,
} from "@brink-lang/editor";
import type { ArgumentWidget, CallWidgetSite, SlotWidget } from "@brink/wasm-types";

// jsdom has no ResizeObserver; the popover chrome (`widget-popover.ts`)
// observes its panel for reflow. A no-op stub is enough for these DOM-shape
// assertions, which don't depend on live positioning.
beforeAll(() => {
  if (typeof globalThis.ResizeObserver === "undefined") {
    class ResizeObserverStub {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    }
    (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = ResizeObserverStub;
  }
});

afterEach(() => {
  setHostWidgets([]); // registry is mount-time global state — reset between tests
  document.body.replaceChildren();
});

function anchor(): HTMLElement {
  const el = document.createElement("div");
  document.body.appendChild(el);
  return el;
}

function plainWidget(type: string): ArgumentWidget {
  return { type, editor: { render: () => () => {} } };
}

// ── 1. Base-type registration (matchHostWidget) ─────────────────────

describe("base-type host widget registration (#990)", () => {
  it("registers and resolves each base type through the ordinary registry", () => {
    setHostWidgets([
      plainWidget("bool"),
      plainWidget("int"),
      plainWidget("float"),
      plainWidget("string"),
    ]);
    expect(getHostWidget("bool")?.type).toBe("bool");
    expect(getHostWidget("int")?.type).toBe("int");
    expect(getHostWidget("float")?.type).toBe("float");
    expect(getHostWidget("string")?.type).toBe("string");
  });

  it("matchHostWidget falls back from an untyped slot's type_name to a base-type registration", () => {
    setHostWidgets([plainWidget("bool")]);
    const slot: SlotWidget = {
      param_name: "add",
      type_name: "bool",
      state: { kind: "filled", start: 0, end: 4, value: "true" },
    };
    expect(matchHostWidget(slot)?.type).toBe("bool");
  });

  it("a slot's declared widget kind still wins over the type_name fallback", () => {
    setHostWidgets([plainWidget("bool"), plainWidget("host.acme.special_bool")]);
    const slot: SlotWidget = {
      param_name: "add",
      widget: "host.acme.special_bool",
      type_name: "bool",
      state: { kind: "empty", insert_at: 0, needs_leading_comma: false },
    };
    expect(matchHostWidget(slot)?.type).toBe("host.acme.special_bool");
  });

  it("no registration for a base type means no match — plain literal editing", () => {
    setHostWidgets([]);
    const slot: SlotWidget = {
      param_name: "name",
      type_name: "string",
      state: { kind: "filled", start: 0, end: 5, value: "hero" },
    };
    expect(matchHostWidget(slot)).toBeUndefined();
  });

  it("an untyped slot (no type_name) never matches, even with a matching kind registered", () => {
    setHostWidgets([plainWidget("bool")]);
    const slot: SlotWidget = {
      param_name: "mystery",
      state: { kind: "expr" },
    };
    expect(matchHostWidget(slot)).toBeUndefined();
  });
});

// ── 2. Form field precedence: color → hostWidget → values → text ───

describe("Form field precedence — color > hostWidget > values > text (#990)", () => {
  function pickerWidget(): ArgumentWidget {
    return {
      type: "actor_id",
      editor: {
        render: (_ctx, _host, container) => {
          const btn = document.createElement("button");
          btn.className = "celeris-actor-picker";
          container.appendChild(btn);
          return () => {};
        },
      },
    };
  }

  it("a host widget outranks a values dropdown on the same field", () => {
    const fields: FormField[] = [
      {
        paramName: "actor",
        typeName: "actor_id",
        values: [{ value: "1", label: "Hero" }],
        hostWidget: pickerWidget(),
      },
    ];
    openArgumentForm(anchor(), { fields, onApply: () => {}, onCancel: () => {} });

    expect(document.querySelector(".brink-arg-form-select")).toBeNull();
    expect(document.querySelector(".brink-arg-form-host")).not.toBeNull();
  });

  it("a values-only field (no hostWidget) is unaffected — still the dropdown", () => {
    const fields: FormField[] = [
      { paramName: "actor", typeName: "actor_id", values: [{ value: "1", label: "Hero" }] },
    ];
    openArgumentForm(anchor(), { fields, onApply: () => {}, onCancel: () => {} });

    expect(document.querySelector(".brink-arg-form-select")).not.toBeNull();
    expect(document.querySelector(".brink-arg-form-host")).toBeNull();
  });

  it("color still wins over a hostWidget declared on the same field", () => {
    const fields: FormField[] = [
      {
        paramName: "tint",
        typeName: "hex_color",
        widgetKind: "color",
        hostWidget: pickerWidget(),
      },
    ];
    openArgumentForm(anchor(), { fields, onApply: () => {}, onCancel: () => {} });

    expect(document.querySelector(".brink-arg-form-picker")).not.toBeNull();
    expect(document.querySelector(".brink-arg-form-host")).toBeNull();
  });

  it("a field with neither values nor hostWidget still falls back to text", () => {
    const fields: FormField[] = [{ paramName: "label", typeName: "string" }];
    openArgumentForm(anchor(), { fields, onApply: () => {}, onCancel: () => {} });

    expect(document.querySelector(".brink-arg-form-input")).not.toBeNull();
  });
});

// ── 3. editor.surface: "inline" mount ───────────────────────────────

describe("editor.surface: 'inline' mount (#990)", () => {
  function toggleWidget(): ArgumentWidget {
    return {
      type: "bool",
      editor: {
        surface: "inline",
        render: (ctx, host, container) => {
          const toggle = document.createElement("button");
          toggle.className = "celeris-bool-toggle";
          toggle.textContent = ctx.values[0] === "true" ? "On" : "Off";
          toggle.addEventListener("click", () => {
            const next = toggle.textContent === "On" ? "false" : "true";
            toggle.textContent = next === "true" ? "On" : "Off";
            host.resolve([next]);
          });
          container.appendChild(toggle);
          return () => {};
        },
      },
    };
  }

  it("mounts the control directly in the row — no summary chip, no Edit button", () => {
    const fields: FormField[] = [
      { paramName: "add", typeName: "bool", hostWidget: toggleWidget(), initial: "true" },
    ];
    openArgumentForm(anchor(), { fields, onApply: () => {}, onCancel: () => {} });

    expect(document.querySelector(".brink-arg-form-host")).toBeNull();
    expect(document.querySelector(".brink-arg-form-edit")).toBeNull();
    const toggle = document.querySelector<HTMLButtonElement>(".celeris-bool-toggle");
    expect(toggle).not.toBeNull();
    expect(toggle?.textContent).toBe("On");
  });

  it("resolving the inline control updates the draft Apply writes back", () => {
    const fields: FormField[] = [
      { paramName: "add", typeName: "bool", hostWidget: toggleWidget(), initial: "true" },
    ];
    let applied: string[] = [];
    openArgumentForm(anchor(), {
      fields,
      onApply: (literals) => {
        applied = literals;
      },
      onCancel: () => {},
    });

    document.querySelector<HTMLButtonElement>(".celeris-bool-toggle")?.click();
    document.querySelector<HTMLButtonElement>(".brink-arg-form-btn-primary")?.click();

    expect(applied).toEqual(["false"]);
  });

  it("without 'inline', the same widget still gets the chip + Edit chrome (unchanged)", () => {
    const widget: ArgumentWidget = {
      type: "region_id",
      editor: { render: () => () => {} },
    };
    const fields: FormField[] = [{ paramName: "region", typeName: "region_id", hostWidget: widget }];
    openArgumentForm(anchor(), { fields, onApply: () => {}, onCancel: () => {} });

    expect(document.querySelector(".brink-arg-form-host")).not.toBeNull();
    expect(document.querySelector(".brink-arg-form-edit")).not.toBeNull();
  });
});

// ── Integration: the celeris scenario (actor dropdown + bool toggle) ─

describe("celeris scenario integration: change_party_member(actor, add) (#990)", () => {
  /** A fake EditorView whose doc slices deterministically — only `state.doc`
   *  is read by `openCallForm` for a filled slot's raw literal text. */
  function fakeView(doc: string): EditorView {
    return { state: EditorState.create({ doc }) } as unknown as EditorView;
  }

  it("actor keeps its values dropdown; add gets celeris's registered bool toggle inline", () => {
    // No host widget for `actor_id` — it resolves purely from `values` (#174),
    // exactly as described in the issue. Only `bool` is host-registered.
    setHostWidgets([
      {
        type: "bool",
        editor: {
          surface: "inline",
          render: (ctx, _host, container) => {
            const toggle = document.createElement("button");
            toggle.className = "celeris-bool-toggle";
            toggle.textContent = ctx.values[0] === "true" ? "On" : "Off";
            container.appendChild(toggle);
            return () => {};
          },
        },
      },
    ]);

    // Doc text is irrelevant content-wise — only used so `add`'s filled slot
    // (offsets [10, 14)) slices back to the literal "true".
    const doc = "..........true...";
    const view = fakeView(doc);

    const site: CallWidgetSite = {
      callee: "change_party_member",
      name_start: 0,
      name_end: 20,
      groups: [],
      slots: [
        {
          param_name: "actor",
          type_name: "actor_id",
          values: [
            { value: "1", label: "Hero" },
            { value: "2", label: "Villain" },
          ],
          state: { kind: "empty", insert_at: 30, needs_leading_comma: false },
        },
        {
          param_name: "add",
          type_name: "bool",
          state: { kind: "filled", start: 10, end: 14, value: "true" },
        },
      ],
    };

    openCallForm(anchor(), site, view);

    // actor: still a plain values dropdown (no host widget registered for it).
    const select = document.querySelector<HTMLSelectElement>(".brink-arg-form-select");
    expect(select).not.toBeNull();
    expect(Array.from(select?.options ?? []).map((o) => o.textContent)).toContain("Hero");

    // add: celeris's bool toggle, mounted inline in the row — not a text input,
    // not the chip+Edit chrome.
    expect(document.querySelector(".brink-arg-form-input")).toBeNull();
    expect(document.querySelector(".brink-arg-form-host")).toBeNull();
    const toggle = document.querySelector<HTMLButtonElement>(".celeris-bool-toggle");
    expect(toggle).not.toBeNull();
    expect(toggle?.textContent).toBe("On");
  });
});
