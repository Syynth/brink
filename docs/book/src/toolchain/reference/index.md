# Reference

Look-it-up material for the toolchain. You don't read these front to back —
you jump in when you need the exact opcode, byte layout, or error variant.

- **[Runtime API](./runtime-api.md)** — the `brink-runtime` public surface:
  `link`, `Program`, `Story`, statistics, RNG.
- **[Bytecode & Opcodes](./opcodes.md)** — the full opcode set executed by the VM.
- **[Binary Format](./format.md)** — the `.inkb` / `.inkt` / `.inkl` file layouts.
- **[Containers & DefinitionId](./containers.md)** — the identity scheme and the
  container/address model.
- **[Line Templates](./line-templates.md)** — the localizable line content types
  (slots, selects, plural keys).
- **[Errors](./errors.md)** — every `RuntimeError` variant and what causes it.

For the *concepts* behind these — how the VM steps, why the format is split —
see [Concepts](../concepts/index.md).
