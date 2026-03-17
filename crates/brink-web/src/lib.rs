use std::cell::RefCell;

use brink_runtime::FastRng;
use serde::Serialize;
use wasm_bindgen::prelude::*;

// ── Compilation ─────────────────────────────────────────────────────

/// Compile ink source and return JSON with diagnostics or story data.
#[wasm_bindgen]
pub fn compile(source: &str) -> String {
    let result = brink_compiler::compile("main.ink", |_path| Ok(source.to_owned()));

    match result {
        Ok(output) => {
            let warnings: Vec<DiagnosticJs> = output
                .warnings
                .iter()
                .map(|d| DiagnosticJs {
                    message: d.message.clone(),
                    start: d.range.start().into(),
                    end: d.range.end().into(),
                    severity: format!("{:?}", d.code.severity()),
                })
                .collect();

            let data = output.data;
            let mut bytes = Vec::new();
            brink_format::write_inkb(&data, &mut bytes);

            let resp = CompileResult {
                ok: true,
                story_bytes: Some(bytes),
                warnings,
                error: None,
            };
            serde_json::to_string(&resp).unwrap_or_default()
        }
        Err(e) => {
            let resp = CompileResult {
                ok: false,
                story_bytes: None,
                warnings: Vec::new(),
                error: Some(format!("{e}")),
            };
            serde_json::to_string(&resp).unwrap_or_default()
        }
    }
}

#[derive(Serialize)]
struct CompileResult {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    story_bytes: Option<Vec<u8>>,
    warnings: Vec<DiagnosticJs>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct DiagnosticJs {
    message: String,
    start: u32,
    end: u32,
    severity: String,
}

// ── Runtime ─────────────────────────────────────────────────────────

/// A running story instance. Owns Program + Story to avoid lifetime issues in wasm.
#[wasm_bindgen]
pub struct StoryRunner {
    // We store Program in a Box to get a stable address, and the Story
    // borrows from it. We use raw pointer + RefCell to work around the
    // self-referential borrow.
    program: Box<brink_runtime::Program>,
    // Safety: story borrows from program which is heap-pinned and never moved.
    // We only access story through &mut self methods (single-threaded wasm).
    story: RefCell<Option<brink_runtime::Story<'static, FastRng>>>,
}

#[wasm_bindgen]
impl StoryRunner {
    /// Create a new story runner from compiled story bytes.
    #[wasm_bindgen(constructor)]
    pub fn new(story_bytes: &[u8]) -> Result<StoryRunner, JsError> {
        let data = brink_format::read_inkb(story_bytes)
            .map_err(|e| JsError::new(&format!("decode error: {e}")))?;
        let program = Box::new(
            brink_runtime::link(&data).map_err(|e| JsError::new(&format!("link error: {e}")))?,
        );

        // Safety: we pin the Program in a Box and keep it alive for the
        // lifetime of StoryRunner. The Story borrows it, but we transmute
        // the lifetime to 'static. This is safe because:
        // 1. program is heap-allocated and never moved
        // 2. story is dropped before program (struct drop order)
        // 3. wasm is single-threaded
        let program_ptr: *const brink_runtime::Program = &raw const *program;
        // SAFETY: program is heap-allocated via Box and never moved. Story borrows
        // it for 'static, but StoryRunner keeps the Box alive and drops story first
        // (struct field drop order). wasm is single-threaded.
        #[expect(unsafe_code)]
        let program_ref: &'static brink_runtime::Program = unsafe { &*program_ptr };

        let story = brink_runtime::Story::<FastRng>::new(program_ref);

        Ok(StoryRunner {
            program,
            story: RefCell::new(Some(story)),
        })
    }

    /// Continue the story. Returns JSON with text, choices, and status.
    pub fn continue_story(&self) -> Result<String, JsError> {
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;

        let result = story
            .continue_maximally()
            .map_err(|e| JsError::new(&format!("runtime error: {e}")))?;

        let resp = match result {
            brink_runtime::StepResult::Done { text, tags } => StepResultJs {
                status: "continue",
                text,
                choices: Vec::new(),
                tags,
            },
            brink_runtime::StepResult::Choices {
                text,
                choices,
                tags,
            } => StepResultJs {
                status: "choices",
                text,
                choices: choices
                    .into_iter()
                    .map(|c| ChoiceJs {
                        text: c.text,
                        index: c.index,
                        tags: c.tags,
                    })
                    .collect(),
                tags,
            },
            brink_runtime::StepResult::Ended { text, tags } => StepResultJs {
                status: "ended",
                text,
                choices: Vec::new(),
                tags,
            },
        };

        serde_json::to_string(&resp).map_err(|e| JsError::new(&format!("json error: {e}")))
    }

    /// Choose an option by index.
    pub fn choose(&self, index: usize) -> Result<(), JsError> {
        let mut borrow = self.story.borrow_mut();
        let story = borrow
            .as_mut()
            .ok_or_else(|| JsError::new("story not initialized"))?;

        story
            .choose(index)
            .map_err(|e| JsError::new(&format!("choose error: {e}")))
    }

    /// Reset: create a fresh story from the same program.
    pub fn reset(&self) {
        let program_ptr: *const brink_runtime::Program = &raw const *self.program;
        // SAFETY: same invariants as in `new` — Box is pinned and outlives the Story.
        #[expect(unsafe_code)]
        let program_ref: &'static brink_runtime::Program = unsafe { &*program_ptr };

        let story = brink_runtime::Story::<FastRng>::new(program_ref);
        *self.story.borrow_mut() = Some(story);
    }
}

#[derive(Serialize)]
struct StepResultJs {
    status: &'static str,
    text: String,
    choices: Vec<ChoiceJs>,
    tags: Vec<Vec<String>>,
}

#[derive(Serialize)]
struct ChoiceJs {
    text: String,
    index: usize,
    tags: Vec<String>,
}

// ── IDE ─────────────────────────────────────────────────────────────

/// Compute semantic tokens for syntax highlighting. Returns JSON array of tokens.
#[wasm_bindgen]
pub fn semantic_tokens(source: &str) -> String {
    let parse = brink_syntax::parse(source);
    let root = parse.syntax();
    let analysis = brink_analyzer::AnalysisResult {
        index: brink_ir::SymbolIndex::default(),
        resolutions: Vec::new(),
        diagnostics: Vec::new(),
    };
    let file_id = brink_ir::FileId(0);

    let raw = brink_ide::semantic_tokens::semantic_tokens(source, &root, &analysis, file_id);

    let tokens: Vec<TokenJs> = raw
        .iter()
        .map(|t| TokenJs {
            line: t.line,
            start_char: t.start_char,
            length: t.length,
            token_type: t.token_type,
            modifiers: t.modifiers,
        })
        .collect();

    serde_json::to_string(&tokens).unwrap_or_default()
}

/// Get token type names for the legend.
#[wasm_bindgen]
pub fn token_type_names() -> String {
    serde_json::to_string(brink_ide::semantic_tokens::token_type_names()).unwrap_or_default()
}

/// Get token modifier names for the legend.
#[wasm_bindgen]
pub fn token_modifier_names() -> String {
    serde_json::to_string(brink_ide::semantic_tokens::token_modifier_names()).unwrap_or_default()
}

#[derive(Serialize)]
struct TokenJs {
    line: u32,
    start_char: u32,
    length: u32,
    token_type: u32,
    modifiers: u32,
}
