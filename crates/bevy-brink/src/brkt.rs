//! `.brkt` transcript persistence.
//!
//! A `.brkt` is the serialized output history of a playthrough — the
//! append-only log of structural output parts (line refs, values, glue,
//! tags). Because it stores *structure*, not resolved strings, a saved
//! transcript can be re-rendered against any matching program + locale
//! without re-running the story. Uses: a story-log game mechanic, QA
//! capture, and the visible-history half of a save file.
//!
//! This is a thin bevy layer over the runtime's
//! [`brink_runtime::transcript`] serialization:
//!
//! - [`capture_transcript`] — a live flow's transcript → `.brkt` bytes (write
//!   them into your save file).
//! - [`TranscriptAsset`] + [`BrktLoader`] — load saved `.brkt` bytes as an
//!   asset.
//! - [`render_transcript_asset`] — a loaded transcript + program + locale →
//!   rendered `(text, tags)` lines (validates the program checksum first).

use bevy_asset::{Asset, AssetLoader, LoadContext, io::Reader};
use bevy_reflect::TypePath;
use brink_format::PluralResolver;
use brink_runtime::transcript::{
    TranscriptData, TranscriptError, read_transcript, render_transcript, write_transcript,
};

use crate::asset::{LineTablesAsset, ProgramAsset};
use crate::flow::BrinkFlow;

/// A loaded `.brkt` transcript — the output history of a (past) playthrough,
/// re-renderable against any matching program + locale.
#[derive(Asset, TypePath)]
pub struct TranscriptAsset {
    pub data: TranscriptData,
}

/// Asset loader for `.brkt` (serialized transcript) files. Decodes via
/// [`brink_runtime::transcript::read_transcript`].
#[derive(Default, TypePath)]
pub struct BrktLoader;

/// Errors that can occur loading a `.brkt` file.
#[derive(Debug, thiserror::Error)]
pub enum BrktLoaderError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid .brkt: {0}")]
    Decode(#[from] TranscriptError),
}

impl AssetLoader for BrktLoader {
    type Asset = TranscriptAsset;
    type Settings = ();
    type Error = BrktLoaderError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let data = read_transcript(&bytes)?;
        Ok(TranscriptAsset { data })
    }

    fn extensions(&self) -> &[&str] {
        &["brkt"]
    }
}

/// Serialize a flow's current transcript to `.brkt` bytes for saving.
///
/// Store the returned bytes however your save system persists them; load
/// them back through the `.brkt` asset loader (or
/// [`read_transcript`](brink_runtime::transcript::read_transcript)) and
/// re-display with [`render_transcript_asset`]. The bytes embed the program's
/// `source_checksum` so a load can detect a mismatched story version.
#[must_use]
pub fn capture_transcript<M: Send + Sync + 'static>(
    flow: &BrinkFlow<M>,
    program: &ProgramAsset,
) -> Vec<u8> {
    write_transcript(
        flow.inner.transcript(),
        program.program.source_checksum(),
        flow.inner.fragments(),
    )
}

