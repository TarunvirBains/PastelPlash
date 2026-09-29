//! The shipped styles, built into the binary.

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
        "overlays/impressionist-brushwork.toml",
        include_str!("../../styles/overlays/impressionist-brushwork.toml"),
    ),
];
