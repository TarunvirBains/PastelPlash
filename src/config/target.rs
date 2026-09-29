//! The target layer: how the game will render it.

use std::collections::BTreeMap;

use anyhow::Result;
use serde::Deserialize;

use super::{Category, check, non_negative, unit};

/// How the game will render it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Target {
    pub name: String,
    /// Output resolution floor for every restyled category (a category's own `resolution`
    /// replaces it). Off by default.
    pub resolution: Resolution,
    /// Per-category treatment; categories not listed get [`Treatment::default`].
    pub categories: BTreeMap<Category, Treatment>,
}

impl Target {
    pub fn treatment(&self, category: Category) -> Treatment {
        self.categories
            .get(&category)
            .cloned()
            .unwrap_or_else(|| Treatment::default_for(category))
    }

    /// The resolution floor a category gets: its own, else the target's.
    pub fn resolution(&self, category: Category) -> Resolution {
        self.categories
            .get(&category)
            .and_then(|t| t.resolution)
            .unwrap_or(self.resolution)
    }
}

/// Output resolution floor: textures whose long side is below `floor` are written larger, by an
/// integer factor (at most `max_factor`; a power of two with `power_of_two`), so low-resolution
/// sources don't show their texel grid in a high-resolution game. They are enlarged with a
/// Lanczos filter (no invented detail), restyled at `internal` times the output size (the
/// painting's marks resolve finer than the output texels), then area-averaged down. Paint marks
/// keep their size relative to the source's content. Textures at or above the floor keep their
/// size. The game must accept larger replacements (for `.o2r` packs the adapter rescales the
/// texture header).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Resolution {
    /// Long side (texels) below which the output is enlarged; 0 = off.
    pub floor: u32,
    /// Largest enlargement factor.
    pub max_factor: u32,
    /// Factors are powers of two (2, 4, …) rather than any integer.
    pub power_of_two: bool,
    /// Restyle at this multiple of the output size, then area-average down (1 = at output size).
    pub internal: u32,
}

impl Default for Resolution {
    fn default() -> Self {
        Self {
            floor: 0,
            max_factor: 4,
            power_of_two: true,
            internal: 2,
        }
    }
}

impl Resolution {
    /// The output enlargement factor for a `w`×`h` texture (1 = keep its size): the smallest
    /// allowed factor that brings the long side to the floor, capped at `max_factor`.
    pub fn factor(&self, w: u32, h: u32) -> u32 {
        let long = w.max(h);
        if self.floor == 0 || long == 0 || long >= self.floor {
            return 1;
        }
        let need = self.floor.div_ceil(long);
        let f = if self.power_of_two {
            need.next_power_of_two()
        } else {
            need
        };
        f.clamp(1, self.max_factor.max(1))
    }

    /// (output factor, internal factor over the source) for a `w`×`h` texture; (1, 1) when it
    /// keeps its size.
    pub fn plan(&self, w: u32, h: u32) -> (u32, u32) {
        let k = self.factor(w, h);
        if k <= 1 {
            (1, 1)
        } else {
            (k, k * self.internal.max(1))
        }
    }

