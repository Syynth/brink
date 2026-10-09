//! How each transcript row reads on the Player's Stage surface (the web
//! studio's direction C, decision log 2026-09-02 "Player look"): plain
//! narration, a speaker's run — the cue printing the speaker's name once,
//! the lines after it hanging off a rule in their colour — and action,
//! dimmed.
//!
//! The reading is the project's dialogue dialect's, through the same two
//! pieces the web's `foldPlayerRuns` uses: the emitted-line parser and
//! the run rule (`brink_ide::dialect_infer`). No dialect, no runs: every
//! line is narration. Studio chrome (a choice echo, a notice, an error)
//! never joins a run, and a choice echo is the reader's turn, so it ends
//! whatever run was open.

use brink_ide::dialect_infer::{EmittedLine, EmittedParser, runs_of};
use brink_ir::{DialogueDialect, ElementNature};

/// What a row is, for drawing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// Prose the reader reads, on the bare spine.
    Narration,
    /// A line in a speaker's run. `cue` opens the run, and is where the
    /// speaker's name prints.
    Speech { speaker: String, cue: bool },
    /// Action and the dialect's machinery: dimmed (ruled: "action is
    /// dimmed, narration isn't").
    Action,
    /// Not story text.
    Chrome,
}

/// One row's reading: its role, and the text to show — a cue line's text
/// without the cue itself, since the name prints as the run's header — and
/// a direction that opens a speech (`(quietly)`), set apart from the words
/// so it reads as direction, not dialogue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Look {
    pub role: Role,
    pub text: String,
    pub direction: Option<String>,
}

/// A dialect with its emitted parser compiled, kept between renders.
pub(crate) struct Reader {
    dialect: DialogueDialect,
    parser: EmittedParser,
}

impl Reader {
    /// `None` when the dialect's emitted shapes do not compile — the
    /// analysis reports that; the Player just reads plainly.
    pub(crate) fn new(dialect: &DialogueDialect) -> Option<Self> {
        Some(Self {
            dialect: dialect.clone(),
            parser: EmittedParser::compile(dialect).ok()?,
        })
    }

    /// Whether this reader was built for `dialect`.
    pub(crate) fn is_for(&self, dialect: &DialogueDialect) -> bool {
        self.dialect == *dialect
    }
}

/// Read the transcript: `rows[i]` is a story line's text, or `None` for
/// chrome; `echo[i]` marks a choice echo (the reader's turn).
pub(crate) fn read(rows: &[Option<&str>], echo: &[bool], reader: Option<&Reader>) -> Vec<Look> {
    let mut looks: Vec<Look> = rows
        .iter()
        .map(|row| match row {
            Some(text) => Look {
                role: Role::Narration,
                text: (*text).to_owned(),
                direction: None,
            },
            None => Look {
                role: Role::Chrome,
                text: String::new(),
                direction: None,
            },
        })
        .collect();
    let Some(reader) = reader else {
        return looks;
    };
    // The story lines alone, each remembering whether the reader took a
    // turn since the one before it.
    let mut index: Vec<usize> = Vec::new();
    let mut lines: Vec<EmittedLine> = Vec::new();
    let mut boundary = false;
    for (i, row) in rows.iter().enumerate() {
        match row {
            Some(text) => {
                index.push(i);
                lines.push(EmittedLine {
                    segments: reader.parser.parse_emitted(text),
                    boundary,
                });
                boundary = false;
            }
            None => boundary |= echo.get(i).copied().unwrap_or(false),
        }
    }
    let action = |kind: &str| {
        kind.contains("action")
            || kind.contains("parenthetical")
            || reader
                .dialect
                .elements
                .iter()
                .find(|e| e.kind == kind)
                .is_some_and(|e| e.nature != ElementNature::Narrative)
    };
    for run in runs_of(&lines, &reader.dialect) {
        let Some(first) = run.lines.first().copied() else {
            continue;
        };
        let opening = lines[first].segments.first();
        let cue_kind = opening.and_then(|s| s.kind.clone());
        let speaker = match (&run.kind, opening) {
            (Some(kind), Some(seg)) if Some(kind) == cue_kind.as_ref() && !action(kind) => seg
                .content
                .clone()
                .or_else(|| Some(seg.text.trim().trim_end_matches(':').trim().to_owned())),
            _ => None,
        };
        for (n, &line) in run.lines.iter().enumerate() {
            let at = index[line];
            let segments = &lines[line].segments;
            let look = &mut looks[at];
            match &speaker {
                Some(name) => {
                    let cue = n == 0;
                    // The name is the header; the line shows the rest. A
                    // direction opening the speech stands apart from it.
                    let rest = &segments[usize::from(cue).min(segments.len())..];
                    let opens = rest
                        .first()
                        .filter(|s| s.kind.as_deref().is_some_and(action));
                    look.direction = opens.map(|s| s.text.trim().to_owned());
                    look.text = rest
                        .iter()
                        .skip(usize::from(opens.is_some()))
                        .map(|s| s.text.as_str())
                        .collect::<String>()
                        .trim()
                        .to_owned();
                    look.role = Role::Speech {
                        speaker: name.clone(),
                        cue,
                    };
                }
                None => {
                    if segments
                        .first()
                        .and_then(|s| s.kind.as_deref())
                        .is_some_and(action)
                    {
                        look.role = Role::Action;
                    }
                }
            }
        }
    }
    looks
}

