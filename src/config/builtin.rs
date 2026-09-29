//! The shipped styles, built into the binary.

use std::path::Path;

use anyhow::{Context, Result};

/// The style used when none is given (the CLI's default; `make-mod.sh`'s installed style).
pub const DEFAULT_STYLE: &str = "impressionist";

/// The shipped styles (`styles/`), built into the binary so they work by name from anywhere:
/// (path relative to `styles/`, text).
pub(super) const BUILTIN_STYLES: &[(&str, &str)] = &[
    (
        "watercolor.toml",
        include_str!("../../styles/watercolor.toml"),
    ),
    (
        "impressionist.toml",
        include_str!("../../styles/impressionist.toml"),
    ),
    (
        "ss-baseline.toml",
        include_str!("../../styles/ss-baseline.toml"),
    ),
    (
        "ss-impressionist.toml",
        include_str!("../../styles/ss-impressionist.toml"),
    ),
    (
        "ss-terracotta.toml",
        include_str!("../../styles/ss-terracotta.toml"),
    ),
    (
        "ss-terracotta-impressionist.toml",
        include_str!("../../styles/ss-terracotta-impressionist.toml"),
    ),
    (
        "overlays/impressionist-brushwork.toml",
        include_str!("../../styles/overlays/impressionist-brushwork.toml"),
    ),
    (
        "overlays/terracotta.toml",
        include_str!("../../styles/overlays/terracotta.toml"),
    ),
];

/// Names of the built-in styles (overlays are layers, not styles).
pub(super) fn names() -> Vec<&'static str> {
    BUILTIN_STYLES
        .iter()
        .filter(|(p, _)| !p.contains('/'))
        .map(|(p, _)| p.trim_end_matches(".toml"))
        .collect()
}

/// Reads a built-in style file by its path relative to `styles/`.
pub(super) fn read(path: &Path) -> Result<String> {
    let key = path.to_string_lossy().replace('\\', "/");
    BUILTIN_STYLES
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, t)| t.to_string())
        .with_context(|| {
            format!(
                "no built-in style {key:?} (built-in: {})",
                names().join(", ")
            )
        })
}
