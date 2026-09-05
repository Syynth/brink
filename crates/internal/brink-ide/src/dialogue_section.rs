//! The `[dialogue]` section of `brink.toml` as one owned block (#3410) —
//! the Rust home of `@brink/studio-store`'s `dialogue-section.ts`, held to
//! the same tests so a section one studio writes reads as its own in the
//! other.
//!
//! Key-level edits (`brink_project_config::edit`) cannot write
//! `[[dialogue.elements]]`, an array of tables — and the Conventions editor
//! owns the whole `[dialogue]` table anyway: it is the resolution of what
//! the author taught, not a set of independent keys. So this road rewrites
//! the SECTION: from the `[dialogue]` header through every `[dialogue.*]` /
//! `[[dialogue.*]]` sub-table that follows, and nothing else — every other
//! byte of the file survives untouched.
//!
//! Round-trip rule (#3392: hand edits and the UI must round-trip): the
//! editor stamps the section with a marker carrying a hash of its body. A
//! section with no marker was written by hand; one whose hash no longer
//! matches was written by the editor and edited since. Both are `owner`
//! values the UI must ASK about before replacing — never overwrite a
//! hand-edited section silently.

use std::sync::LazyLock;

use brink_project_config::DialogueConfig;
use regex::Regex;

/// The marker the editor stamps above its section.
pub const CONVENTIONS_MARKER: &str = "# conventions-editor:";

/// What to write: the table form, or the file form pointing at an
/// artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogueSpec {
    Table(DialogueConfig),
    File(String),
}

/// Who last wrote the section, as far as the marker can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionOwner {
    /// Stamped, and the body still hashes to the stamp.
    Editor,
    /// No marker: written by hand.
    Hand,
    /// Stamped, but edited since.
    Edited,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogueSection {
    /// Line range `[start, end)` of the whole section, marker line included.
    pub start: usize,
    pub end: usize,
    /// The section's text, marker line included, without a trailing newline.
    pub text: String,
    pub owner: SectionOwner,
}

/// A TOML basic string: quotes and backslashes escaped.
#[must_use]
pub fn toml_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// FNV-1a over the section body (everything but the marker line), with
/// trailing whitespace per line ignored so an editor's trim never counts as
/// an edit. Over code points, so the web studio's stamp and this one agree.
#[must_use]
pub fn section_hash(body: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for line in body.split('\n') {
        for ch in line.trim_end().chars().chain(std::iter::once('\n')) {
            h ^= ch as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    format!("{h:08x}")
}

fn marker_line(hash: &str) -> String {
    format!(
        "{CONVENTIONS_MARKER} {hash} \u{2014} written by Settings \u{203a} Conventions. Edit freely; the editor asks before rewriting this section."
    )
}

/// Render `spec` as the section text, marker included, no trailing newline.
#[must_use]
pub fn render_dialogue_section(spec: &DialogueSpec) -> String {
    let mut lines: Vec<String> = vec!["[dialogue]".to_owned()];
    match spec {
        DialogueSpec::File(file) => lines.push(format!("file = {}", toml_string(file))),
        DialogueSpec::Table(t) => {
            if let Some(preset) = &t.preset {
                lines.push(format!("preset = {}", toml_string(preset)));
            }
            if !t.run_ends_at.is_empty() {
                let items: Vec<String> = t.run_ends_at.iter().map(|s| toml_string(s)).collect();
                lines.push(format!("run-ends-at = [{}]", items.join(", ")));
            }
            for el in &t.elements {
                lines.push(String::new());
                lines.push("[[dialogue.elements]]".to_owned());
                lines.push(format!("kind = {}", toml_string(&el.kind)));
                if let Some(nature) = &el.nature {
                    lines.push(format!("nature = {}", toml_string(nature)));
                }
                if let Some(prefix) = &el.prefix {
                    lines.push(format!("prefix = {}", toml_string(prefix)));
                }
                if let Some(suffix) = &el.suffix {
                    lines.push(format!("suffix = {}", toml_string(suffix)));
                }
                if el.glued == Some(true) {
                    lines.push("glued = true".to_owned());
                }
                if let Some(role) = &el.content_role {
                    lines.push(format!("content-role = {}", toml_string(role)));
                }
                if let Some(pattern) = &el.pattern {
                    lines.push(format!("pattern = {}", toml_string(pattern)));
                }
                if let Some(template) = &el.template {
                    lines.push(format!("template = {}", toml_string(template)));
                }
            }
        }
    }
    let body = lines.join("\n");
    format!("{}\n{body}", marker_line(&section_hash(&body)))
}

#[expect(clippy::expect_used, reason = "a fixed pattern the tests compile")]
static HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*\[\[?\s*([^\]]*?)\s*\]\]?\s*(?:#.*)?$").expect("a fixed pattern")
});
#[expect(clippy::expect_used, reason = "a fixed pattern the tests compile")]
static STAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*# conventions-editor:\s*([0-9a-f]{8})\b").expect("a fixed pattern")
});

