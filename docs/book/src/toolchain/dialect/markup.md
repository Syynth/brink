# Inline Markup

Markup is an **optional layer** for decorating prose with semantic meaning. A story
author can wrap spans of text in tags to label them with meaning — `<wave>text</wave>`
for visual effects, `<item id="key">thing</item>` to link an object — and the runtime
delivers those tags to the host engine so it can render them however it wants.

Markup is **freeform by default**: an author can use any tags without declaring them,
and the compiler never complains. Optionally, the **host engine** can declare a markup
vocabulary in its **capability manifest** to validate tags and attributes — turning
what was freeform into a tightly-checked surface.

This chapter covers both sides: the syntax story authors write, and how integrators
declare and validate a markup vocabulary.

Inline markup is a **native `.brink`-surface feature**. It is not available on the
ink surface, even with `dialect = "brink"` enabled on an `.ink` source: writing
`<wave>x</wave>` in an `.ink` file produces literal text, with no diagnostic.

## Author syntax

### Basic spans

A span is XML-shaped: a tag with a name, optional attributes, and text content.

```brink
flow scene() {
  He hands you <item id="lantern">the old lantern</item>.
  The sign flickers <wave amount="3">in the corner</wave>.
}
```

Attributes are `name="value"` pairs (the quotes are required). The value is always
literal text — there is no type system or interpolation inside an attribute value.

### Hyphenated tag names

A tag name may contain `-` as a separator between words — useful for kebab-case
vocabularies borrowed from XML/HTML custom elements:

```brink
flow scene() {
  The screen goes dark. <fade-in>A new day begins.</fade-in>
}
```

The hyphen is only legal *between* two name segments — never as the first or
last character (`<-x>` and `<x->` are both errors).

### Self-closing tags

For markers that carry no content — a sound effect, a visual cue — use self-closing
syntax:

```brink
flow scene() {
  The bell tolls again. <sfx name="bell"/> Somewhere a door slams.
}
```

### Nesting

Spans can nest, and they can contain interpolation:

```brink
flow dialogue() {
  var name = "Kestrel"
  <b>Hello, {name}!</b>
}
```

**Important:** A tag must open and close in the same scope. You can't have a tag open
in one branch of a conditional and close in another:

```text
{if tired: <i>yawn</i> else: ready}          // Correct: span is entirely in the branch

<i>hello {if tired: world</i> else: friend}  // Nesting violation
```

This constraint is enforced by the compiler. It exists because markup is
**line-scoped**: the runtime treats each line as a complete unit for translation
purposes, so spans cannot leak across line boundaries either.

### Escaping special characters

Four characters have special meaning in markup and interpolation:

| Character | Escape | When to use |
|-----------|--------|------------|
| `<` | `\<` | When you need a literal `<` (e.g., `\<3`) |
| `{` | `\{` | When you need a literal `{` in prose (e.g., `\{HP: 5}`) |
| `#` | `\#` | When you need a literal `#` as line-start text |
| `\` | `\\` | When you need a literal backslash |

```brink
flow scene() {
  The code is: `\<vector>` to declare a pointer.
  Health: \{HP}
}
```

If you write a backslash followed by anything else — like `\n` or `\t` — the compiler
reports an error. The escape set is **final and small**.

## For integrators: the host manifest

### Why a manifest?

By default, any tag is allowed. This is great for iteration — authors can try new
markup without asking permission. But once a project stabilizes, you probably want
to lock down the vocabulary: "these are the valid tags and their attributes, and
anything else is a mistake."

The host engine declares this vocabulary in its **capability manifest**, the same
place it declares external functions. The compiler then validates tags against the
vocabulary and reports problems.

### Declaring a markup vocabulary

The manifest's `markup` section is an array of span kinds:

```json
{
  "markup": [
    { "name": "wave", "attrs": [{ "name": "amount" }] },
    { "name": "item", "attrs": [{ "name": "id", "required": true }] },
    { "name": "sfx", "attrs": [{ "name": "name" }, { "name": "volume", "required": true }] },
    { "name": "b" },
    { "name": "i" }
  ]
}
```

Each span kind has:

- **`name`** (required) — the tag name
- **`attrs`** (optional) — an array of attribute declarations this kind accepts

Each attribute declaration has:

- **`name`** (required) — the attribute name
- **`required`** (optional, defaults to `false`) — whether every span of this kind
  must carry this attribute

If a span kind has no `attrs`, it accepts no attributes (like the `<b>` and `<i>`
examples above).

### Validation

Once the manifest declares at least one span kind, the compiler validates all markup
in the project:

- An **undeclared tag** (e.g., `<glitch>` when only `wave` is declared) reports
  `E164`.
- An **undeclared attribute** on a declared kind (e.g., `<wave speed="2">` when only
  `amount` is declared) reports `E165`.
- A **missing required attribute** on a declared kind (e.g., `<item>the lantern</item>`
  when `item`'s `id` is declared `required`) reports `E173`.

Attribute **values** are never checked — they are always plain text, so there is
nothing to type-check. Only the attribute *name*, and now whether it is *required*,
is part of the declared vocabulary.

### Freeform markup (no validation)

To go back to freeform markup:

1. Remove the `markup` section from the manifest entirely, *or*
2. Use `[lints]` configuration to suppress the diagnostics:

```toml
[lints]
E164 = "allow"
E165 = "allow"
E173 = "allow"
```

You can also suppress markup validation for a single tag or line:

```brink
@[allow(E164)]
<custom>This tag is not declared, but we allow it.</custom>

// brink-disable E164
<another>Also allowed.</another>
```

### Severity control

`E164`, `E165`, and `E173` all default to **`Warning`** severity, which means they
don't break the build. You can make them stricter:

```toml
[lints]
E164 = "deny"
E165 = "deny"
E173 = "deny"
```

Now undeclared tags, undeclared attributes, and missing required attributes are hard
errors. You can also suppress them selectively with `@[allow(…)]` or line-level
`// brink-disable`.

## How the runtime sees markup

The runtime delivers markup to the host engine through the [`Line` enum](../reference/runtime-api.md).
When the engine receives a line, it can inspect the markup and render it however it
wants — a text-effect system might animate `<wave>`, a dialogue system might color-code
`<speaker>` tags, and so on.

Markup is **presentation only**: it has no effect on game logic or control flow. The
same game state and branching structure exists whether markup is present or not.
