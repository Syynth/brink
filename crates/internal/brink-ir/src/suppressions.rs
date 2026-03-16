//! Diagnostic suppression and expectation directives.
//!
//! Supports:
//! - `// brink-disable-all`  — suppress all diagnostics for the entire project (root file only)
//! - `// brink-disable-file` — suppress all diagnostics in this file
//! - `// brink-disable`      — suppress all diagnostics on the next line
//! - `// brink-disable E027` — suppress specific code(s) on the next line
//! - `// brink-expect E027`  — suppress E027 on next line, error if E027 doesn't appear
//! - `// brink-expect`       — suppress all on next line, error if NO diagnostic appears

use std::collections::BTreeMap;

use rowan::TextRange;

use crate::{Diagnostic, DiagnosticCode, FileId};

/// Parsed suppression/expectation directives for a single file.
#[derive(Debug, Clone, Default)]
pub struct Suppressions {
    /// `// brink-disable-all` found in this file.
    pub disable_all: bool,
    /// `// brink-disable-file` found in this file.
    pub disable_file: bool,
    /// Target line (0-based) → directive. Sorted by line.
    pub line_directives: BTreeMap<u32, LineDirective>,
}

/// A per-line suppression or expectation directive.
#[derive(Debug, Clone)]
pub struct LineDirective {
    /// Whether this is a `disable` or `expect` directive.
    pub kind: DirectiveKind,
    /// `None` = all codes; `Some(vec)` = specific codes.
    pub codes: Option<Vec<DiagnosticCode>>,
    /// Byte range of the directive comment (for "unmet expect" error location).
    pub range: TextRange,
}

/// The kind of a per-line directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectiveKind {
    /// `// brink-disable` — suppress matched diagnostics silently.
    Disable,
    /// `// brink-expect` — suppress matched diagnostics, error if none match.
    Expect,
}

/// Parse suppression/expectation directives from source text.
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "source file line indices and byte offsets fit in u32 for any reasonable file"
)]
pub fn parse_suppressions(source: &str) -> Suppressions {
    let mut result = Suppressions::default();
    let mut byte_offset: u32 = 0;

    for (line_idx, line) in source.lines().enumerate() {
        let line_byte_start = byte_offset;
        // Advance byte_offset past this line + its newline
        byte_offset += line.len() as u32;
        // Account for the newline character(s)
        let rest = &source[byte_offset as usize..];
        if rest.starts_with("\r\n") {
            byte_offset += 2;
        } else if rest.starts_with('\n') || rest.starts_with('\r') {
            byte_offset += 1;
        }

        // Find `//` comment start
        let Some(comment_pos) = line.find("//") else {
            continue;
        };
        let comment = line[comment_pos + 2..].trim();

        if comment == "brink-disable-all" {
            result.disable_all = true;
        } else if comment == "brink-disable-file" {
            result.disable_file = true;
        } else if comment == "brink-disable" || comment == "brink-expect" {
            let kind = if comment == "brink-expect" {
                DirectiveKind::Expect
            } else {
                DirectiveKind::Disable
            };
            let target_line = (line_idx + 1) as u32;
            let comment_byte_start = line_byte_start + comment_pos as u32;
            let comment_byte_end = line_byte_start + line.len() as u32;
            result.line_directives.insert(
                target_line,
                LineDirective {
                    kind,
                    codes: None,
                    range: TextRange::new(comment_byte_start.into(), comment_byte_end.into()),
                },
            );
        } else if let Some(codes_str) = comment
            .strip_prefix("brink-disable ")
            .or_else(|| comment.strip_prefix("brink-expect "))
        {
            let kind = if comment.starts_with("brink-expect") {
                DirectiveKind::Expect
            } else {
                DirectiveKind::Disable
            };
            let codes: Vec<DiagnosticCode> = codes_str
                .split_whitespace()
                .filter_map(DiagnosticCode::from_str_code)
                .collect();
            if !codes.is_empty() {
                let target_line = (line_idx + 1) as u32;
                let comment_byte_start = line_byte_start + comment_pos as u32;
                let comment_byte_end = line_byte_start + line.len() as u32;
                result.line_directives.insert(
                    target_line,
                    LineDirective {
                        kind,
                        codes: Some(codes),
                        range: TextRange::new(comment_byte_start.into(), comment_byte_end.into()),
                    },
                );
            }
        }
    }

    result
}

