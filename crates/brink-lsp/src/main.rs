mod backend;
mod convert;
mod semantic_tokens;

use std::sync::{Arc, Mutex};

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
    let (service, socket) = LspService::new(|client| Backend::new(client, Arc::clone(&db)));
    Server::new(stdin, stdout, socket).serve(service).await;
}
