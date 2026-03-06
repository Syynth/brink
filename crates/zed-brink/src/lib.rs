use zed_extension_api as zed;

struct InkExtension;

impl zed::Extension for InkExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let path = worktree
            .which("brink-lsp")
            .ok_or_else(|| "brink-lsp not found on PATH. Install with: cargo install --path crates/brink-lsp".to_string())?;

        Ok(zed::Command {
            command: path,
            args: vec![],
            env: Default::default(),
        })
    }
}

zed::register_extension!(InkExtension);
