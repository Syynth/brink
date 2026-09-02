//! `brink.toml [dialogue]` → `DialogueDialect` (RULED 2026-08-30, "Project-
//! declared dialogue dialect lives in brink.toml", #3387).
//!
//! The config crate only parses; THIS is where a declaration becomes the
//! one artifact every consumer reads: the shipped preset (if named) with
//! the overlay elements merged in (`extend_dialect`), each affix-sugar row
//! compiled to its source AND emitted shapes at the one derivation site,
//! or — the escape hatch — a full hand-written JSON artifact read through
//! the caller's file reader (the session's own document tree in brink-web,
//! the filesystem for the CLI). Validation is the dialect's own
//! (`validate` + `ResolvedDialect::compile`), so a project that declares
//! something unusable fails loudly at config-apply time with a readable
//! message, never silently in the Player.

use brink_ir::dialect::{
    AffixShape, DialectElement, DialogueDialect, ElementNature, PRESET_NAMES, PatternShape,
    ResolvedDialect, SourceShape, Templates, affix_element, extend_dialect, preset_by_name,
    validate,
};
use brink_project_config::{DialogueConfig, DialogueElementConfig};

/// Resolve a parsed `[dialogue]` declaration. `read_file` serves the
/// string-form artifact by the path written in the file (relative to
/// `brink.toml`; the caller owns that resolution) — `None` = not found.
///
/// # Errors
/// A human-readable message naming what was wrong: an unknown preset, a
/// missing/invalid artifact file, an element that is neither affix- nor
/// pattern-shaped, or a resolved dialect that fails the artifact's own
/// validation.
pub fn resolve_dialogue_config(
    config: &DialogueConfig,
    read_file: &dyn Fn(&str) -> Option<String>,
) -> Result<DialogueDialect, String> {
    if let Some(file) = &config.file {
        if config.preset.is_some() || !config.elements.is_empty() {
            return Err(format!(
                "`dialogue = \"{file}\"` is the whole artifact — it cannot be combined with \
                 `preset`/`elements` (use the `[dialogue]` table form for a preset plus overlays)"
            ));
        }
        let text = read_file(file)
            .ok_or_else(|| format!("dialogue artifact `{file}` was not found in the project"))?;
        let dialect: DialogueDialect = serde_json::from_str(&text)
            .map_err(|e| format!("dialogue artifact `{file}` is not a valid dialect: {e}"))?;
        return check(dialect);
    }

    let base = match &config.preset {
        Some(name) => preset_by_name(name).ok_or_else(|| {
            format!(
                "unknown dialogue preset `{name}` — shipped presets: {}",
                PRESET_NAMES.join(", ")
            )
        })?,
        None => DialogueDialect {
            version: 1,
            name: "project".to_owned(),
            elements: Vec::new(),
            chain: Vec::new(),
            transitions: Vec::new(),
            templates: Templates::default(),
        },
    };
    let overlay_elements = config
        .elements
        .iter()
        .map(element_from_config)
        .collect::<Result<Vec<_>, _>>()?;
    let overlay = DialogueDialect {
        version: 1,
        name: String::new(),
        elements: overlay_elements,
        chain: Vec::new(),
        transitions: Vec::new(),
        templates: Templates::default(),
    };
    check(extend_dialect(&base, &overlay))
}

fn check(dialect: DialogueDialect) -> Result<DialogueDialect, String> {
    validate(&dialect).map_err(|errs| {
        let parts: Vec<String> = errs.iter().map(|e| format!("{e:?}")).collect();
        format!("dialogue dialect is invalid: {}", parts.join("; "))
    })?;
    ResolvedDialect::compile(&dialect)
        .map_err(|e| format!("dialogue dialect is invalid: {e:?}"))?;
    Ok(dialect)
}

