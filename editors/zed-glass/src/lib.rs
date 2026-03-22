use zed_extension_api as zed;

struct GlassExtension;

impl zed::Extension for GlassExtension {
    fn new() -> Self {
        GlassExtension
    }
}

zed::register_extension!(GlassExtension);