fn header_name(line: &str) -> Option<&str> {
    HEADER
        .captures(line)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
}

fn belongs_to_dialogue(line: &str) -> bool {
    header_name(line).is_some_and(|n| n == "dialogue" || n.starts_with("dialogue."))
}

/// Find the `[dialogue]` section, or `None` when the file has none.
#[must_use]
pub fn find_dialogue_section(source: &str) -> Option<DialogueSection> {
    let lines: Vec<&str> = source.split('\n').collect();
    let header_at = lines
        .iter()
        .position(|l| header_name(l) == Some("dialogue"))?;
    let mut end = header_at + 1;
    while end < lines.len()
        && (header_name(lines[end]).is_none() || belongs_to_dialogue(lines[end]))
    {
        end += 1;
    }
    // Blank lines between the section and the next table stay with the
    // file.
    while end > header_at + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    let above = if header_at > 0 {
        lines[header_at - 1]
    } else {
        ""
    };
    let marked = above.trim_start().starts_with(CONVENTIONS_MARKER);
    let start = if marked { header_at - 1 } else { header_at };
    let text = lines[start..end].join("\n");
    let owner = if marked {
        let stamped = STAMP
            .captures(above)
            .and_then(|c| c.get(1))
            .map_or("", |m| m.as_str());
        let body = lines[header_at..end].join("\n");
        if stamped == section_hash(&body) {
            SectionOwner::Editor
        } else {
            SectionOwner::Edited
        }
    } else {
        SectionOwner::Hand
    };
    Some(DialogueSection {
        start,
        end,
        text,
        owner,
    })
}

