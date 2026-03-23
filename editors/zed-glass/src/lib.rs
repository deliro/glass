use zed_extension_api::{self as zed, LanguageServerId};

struct GlassExtension;

impl zed::Extension for GlassExtension {
    fn new() -> Self {
        GlassExtension
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        let binary = worktree
            .which("glass")
            .ok_or_else(|| "glass not found on PATH. Install with: cargo install --path <glass-repo>".to_string())?;
        Ok(zed::Command {
            command: binary,
            args: vec!["lsp".to_string()],
            env: Default::default(),
        })
    }
}

zed::register_extension!(GlassExtension);
