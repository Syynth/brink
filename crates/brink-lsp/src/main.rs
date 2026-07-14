mod backend;
mod convert;
mod semantic_tokens;

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use tokio::sync::{Notify, watch};
use tower_lsp::{LspService, Server};

use crate::backend::{Backend, DiagnosticsPublisher, LanguageOptions};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let db = Arc::new(Mutex::new(brink_db::ProjectDb::new()));
    let generation = Arc::new(AtomicU64::new(0));
    let trigger = Arc::new(Notify::new());
    let (analysis_tx, analysis_rx) = watch::channel(None);
    // Client-declared dialect + TM-3 typed-mode policy (#599, #660):
    // `initialize`'s `initializationOptions` handlers write into these
    // shared `Arc<Mutex<_>>`s, and both the foreground `Backend` and the
    // background `analysis_loop` task read them, so live diagnostics
    // analyze under the same client-declared policy as everything else.
    let language = LanguageOptions::new();

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
        ));

        Backend::new(
            client,
            Arc::clone(&db),
            analysis_rx,
            Arc::clone(&trigger),
            Arc::clone(&generation),
            publisher,
            language.clone(),
        )
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