/// Re-render a loaded transcript against a program + locale line tables,
/// producing `(text, tags)` per line — the same output the live
/// [`BrinkTranscript`](crate::BrinkTranscript) would show.
///
/// Validates the transcript's `source_checksum` against the program first
/// (rendering against the wrong story would produce garbage), so pass the
/// program the transcript was captured from. `line_tables` may be the base
/// or any localized tables — the saved history re-renders in that locale.
///
/// # Errors
/// [`TranscriptError::ChecksumMismatch`] if the transcript wasn't produced by
/// this program.
pub fn render_transcript_asset(
    transcript: &TranscriptAsset,
    program: &ProgramAsset,
    line_tables: &LineTablesAsset,
    resolver: Option<&dyn PluralResolver>,
) -> Result<Vec<(String, Vec<String>)>, TranscriptError> {
    let program_checksum = program.program.source_checksum();
    if transcript.data.source_checksum != program_checksum {
        return Err(TranscriptError::ChecksumMismatch {
            transcript: transcript.data.source_checksum,
            program: program_checksum,
        });
    }
    Ok(render_transcript(
        &transcript.data.parts,
        &program.program,
        &line_tables.tables,
        resolver,
        &transcript.data.fragments,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_runtime::{FallbackHandler, FastRng, FlowInstance, LocaleMode, apply_locale};

    /// Compile a story, round-trip it through `.inkb` (so the program carries a
    /// real checksum), drive a root flow to the end, and return the driven
    /// flow + its program asset + base line tables.
    fn driven(src: &str) -> (BrinkFlow<()>, ProgramAsset, LineTablesAsset) {
        let owned = src.to_string();
        let out = brink_compiler::compile("t.ink", move |p| {
            if p == "t.ink" {
                Ok(owned.clone())
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "x"))
            }
        })
        .expect("compile");
        let mut inkb = Vec::new();
        brink_format::write_inkb(&out.data, &mut inkb);
        let loaded = brink_format::read_inkb(&inkb).expect("read_inkb");
        let (program, tables) = brink_runtime::link(&loaded).expect("link");

        let (mut flow, mut ctx) = FlowInstance::new_at_root(&program);
        for _ in 0..10_000 {
            let line = flow
                .step_single_line::<FastRng>(&program, &tables, &mut ctx, &FallbackHandler, None)
                .expect("step");
            if line.is_terminal() {
                break;
            }
        }
        let (_, initial_context) = FlowInstance::new_at_root(&program);
        (
            BrinkFlow::<()>::new(flow),
            ProgramAsset {
                program,
                initial_context,
            },
            LineTablesAsset { tables },
        )
    }

    #[test]
    fn capture_roundtrips_and_renders_like_live() {
        let (flow, prog, base) = driven("Hello there.\nGeneral Kenobi.\n-> END\n");

        let bytes = capture_transcript::<()>(&flow, &prog);
        let reloaded = TranscriptAsset {
            data: read_transcript(&bytes).expect("read_transcript"),
        };

        let rendered = render_transcript_asset(&reloaded, &prog, &base, None).expect("render");
        let text = rendered
            .iter()
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Hello there."), "got {text:?}");
        assert!(text.contains("General Kenobi."), "got {text:?}");

        // A reloaded-then-rendered transcript matches rendering the live one.
        let live = render_transcript(
            flow.inner.transcript(),
            &prog.program,
            &base.tables,
            None,
            flow.inner.fragments(),
        );
        assert_eq!(rendered, live);
    }

    #[test]
    fn render_rejects_mismatched_program() {
        let (flow, prog, base) = driven("A line.\n-> END\n");
        let bytes = capture_transcript::<()>(&flow, &prog);
        let reloaded = TranscriptAsset {
            data: read_transcript(&bytes).expect("read"),
        };

        // A different story → different checksum → mismatch error.
        let (_, other_prog, _) = driven("An entirely different tale.\n-> END\n");
        let err = render_transcript_asset(&reloaded, &other_prog, &base, None).unwrap_err();
        assert!(
            matches!(err, TranscriptError::ChecksumMismatch { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn saved_transcript_renders_in_a_different_locale() {
        // Capture on the base locale, then re-render the SAME saved transcript
        // against a localized line table — the persisted history localizes.
        let src = "Hello world\n-> END\n";
        let (flow, prog, base) = driven(src);
        let bytes = capture_transcript::<()>(&flow, &prog);
        let reloaded = TranscriptAsset {
            data: read_transcript(&bytes).expect("read"),
        };

        // Build an `es` overlay translating the first line.
        let out = brink_compiler::compile("t.ink", |p| {
            if p == "t.ink" {
                Ok(src.to_string())
            } else {
                Err(std::io::Error::new(std::io::ErrorKind::NotFound, "x"))
            }
        })
        .expect("compile");
        let mut inkb = Vec::new();
        brink_format::write_inkb(&out.data, &mut inkb);
        let loaded = brink_format::read_inkb(&inkb).expect("read_inkb");
        let checksum = brink_format::read_inkb_index(&inkb)
            .expect("index")
            .checksum;
        let mut lines = brink_intl::export_lines(&loaded, checksum);
        lines.scopes[0].lines[0].content =
            Some(brink_intl::ContentJson::Plain("Hola mundo\n".to_string()));
        let inkl_bytes = brink_intl::compile_locale(&inkb, &lines, "es").expect("compile_locale");
        let locale_data = brink_format::read_inkl(&inkl_bytes).expect("read_inkl");
        let es_tables = apply_locale(
            &prog.program,
            &locale_data,
            &base.tables,
            LocaleMode::Overlay,
        )
        .expect("apply_locale");

        let rendered = render_transcript_asset(
            &reloaded,
            &prog,
            &LineTablesAsset { tables: es_tables },
            None,
        )
        .expect("render");
        let text = rendered.iter().map(|(t, _)| t.as_str()).collect::<String>();
        assert!(
            text.contains("Hola mundo"),
            "saved history localizes; got {text:?}"
        );
    }
}
