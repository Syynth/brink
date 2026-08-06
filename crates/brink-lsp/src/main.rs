mod backend;
mod convert;
mod semantic_tokens;

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use tokio::sync::{Notify, watch};
use tower_lsp::{LspService, Server};

use crate::backend::projects::NativeProjects;
use crate::backend::{Backend, DiagnosticsPublisher, LanguageOptions};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let db = Arc::new(Mutex::new(NativeProjects::new()));
    let generation = Arc::new(AtomicU64::new(0));
    let trigger = Arc::new(Notify::new());
    let (analysis_tx, analysis_rx) = watch::channel(None);
    // Client-declared dialect + TM-3 typed-mode policy (#599, #660):
    // `initialize`'s `initializationOptions` handlers write into these
    // shared `Arc<Mutex<_>>`s, and both the foreground `Backend` and the
    // background `analysis_loop` task read them, so live diagnostics
    // analyze under the same client-declared policy as everything else.
    let language = LanguageOptions::new();
    // Shared undeclared-rename-detection baseline (issue #1672 part 2,
    // review finding): one map, cloned into both the foreground `Backend`
    // (which advances it at `did_open`/`did_save`) and the background
    // `analysis_loop` (which only reads it), so the two publishers agree on
    // the same checkpoint within one generation instead of the background
    // pass diffing against a baseline the foreground side had already moved.
    let previous_manifests: Arc<Mutex<HashMap<brink_ir::FileId, brink_ir::SymbolManifest>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let (service, socket) = LspService::new(|client| {
        // One serialized diagnostics publisher shared by the foreground
        // `Backend` and the background `analysis_loop` (#615): it owns the
        // last-published state and the send, so wire order == decision order.
        let publisher = DiagnosticsPublisher::new(client.clone());

        // Spawn the background analysis loop
        tokio::spawn(backend::analysis_loop(
            Arc::clone(&db),
            Arc::clone(&generation),
            Arc::clone(&trigger),
            analysis_tx,
            client.clone(),
            publisher.clone(),
            language.clone(),
            Arc::clone(&previous_manifests),
        ));

        Backend::new(
            client,
            Arc::clone(&db),
            analysis_rx,
            Arc::clone(&trigger),
            Arc::clone(&generation),
            publisher,
            language.clone(),
            Arc::clone(&previous_manifests),
        )
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
