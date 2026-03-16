mod backend;
mod convert;
mod semantic_tokens;

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use tokio::sync::{Notify, watch};
use tower_lsp::{LspService, Server};

use crate::backend::Backend;

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
    let last_published = Arc::new(Mutex::new(HashMap::new()));

    let (service, socket) = LspService::new(|client| {
        // Spawn the background analysis loop
        tokio::spawn(backend::analysis_loop(
            Arc::clone(&db),
            Arc::clone(&generation),
            Arc::clone(&trigger),
            analysis_tx,
            client.clone(),
            Arc::clone(&last_published),
        ));

        Backend::new(
            client,
            Arc::clone(&db),
            analysis_rx,
            Arc::clone(&trigger),
            Arc::clone(&generation),
            Arc::clone(&last_published),
        )
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