/// A speaker's colour slot: the same name gets the same colour for the
/// whole session, and across sessions.
pub(crate) fn colour_slot(speaker: &str, slots: usize) -> usize {
    let hash = speaker
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)));
    hash as usize % slots.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_dialect_every_line_is_narration() {
        let looks = read(&[Some("MARA: Hi."), None], &[false, true], None);
        assert_eq!(looks[0].role, Role::Narration);
        assert_eq!(looks[0].text, "MARA: Hi.");
        assert_eq!(looks[1].role, Role::Chrome);
    }

    #[test]
    fn a_cue_opens_a_run_and_prints_as_its_header() {
        let dialect = brink_ir::dialect::at_cue_preset();
        let reader = Reader::new(&dialect).expect("the preset compiles");
        let looks = read(
            &[
                Some("@MARA: You came back."),
                Some("The boats knock."),
                None,
            ],
            &[false, false, true],
            Some(&reader),
        );
        assert_eq!(
            looks[0].role,
            Role::Speech {
                speaker: "MARA".to_owned(),
                cue: true
            },
            "{looks:?}"
        );
        assert_eq!(
            looks[0].text, "You came back.",
            "the cue prints as the header"
        );
        assert_eq!(
            looks[1].role,
            Role::Speech {
                speaker: "MARA".to_owned(),
                cue: false
            },
            "the next narrative line chains into the run"
        );
        assert_eq!(looks[2].role, Role::Chrome);
    }

    #[test]
    fn a_direction_opening_a_speech_stands_apart() {
        let dialect = brink_ir::dialect::at_cue_preset();
        let reader = Reader::new(&dialect).expect("the preset compiles");
        let looks = read(
            &[
                Some("@JONAH: (quietly)I said I would."),
                Some("(beat)Every year."),
            ],
            &[false, false],
            Some(&reader),
        );
        assert_eq!(
            looks[0].direction.as_deref(),
            Some("(quietly)"),
            "{looks:?}"
        );
        assert_eq!(looks[0].text, "I said I would.");
        // Only after a cue: at a line's start the dialect's emitted parser
        // takes reserved prefixes alone, so prose that opens with "(" is
        // never mistaken for a direction (the web's parser, one rule).
        assert_eq!(looks[1].direction, None, "{looks:?}");
        assert_eq!(looks[1].text, "(beat)Every year.");
    }

    #[test]
    fn a_speakers_colour_is_stable() {
        assert_eq!(colour_slot("MARA", 8), colour_slot("MARA", 8));
    }
}