/// Replace the `[dialogue]` section with `block` (a rendered section, or
/// `None` to remove it). Absent → appended after a blank line. Every byte
/// outside the section is preserved.
#[must_use]
pub fn set_dialogue_section(source: &str, block: Option<&str>) -> String {
    let lines: Vec<&str> = source.split('\n').collect();
    let Some(found) = find_dialogue_section(source) else {
        let Some(block) = block else {
            return source.to_owned();
        };
        let trimmed = source.trim_end_matches('\n');
        return if trimmed.is_empty() {
            format!("{block}\n")
        } else {
            format!("{trimmed}\n\n{block}\n")
        };
    };
    let before = &lines[..found.start];
    let mut after: &[&str] = &lines[found.end..];
    let Some(block) = block else {
        // Take one separating blank line with the section, not two.
        if before.last().is_some_and(|l| l.trim().is_empty())
            && after.first().is_some_and(|l| l.trim().is_empty())
        {
            after = &after[1..];
        }
        return before
            .iter()
            .chain(after.iter())
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
    };
    let mut out: Vec<&str> = before.to_vec();
    out.extend(block.split('\n'));
    out.extend(after.iter().copied());
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_project_config::DialogueElementConfig;

    const OTHER: &str = "# My story's config\n[project]\nentry = \"main.ink\" # the entry\n\n[lints]\nE063 = \"allow\"\n";

    fn table() -> String {
        render_dialogue_section(&DialogueSpec::Table(DialogueConfig {
            preset: Some("at-cue".to_owned()),
            file: None,
            run_ends_at: vec!["action".to_owned(), "choices".to_owned()],
            elements: vec![
                DialogueElementConfig {
                    kind: "character".to_owned(),
                    prefix: Some("@".to_owned()),
                    suffix: Some(": ".to_owned()),
                    glued: Some(true),
                    content_role: Some("speaker".to_owned()),
                    ..DialogueElementConfig::default()
                },
                DialogueElementConfig {
                    kind: "action".to_owned(),
                    prefix: Some("> ".to_owned()),
                    ..DialogueElementConfig::default()
                },
            ],
        }))
    }

    #[test]
    fn renders_the_table_form_with_the_marker_first() {
        let t = table();
        assert!(
            t.lines()
                .next()
                .is_some_and(|l| l.starts_with(CONVENTIONS_MARKER))
        );
        assert!(t.contains(
            "[dialogue]\npreset = \"at-cue\"\nrun-ends-at = [\"action\", \"choices\"]\n\n[[dialogue.elements]]\nkind = \"character\"\nprefix = \"@\"\nsuffix = \": \"\nglued = true\ncontent-role = \"speaker\"\n\n[[dialogue.elements]]\nkind = \"action\"\nprefix = \"> \""
        ), "{t}");
        assert!(!t.ends_with('\n'));
    }

    #[test]
    fn renders_the_file_form_and_escapes() {
        let s = render_dialogue_section(&DialogueSpec::File("dialect.json".to_owned()));
        let rest: Vec<&str> = s.lines().skip(1).collect();
        assert_eq!(rest, ["[dialogue]", "file = \"dialect.json\""]);
        let q = render_dialogue_section(&DialogueSpec::Table(DialogueConfig {
            elements: vec![DialogueElementConfig {
                kind: "q".to_owned(),
                prefix: Some("\"".to_owned()),
                suffix: Some("\\".to_owned()),
                ..DialogueElementConfig::default()
            }],
            ..DialogueConfig::default()
        }));
        assert!(q.contains("prefix = \"\\\"\""), "{q}");
        assert!(q.contains("suffix = \"\\\\\""), "{q}");
    }

    #[test]
    fn appends_and_finds_back_as_the_editors() {
        let t = table();
        let out = set_dialogue_section(OTHER, Some(&t));
        assert_eq!(out, format!("{OTHER}\n{t}\n"));
        let found = find_dialogue_section(&out).expect("a section");
        assert_eq!(found.owner, SectionOwner::Editor);
        assert_eq!(found.text, t);
    }

    #[test]
    fn replaces_only_the_section() {
        let t = table();
        let with = format!("{OTHER}\n{t}\n\n[player]\nfast = true # keep\n");
        let next = render_dialogue_section(&DialogueSpec::Table(DialogueConfig {
            preset: Some("at-cue".to_owned()),
            ..DialogueConfig::default()
        }));
        assert_eq!(
            set_dialogue_section(&with, Some(&next)),
            format!("{OTHER}\n{next}\n\n[player]\nfast = true # keep\n")
        );
    }

    #[test]
    fn removes_the_section_and_one_blank_line() {
        let t = table();
        let with = format!("{OTHER}\n{t}\n\n[player]\nfast = true\n");
        assert_eq!(
            set_dialogue_section(&with, None),
            format!("{OTHER}\n[player]\nfast = true\n")
        );
        assert_eq!(set_dialogue_section(OTHER, None), OTHER);
    }

    #[test]
    fn owners_hand_edited_and_trailing_whitespace() {
        let src = format!(
            "{OTHER}\n[dialogue]\npreset = \"at-cue\"\n\n[[dialogue.elements]]\nkind = \"action\"\nprefix = \">\"\n\n[player]\nx = 1\n"
        );
        let found = find_dialogue_section(&src).expect("a section");
        assert_eq!(found.owner, SectionOwner::Hand);
        assert_eq!(
            found.text,
            "[dialogue]\npreset = \"at-cue\"\n\n[[dialogue.elements]]\nkind = \"action\"\nprefix = \">\""
        );
        let t = table();
        let edited =
            set_dialogue_section(OTHER, Some(&t)).replace("prefix = \"> \"", "prefix = \">> \"");
        assert_eq!(
            find_dialogue_section(&edited).map(|s| s.owner),
            Some(SectionOwner::Edited)
        );
        let trimmed =
            set_dialogue_section(OTHER, Some(&t)).replace("prefix = \"@\"", "prefix = \"@\"   ");
        assert_eq!(
            find_dialogue_section(&trimmed).map(|s| s.owner),
            Some(SectionOwner::Editor)
        );
    }

    #[test]
    fn a_sub_table_of_another_table_ends_the_section() {
        let src = "[dialogue]\npreset = \"at-cue\"\n[dialogue.extra]\nk = 1\n[dialogues]\nz = 2\n";
        let found = find_dialogue_section(src).expect("a section");
        assert_eq!(
            found.text,
            "[dialogue]\npreset = \"at-cue\"\n[dialogue.extra]\nk = 1"
        );
    }

    #[test]
    fn the_hash_matches_the_web_studios_stamp() {
        // The TypeScript FNV-1a over code points, for an ASCII body.
        assert_eq!(
            section_hash("[dialogue]\npreset = \"at-cue\""),
            section_hash("[dialogue]  \npreset = \"at-cue\"  ")
        );
        assert_ne!(section_hash("a"), section_hash("b"));
        assert_eq!(section_hash("").len(), 8);
    }
}