    fn validate(&self, name: &str) -> Result<()> {
        check((1..=16).contains(&self.max_factor), || {
            format!(
                "{name}.max_factor = {} must be within 1..=16",
                self.max_factor
            )
        })?;
        check((1..=4).contains(&self.internal), || {
            format!("{name}.internal = {} must be within 1..=4", self.internal)
        })?;
        check(self.floor <= 16384, || {
            format!("{name}.floor = {} must be at most 16384", self.floor)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Treatment {
    /// How strongly to remove baked shading, 0..=1 (multiplied by the style's delight strength).
    pub delight: f32,
    /// Maximum OKLCH lightness, for textures the renderer brightens further.
    pub lightness_ceiling: Option<f32>,
    /// Width (OKLab L) of the soft knee below the ceiling; only values within it are
    /// compressed.
    pub ceiling_knee: f32,
    /// Multiplies the palette's hue shifts, pulls and harmonization (e.g. < 1 to keep actors'
    /// own skin, hair and leather hues).
    pub hue: f32,
    /// Force tint-safe (lightness-only palette) on or off; unset = detect from chroma.
    pub tint_safe: Option<bool>,
    /// Scales the palette's lightness lift (e.g. < 1 for lava or dark dungeon areas).
    pub floor_scale: f32,
    /// Multiplies the style's Kuwahara radius.
    pub radius_scale: f32,
    /// Warm/cool temperature strength (0 for relit categories: the renderer decides lit/shade).
    pub warm_cool: f32,
    /// Multiplies the accent-dark fraction and depth.
    pub accent: f32,
    /// Multiplies brushstroke strength (the value and the chroma variation).
    pub strokes: f32,
    /// Multiplies brushstroke width and length.
    pub stroke_scale: f32,
    /// Multiplies the palette's shadow tint amount.
    pub shadow_tint: f32,
    /// Multiplies the paper tooth and paper tint.
    pub paper: f32,
    /// Multiplies the style's value-contrast compression.
    pub value_contrast: f32,
    /// Multiplies the design-like abstraction (0 for finished paintings such as pre-rendered
    /// backgrounds).
    pub abstraction: f32,
    /// Multiplies the soft value grouping (only world and background textures are ever grouped).
    pub grouping: f32,
    /// Exposure handling after stylization (e.g. restore a pre-rendered room's mean lightness).
    pub exposure: Exposure,
    /// Multiplies the palette's earth warmth.
    pub warmth: f32,
    /// Multiplies the palette groups' chroma floors (`c_min`, "pastel is not gray"): < 1 keeps a
    /// finished painting's dull browns brown instead of lifting them toward the reference chroma.
    pub chroma_floor: f32,
    /// Multiplies the palette's dark chroma floor (`dark_chroma`): < 1 keeps a finished painting's
    /// dim shadows as the designers painted them (lifted, colored, but not re-saturated).
    pub dark_chroma: f32,
    /// Multiplies a mood's moonlight cast (0 for relit categories: the renderer lights them).
    pub cast: f32,
    /// Engine-tinted grayscale textures of this category are raised so their median gray sits
    /// here (OKLab L) and painted with brightness-only strokes; unset: the palette's lightness
    /// only. The engine's tint darkens them, so they may exceed the lightness ceiling.
    pub tint_safe_gray: Option<f32>,
    /// Brightness amplitude (OKLab L) of those strokes.
    pub tint_safe_strokes: f32,
    /// Multiplies the pull toward the style's reference water tone (`palette.water`; 0 = none).
    pub reference: f32,
    /// Multiplies the wet-edge darkening (0 for fluids: no outlined cells).
    pub wet_edges: f32,
    /// Multiplies the granulation (0 for fluids: no pigment settling in the valleys).
    pub granulation: f32,
    /// Map colors through the style's palette (false for emissive fluids: lava keeps its heat
    /// colors and glow).
    pub palette: bool,
    /// This category's output resolution floor, replacing the target's `[resolution]`.
    pub resolution: Option<Resolution>,
}

impl Default for Treatment {
    fn default() -> Self {
        Self {
            delight: 0.0,
            lightness_ceiling: None,
            ceiling_knee: 0.15,
            hue: 1.0,
            tint_safe: None,
            floor_scale: 1.0,
            radius_scale: 1.0,
            warm_cool: 0.0,
            accent: 1.0,
            strokes: 1.0,
            stroke_scale: 1.0,
            shadow_tint: 1.0,
            paper: 1.0,
            value_contrast: 1.0,
            warmth: 1.0,
            cast: 1.0,
            chroma_floor: 1.0,
            dark_chroma: 1.0,
            tint_safe_gray: None,
            tint_safe_strokes: 0.0,
            abstraction: 1.0,
            grouping: 1.0,
            exposure: Exposure::default(),
            reference: 0.0,
            wet_edges: 1.0,
            granulation: 1.0,
            palette: true,
            resolution: None,
        }
    }
}

impl Treatment {
    /// The treatment a category gets when the target does not list it. Fluids are soft light
    /// over depth: no de-lighting, value compression, abstraction, accents, wet edges or
    /// granulation, small paint marks and gentle strokes; water keeps its lightness and takes
    /// the reference water tone; lava keeps its own colors (no palette, no moonlight).
    pub fn default_for(category: Category) -> Self {
        let fluid = Self {
            warm_cool: 0.0,
            accent: 0.0,
            value_contrast: 0.0,
            abstraction: 0.0,
            grouping: 0.0,
            wet_edges: 0.0,
            granulation: 0.0,
            radius_scale: 0.3,
            strokes: 0.4,
            paper: 0.3,
            shadow_tint: 0.0,
            warmth: 0.0,
            hue: 0.0,
            floor_scale: 0.0,
            ..Self::default()
        };
        match category {
            Category::Water => Self {
                reference: 1.0,
                ..fluid
            },
            Category::Liquid => fluid,
            // Glow is light, not pigment: no paper showing through, only faint strokes.
            Category::Lava => Self {
                palette: false,
                cast: 0.0,
                paper: 0.0,
                strokes: 0.25,
                ..fluid
            },
            _ => Self::default(),
        }
    }
}

/// Per-category exposure handling (see `src/exposure.rs`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Exposure {
    /// Restore the source's alpha-weighted mean lightness with a smooth monotone tone curve
    /// (the murk lift stays; mids and lights come down to compensate).
    pub preserve_mean: bool,
    /// Lightness range over which the curve fades in: darks below `protect[0]` are untouched.
    pub protect: [f32; 2],
}

impl Default for Exposure {
    fn default() -> Self {
        Self {
            preserve_mean: false,
            protect: [0.15, 0.75],
        }
    }
}

impl Target {
    pub fn validate(&self) -> Result<()> {
        self.resolution.validate("resolution")?;
        for (cat, t) in &self.categories {
            if let Some(r) = &t.resolution {
                r.validate(&format!("categories.{cat:?}.resolution"))?;
            }
            if let Some(c) = t.lightness_ceiling {
                check(c > 0.0 && c <= 1.0, || {
                    format!("categories.{cat:?}.lightness_ceiling = {c} must be within (0, 1]")
                })?;
            }
            unit(&format!("categories.{cat:?}.delight"), t.delight)?;
            non_negative(&format!("categories.{cat:?}.warm_cool"), t.warm_cool)?;
            non_negative(&format!("categories.{cat:?}.floor_scale"), t.floor_scale)?;
            if let Some(g) = t.tint_safe_gray {
                check(g > 0.0 && g < 1.0, || {
                    format!("categories.{cat:?}.tint_safe_gray = {g} must be within (0, 1)")
                })?;
            }
            non_negative(
                &format!("categories.{cat:?}.tint_safe_strokes"),
                t.tint_safe_strokes,
            )?;
            unit(&format!("categories.{cat:?}.reference"), t.reference)?;
            non_negative(&format!("categories.{cat:?}.wet_edges"), t.wet_edges)?;
            non_negative(&format!("categories.{cat:?}.granulation"), t.granulation)?;
            let p = t.exposure.protect;
            check(0.0 <= p[0] && p[0] < p[1] && p[1] <= 1.0, || {
                format!("categories.{cat:?}.exposure.protect = {p:?} must be increasing in 0..=1")
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_floor_factors() {
        let regular = Resolution {
            floor: 1024,
            ..Resolution::default()
        };
        // The Deku Tree rings: 128x256 -> output 512x1024, restyled at 1024x2048.
        assert_eq!(regular.plan(128, 256), (4, 8));
        assert_eq!(regular.factor(300, 200), 4); // 3.4x needed: next power of two
        assert_eq!(regular.factor(700, 100), 2);
        assert_eq!(regular.factor(64, 64), 4); // capped
        assert_eq!(regular.plan(1024, 512), (1, 1)); // at the floor: untouched
        assert_eq!(regular.plan(2048, 64), (1, 1));
        let background = Resolution {
            floor: 3840,
            power_of_two: false,
            ..Resolution::default()
        };
        assert_eq!(background.plan(1280, 960), (3, 6)); // 3840x2880
        assert_eq!(background.factor(1024, 1024), 4); // 3.75x needed
        let off = Resolution::default();
        assert_eq!(off.plan(16, 16), (1, 1));
        let flat = Resolution {
            floor: 1024,
            internal: 1,
            ..Resolution::default()
        };
        assert_eq!(flat.plan(256, 256), (4, 4));
    }

    #[test]
    fn a_category_resolution_replaces_the_targets() {
        let t: Target = toml::from_str(
            "[resolution]\nfloor = 1024\n[categories.background.resolution]\nfloor = 3840\npower_of_two = false\n",
        )
        .unwrap();
        t.validate().unwrap();
        assert_eq!(t.resolution(Category::World).floor, 1024);
        assert_eq!(t.resolution(Category::Background).floor, 3840);
        assert!(!t.resolution(Category::Background).power_of_two);
        let bad: Target = toml::from_str("[resolution]\ninternal = 0\n").unwrap();
        assert!(bad.validate().is_err());
    }
}
