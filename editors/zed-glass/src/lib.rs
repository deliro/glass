use zed_extension_api::{self as zed, LanguageServerId};

struct GlassExtension;

impl zed::Extension for GlassExtension {
    fn new() -> Self {
        GlassExtension
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &LanguageServerId,
        _worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        Ok(zed::Command {
            command: "glass".to_string(),
            args: vec!["lsp".to_string()],
            env: Default::default(),
        })
    }
}

zed::register_extension!(GlassExtension);