fn element_from_config(el: &DialogueElementConfig) -> Result<DialectElement, String> {
    let nature = match el.nature.as_deref() {
        None | Some("narrative") => ElementNature::Narrative,
        Some("machinery") => ElementNature::Machinery,
        Some("structural") => ElementNature::Structural,
        Some(other) => {
            return Err(format!(
                "dialogue element `{}`: unknown nature `{other}` (expected narrative, \
                 machinery, or structural)",
                el.kind
            ));
        }
    };
    let role = el
        .content_role
        .clone()
        .unwrap_or_else(|| "content".to_owned());

    // Explicit pattern form — the unusual case the sugar can't express.
    if el.pattern.is_some() || el.template.is_some() {
        let (Some(pattern), Some(template)) = (&el.pattern, &el.template) else {
            return Err(format!(
                "dialogue element `{}`: `pattern` and `template` go together",
                el.kind
            ));
        };
        if el.prefix.is_some() || el.suffix.is_some() || el.glued.is_some() {
            return Err(format!(
                "dialogue element `{}`: use EITHER the affix keys (prefix/suffix/glued) OR \
                 pattern/template, not both",
                el.kind
            ));
        }
        return Ok(DialectElement {
            kind: el.kind.clone(),
            nature,
            source: Some(SourceShape::Pattern(PatternShape {
                pattern: pattern.clone(),
                content_group: Some(role.clone()),
                template_group: None,
                hidden: Vec::new(),
                template: template.clone(),
            })),
            // A pattern-form element gets no emitted shape of its own — the
            // author writes one through the file form when the runtime
            // output differs from the source; source-only kinds are common.
            emitted: None,
            malformed: Vec::new(),
        });
    }

    // Affix form — a chain-only kind (no prefix, no suffix) is legal too:
    // it is matched only by a chain rule, like the preset's `dialogue`.
    if el.prefix.is_none() && el.suffix.is_none() {
        return Ok(DialectElement {
            kind: el.kind.clone(),
            nature,
            source: None,
            emitted: None,
            malformed: Vec::new(),
        });
    }
    Ok(affix_element(
        &el.kind,
        nature,
        AffixShape {
            prefix: el.prefix.clone(),
            suffix: el.suffix.clone(),
            glued: el.glued.unwrap_or(false),
            content_role: role,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_files(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn preset_plus_affix_overlay_resolves_and_validates() {
        let cfg = DialogueConfig {
            preset: Some("at-cue".to_owned()),
            elements: vec![DialogueElementConfig {
                kind: "action".to_owned(),
                prefix: Some(">".to_owned()),
                ..DialogueElementConfig::default()
            }],
            ..DialogueConfig::default()
        };
        let d = resolve_dialogue_config(&cfg, &no_files).expect("resolves");
        assert_eq!(d.name, "at-cue");
        let kinds: Vec<&str> = d.elements.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["character", "parenthetical", "dialogue", "action"]);
        let action = d
            .elements
            .iter()
            .find(|e| e.kind == "action")
            .expect("action");
        assert!(action.emitted.as_ref().is_some_and(|e| e.reserved_prefix));
    }

    #[test]
    fn no_preset_means_a_project_only_dialect() {
        let cfg = DialogueConfig {
            elements: vec![DialogueElementConfig {
                kind: "aside".to_owned(),
                pattern: Some(r"^\[(?<content>[^\]]*)\]$".to_owned()),
                template: Some("[${content}]".to_owned()),
                ..DialogueElementConfig::default()
            }],
            ..DialogueConfig::default()
        };
        let d = resolve_dialogue_config(&cfg, &no_files).expect("resolves");
        assert_eq!(d.name, "project");
        assert_eq!(d.elements.len(), 1);
    }

    #[test]
    fn unknown_preset_and_missing_file_are_readable_errors() {
        let err = resolve_dialogue_config(
            &DialogueConfig {
                preset: Some("fountain".to_owned()),
                ..DialogueConfig::default()
            },
            &no_files,
        )
        .expect_err("unknown preset");
        assert!(err.contains("unknown dialogue preset `fountain`"), "{err}");
        assert!(err.contains("at-cue"), "names the shipped presets: {err}");

        let err = resolve_dialogue_config(
            &DialogueConfig {
                file: Some("dialect.json".to_owned()),
                ..DialogueConfig::default()
            },
            &no_files,
        )
        .expect_err("missing file");
        assert!(err.contains("was not found"), "{err}");
    }

    #[test]
    fn file_form_reads_a_full_artifact_and_refuses_mixing() {
        let json = serde_json::to_string(&brink_ir::dialect::at_cue_preset()).expect("json");
        let read = move |path: &str| (path == "dialect.json").then(|| json.clone());
        let d = resolve_dialogue_config(
            &DialogueConfig {
                file: Some("dialect.json".to_owned()),
                ..DialogueConfig::default()
            },
            &read,
        )
        .expect("resolves from the file");
        assert_eq!(d.name, "at-cue");

        let err = resolve_dialogue_config(
            &DialogueConfig {
                file: Some("dialect.json".to_owned()),
                preset: Some("at-cue".to_owned()),
                ..DialogueConfig::default()
            },
            &read,
        )
        .expect_err("file + preset");
        assert!(err.contains("cannot be combined"), "{err}");
    }

    #[test]
    fn element_shape_errors_name_the_kind() {
        let err = resolve_dialogue_config(
            &DialogueConfig {
                elements: vec![DialogueElementConfig {
                    kind: "weird".to_owned(),
                    nature: Some("cosmic".to_owned()),
                    prefix: Some("~".to_owned()),
                    ..DialogueElementConfig::default()
                }],
                ..DialogueConfig::default()
            },
            &no_files,
        )
        .expect_err("bad nature");
        assert!(err.contains("`weird`") && err.contains("cosmic"), "{err}");
    }
}
