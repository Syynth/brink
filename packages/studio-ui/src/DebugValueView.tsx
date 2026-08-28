/**
 * Rendering for `DebugValue` — the structured runtime value the debugger's
 * locals carry (#3140, consuming D7/#3185).
 *
 * **Structured on purpose, and this is the component that spends it.**
 * `DebugGlobal.value` is a display string, so the Globals table can only
 * ever print what the runtime already flattened — `"[list]"` tells an
 * author nothing about which members are in it. Locals arrive as a tagged
 * union instead, and rendering them back into one string would throw away
 * exactly the thing D7 added. So a list shows its members, a struct shows
 * its fields, and a handle shows what it points at.
 *
 * Kept deliberately shallow: this is a *state view*, read at a glance while
 * a story runs, not an object inspector. Structs render one level of
 * fields; a field that is itself a struct renders as its own name rather
 * than expanding forever.
 */

import type { DebugValue } from "@brink/wasm-types";

/** How many list members to print before summarising the rest. */
const MAX_MEMBERS = 6;

export function DebugValueView({ value, depth = 0 }: { value: DebugValue; depth?: number }) {
  switch (value.type) {
    case "int":
    case "float":
      return <span className="sv-v-num">{value.value}</span>;

    case "bool":
      return <span className="sv-v-bool">{value.value ? "true" : "false"}</span>;

    case "string":
      // Quoted, so an empty string and a missing value are distinguishable —
      // the difference an author is usually chasing when they look here.
      return <span className="sv-v-str">&ldquo;{value.value}&rdquo;</span>;

    case "null":
      return <span className="sv-v-null">null</span>;

    case "list": {
      if (value.members.length === 0) {
        // NOT "null" and not blank: an empty list is a list, and ink's
        // list semantics make "empty" a meaningful state rather than an
        // absence.
        return <span className="sv-v-null">(empty list)</span>;
      }
      const shown = value.members.slice(0, MAX_MEMBERS);
      const rest = value.members.length - shown.length;
      return (
        <span className="sv-v-list">
          {shown.map((m, i) => (
            // eslint-disable-next-line react/no-array-index-key -- members are plain strings, order is the identity
            <span key={i} className="sv-v-member">
              {m}
            </span>
          ))}
          {rest > 0 && <span className="sv-dim">+{rest} more</span>}
        </span>
      );
    }

    case "divertTarget":
      return <span className="sv-v-divert">→ {value.path ?? "?"}</span>;

    case "struct": {
      const label = value.name ?? "struct";
      // One level only. A nested struct prints its name; chasing it is what
      // the (future) debugger's inspector is for, not a state snapshot.
      if (depth > 0) {
        return <span className="sv-v-struct-name">{label}</span>;
      }
      return (
        <span className="sv-v-struct">
          <span className="sv-v-struct-name">{label}</span>
          {value.fields.map((f) => (
            <span key={f.name} className="sv-v-field">
              <span className="sv-v-field-name">{f.name}</span>
              <DebugValueView value={f.value} depth={depth + 1} />
            </span>
          ))}
        </span>
      );
    }

    case "handle":
      // `id` is a decimal STRING on the wire, deliberately — a full-range
      // host token id would lose precision as a JS number. Printed as
      // given; never parsed.
      return (
        <span className="sv-v-handle">
          {value.kind}#{value.id}
        </span>
      );

    case "other":
      // The runtime's own display string, for kinds the union does not
      // model (closures, maps, ranges…). Same shape Globals has always
      // shown, so this is a floor rather than a regression.
      return <span className="sv-v-other">{value.display}</span>;

    default: {
      // A kind added to the wire union that this component has not learned
      // yet. Print something rather than nothing: a blank cell reads as
      // "this local has no value", which is a different and wrong claim.
      const exhaustive: never = value;
      void exhaustive;
      return <span className="sv-v-other">?</span>;
    }
  }
}
