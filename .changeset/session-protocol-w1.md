---
"@brink-lang/editor": patch
---

Session protocol substrate (editor worker architecture W1, `docs/editor-worker-spec.md`): new `SessionClient` async facade, `SessionTransport` interface, `AdmissionScheduler` (mutations before queries, interactive before background, coalesce-key supersession, staleness drops), and `LocalTransport` — an in-process transport that enforces the protocol's JSON-safety contract on every envelope. No existing behavior changes; consumers migrate onto the client in later waves. Wire shapes are mirrored from the Rust source of truth (`brink-web`'s `protocol` module) with cross-language golden pins.
