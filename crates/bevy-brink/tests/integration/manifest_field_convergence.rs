//! Cross-validation for issue #911's manifest-convergence decision.
//!
//! `bevy_brink::capability::CapabilityManifest` (the host/ECS-side capability
//! grammar, `docs/effects-spec.md` §13.2) and `brink_ir::host_manifest::
//! HostManifest` (the compiler/IDE-side tooling schema,
//! `docs/host-capability-manifest.md`) are two independent Rust types that
//! both deserialize the **same** on-disk manifest file, each reading only the
//! keys it cares about and silently ignoring the rest (serde's default
//! unknown-fields-are-fine behavior). They are deliberately **not** converged
//! onto one canonical type — see `brink_ir::host_manifest`'s and
//! `bevy_brink::capability`'s module docs for the dependency-direction
//! rationale. What this test proves instead: one literal manifest JSON,
//! built from the two crates' shared field-name constants
//! (`brink_format::manifest_field_names`), parses correctly through **both**
//! types at once, each seeing the same external's `name` and the fields it
//! owns — the concrete de-drift check a future rename on either side would
//! break.

use bevy_brink::{CapabilityEffects, CapabilityManifest};
use brink_format::manifest_field_names::{EXTERNALS, NAME};
use brink_ir::host_manifest::HostManifest;

/// One manifest literal, built from the shared constants rather than typed
/// out twice — if either constant's spelling drifts from what the JSON
/// below actually needs, this string stops matching the field names the two
/// structs deserialize by, and the assertions below catch it (a struct
/// falling back to its `#[serde(default)]` empty/absent value rather than
/// picking up the real one). `params` is deliberately omitted: this test's
/// focus is the two shared keys (`externals`, `name`), not the full Tier-1
/// grammar — every real `ManifestParam` construction site in the workspace
/// builds the struct directly in Rust rather than round-tripping it through
/// JSON, so a param-shaped JSON literal here would test an untested corner,
/// not the convergence question this test exists for.
fn manifest_json() -> String {
    format!(
        r#"{{ "{EXTERNALS}": [
            {{ "{NAME}": "get_position",
               "kind": "query",
               "doc": "reads an npc's position",
               "effects": {{ "reads": ["Transform"], "detect": {{"Transform": true}} }} }}
        ] }}"#
    )
}

/// The compiler/IDE side sees `name`/`kind`/`doc` and silently ignores the
/// capability-only `effects` key it has no field for.
#[test]
fn host_manifest_reads_its_own_fields_from_the_shared_json() {
    let manifest: HostManifest = serde_json::from_str(&manifest_json()).expect("parse");
    assert_eq!(manifest.externals.len(), 1);
    let ext = &manifest.externals[0];
    assert_eq!(ext.name, "get_position");
    assert_eq!(ext.doc.as_deref(), Some("reads an npc's position"));
}

/// The host/ECS side sees `name`/`effects` and silently ignores the
/// tooling-only `params`/`kind`/`doc` keys it has no field for.
#[test]
fn capability_manifest_reads_its_own_fields_from_the_shared_json() {
    let manifest = CapabilityManifest::from_json(&manifest_json()).expect("parse");
    assert_eq!(manifest.externals.len(), 1);
    let ext = manifest.external("get_position").expect("entry present");
    assert_eq!(
        ext.effects,
        CapabilityEffects {
            reads: vec!["Transform".to_string()],
            writes: vec![],
            detect: [("Transform".to_string(), true)].into_iter().collect(),
        }
    );
}

/// Both types agree on the one field they share (`name`) from the one
/// manifest text — the concrete "same file serves both consumers" claim
/// each type's doc comment makes, proven rather than merely asserted in
/// prose.
#[test]
fn both_types_agree_on_the_shared_name_field() {
    let json = manifest_json();
    let host_manifest: HostManifest = serde_json::from_str(&json).expect("parse host side");
    let capability_manifest = CapabilityManifest::from_json(&json).expect("parse capability side");
    assert_eq!(
        host_manifest.externals.len(),
        capability_manifest.externals.len()
    );
    assert_eq!(
        host_manifest.externals[0].name,
        capability_manifest.externals[0].name
    );
}