/// Apply suppressions to diagnostics for a single file.
///
/// - Removes diagnostics matched by `disable` directives
/// - Removes diagnostics matched by `expect` directives
/// - Emits E036 for `expect` directives with no matching diagnostic
///
/// Returns the filtered+augmented diagnostic list.
#[expect(
    clippy::cast_possible_truncation,
    reason = "source file byte offsets and line counts fit in u32 for any reasonable file"
)]
pub fn apply_suppressions(
    file_id: FileId,
    source: &str,
    diagnostics: Vec<Diagnostic>,
    suppressions: &Suppressions,
) -> Vec<Diagnostic> {
    if suppressions.disable_file {
        return Vec::new();
    }

    // Build line starts table for mapping byte offsets → line numbers
    let line_starts: Vec<u32> = std::iter::once(0)
        .chain(source.bytes().enumerate().filter_map(|(i, b)| {
            if b == b'\n' {
                Some((i + 1) as u32)
            } else {
                None
            }
        }))
        .collect();

    // Track which expect directives were satisfied
    let mut expect_satisfied: BTreeMap<u32, bool> = BTreeMap::new();
    for (&target_line, directive) in &suppressions.line_directives {
        if directive.kind == DirectiveKind::Expect {
            expect_satisfied.insert(target_line, false);
        }
    }

    let mut result = Vec::with_capacity(diagnostics.len());

    for diag in diagnostics {
        let byte_offset: u32 = diag.range.start().into();
        let line = line_starts
            .partition_point(|&start| start <= byte_offset)
            .saturating_sub(1) as u32;

        if let Some(directive) = suppressions.line_directives.get(&line) {
            let matches = match &directive.codes {
                None => true,
                Some(codes) => codes.contains(&diag.code),
            };
            if matches {
                if directive.kind == DirectiveKind::Expect {
                    expect_satisfied.insert(line, true);
                }
                continue; // suppress this diagnostic
            }
        }

        result.push(diag);
    }

    // Emit E036 for unsatisfied expect directives
    for (&target_line, satisfied) in &expect_satisfied {
        if !satisfied && let Some(directive) = suppressions.line_directives.get(&target_line) {
            let message = match &directive.codes {
                None => "expected a diagnostic on the next line, but none was produced".to_string(),
                Some(codes) => {
                    let code_strs: Vec<&str> = codes.iter().map(|c| c.as_str()).collect();
                    format!(
                        "expected diagnostic {} on the next line, but it was not produced",
                        code_strs.join(", ")
                    )
                }
            };
            result.push(Diagnostic {
                file: file_id,
                range: directive.range,
                message,
                code: DiagnosticCode::E036,
            });
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_disable_all() {
        let src = "// brink-disable-all\nHello\n";
        let sup = parse_suppressions(src);
        assert!(sup.disable_all);
        assert!(!sup.disable_file);
        assert!(sup.line_directives.is_empty());
    }

    #[test]
    fn parse_disable_file() {
        let src = "// brink-disable-file\nHello\n";
        let sup = parse_suppressions(src);
        assert!(!sup.disable_all);
        assert!(sup.disable_file);
    }

    #[test]
    fn parse_blanket_disable_next_line() {
        let src = "// brink-disable\nHello\n";
        let sup = parse_suppressions(src);
        assert_eq!(sup.line_directives.len(), 1);
        let dir = sup.line_directives.get(&1);
        assert!(dir.is_some());
        let dir = dir.unwrap();
        assert_eq!(dir.kind, DirectiveKind::Disable);
        assert!(dir.codes.is_none());
    }

    #[test]
    fn parse_specific_disable() {
        let src = "// brink-disable E027 E028\nHello\n";
        let sup = parse_suppressions(src);
        let dir = sup.line_directives.get(&1).unwrap();
        assert_eq!(dir.kind, DirectiveKind::Disable);
        let codes = dir.codes.as_ref().unwrap();
        assert_eq!(codes, &[DiagnosticCode::E027, DiagnosticCode::E028]);
    }

    #[test]
    fn parse_expect_blanket() {
        let src = "// brink-expect\nHello\n";
        let sup = parse_suppressions(src);
        let dir = sup.line_directives.get(&1).unwrap();
        assert_eq!(dir.kind, DirectiveKind::Expect);
        assert!(dir.codes.is_none());
    }

    #[test]
    fn parse_expect_specific() {
        let src = "// brink-expect E025\nHello\n";
        let sup = parse_suppressions(src);
        let dir = sup.line_directives.get(&1).unwrap();
        assert_eq!(dir.kind, DirectiveKind::Expect);
        let codes = dir.codes.as_ref().unwrap();
        assert_eq!(codes, &[DiagnosticCode::E025]);
    }

    #[test]
    fn parse_ignores_invalid_codes() {
        let src = "// brink-disable XXXX E027\nHello\n";
        let sup = parse_suppressions(src);
        let dir = sup.line_directives.get(&1).unwrap();
        let codes = dir.codes.as_ref().unwrap();
        assert_eq!(codes, &[DiagnosticCode::E027]);
    }

    #[test]
    fn parse_all_invalid_codes_produces_no_directive() {
        let src = "// brink-disable XXXX YYYY\nHello\n";
        let sup = parse_suppressions(src);
        assert!(sup.line_directives.is_empty());
    }

    #[test]
    fn apply_disable_file_removes_all() {
        let file_id = FileId(0);
        let source = "// brink-disable-file\nHello\n";
        let sup = Suppressions {
            disable_file: true,
            ..Suppressions::default()
        };
        let diags = vec![Diagnostic {
            file: file_id,
            range: TextRange::new(22.into(), 27.into()),
            message: "test".to_string(),
            code: DiagnosticCode::E025,
        }];
        let result = apply_suppressions(file_id, source, diags, &sup);
        assert!(result.is_empty());
    }

    #[test]
    fn apply_blanket_disable_suppresses() {
        let file_id = FileId(0);
        let source = "// brink-disable\nHello\n";
        let sup = parse_suppressions(source);
        let diags = vec![Diagnostic {
            file: file_id,
            range: TextRange::new(17.into(), 22.into()), // "Hello" on line 1
            message: "test".to_string(),
            code: DiagnosticCode::E025,
        }];
        let result = apply_suppressions(file_id, source, diags, &sup);
        assert!(result.is_empty());
    }

    #[test]
    fn apply_specific_disable_only_matches_code() {
        let file_id = FileId(0);
        let source = "// brink-disable E027\nHello\n";
        let sup = parse_suppressions(source);
        let diags = vec![
            Diagnostic {
                file: file_id,
                range: TextRange::new(22.into(), 27.into()),
                message: "ambiguous".to_string(),
                code: DiagnosticCode::E027,
            },
            Diagnostic {
                file: file_id,
                range: TextRange::new(22.into(), 27.into()),
                message: "unresolved".to_string(),
                code: DiagnosticCode::E025,
            },
        ];
        let result = apply_suppressions(file_id, source, diags, &sup);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].code, DiagnosticCode::E025);
    }

    #[test]
    fn apply_expect_satisfied() {
        let file_id = FileId(0);
        let source = "// brink-expect E025\nHello\n";
        let sup = parse_suppressions(source);
        let diags = vec![Diagnostic {
            file: file_id,
            range: TextRange::new(21.into(), 26.into()),
            message: "unresolved".to_string(),
            code: DiagnosticCode::E025,
        }];
        let result = apply_suppressions(file_id, source, diags, &sup);
        assert!(result.is_empty()); // suppressed and expectation met
    }

    #[test]
    fn apply_expect_unsatisfied_emits_e036() {
        let file_id = FileId(0);
        let source = "// brink-expect E025\nHello\n";
        let sup = parse_suppressions(source);
        let diags = vec![]; // no diagnostics on next line
        let result = apply_suppressions(file_id, source, diags, &sup);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].code, DiagnosticCode::E036);
        assert!(result[0].message.contains("E025"));
    }

    #[test]
    fn apply_blanket_expect_unsatisfied() {
        let file_id = FileId(0);
        let source = "// brink-expect\nHello\n";
        let sup = parse_suppressions(source);
        let diags = vec![];
        let result = apply_suppressions(file_id, source, diags, &sup);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].code, DiagnosticCode::E036);
        assert!(result[0].message.contains("expected a diagnostic"));
    }

    #[test]
    fn diagnostic_on_wrong_line_not_suppressed() {
        let file_id = FileId(0);
        let source = "// brink-disable\nOk line\nBad line\n";
        let sup = parse_suppressions(source);
        // Diagnostic on line 2 ("Bad line"), not line 1
        let diags = vec![Diagnostic {
            file: file_id,
            range: TextRange::new(25.into(), 33.into()),
            message: "test".to_string(),
            code: DiagnosticCode::E025,
        }];
        let result = apply_suppressions(file_id, source, diags, &sup);
        assert_eq!(result.len(), 1);
    }
}
