# PastelPlash style rules

These rules define PastelPlash's style. A contribution to this repository must keep them passing.
If you want a different aesthetic, you are welcome to fork the project and change the rules and
tests to match your style.

## Rules, tuning and snapshots

There are three kinds of change, and each has its own place:

| Change | Where | Review |
|---|---|---|
| **Tuning** a style (strengths, sizes, hue shifts, floors inside the bounds) | `styles/*.toml`, `targets/*.toml` | Free. No test edits needed. |
| **Changing a rule** (a hard limit of the style) | `rules.toml` (the *style contract*) | Deliberate, in its own commit. |
| **Intended change of look** | re-bless snapshots: `PASTELPLASH_BLESS=1 cargo test --test snapshots` | Look at the new goldens before committing. |

- **The contract.** `rules.toml` holds the hard limits as bounds, not exact values (for example
  "green floor ≥ 0.70", not "green floor = 0.74"). The rule tests read every limit from it and
  contain no magic numbers.
- **Every style.** The tests discover every file in `styles/` and every file in `targets/`, so a
  new style is checked automatically. They check two things:
  1. The style's parameters lie within the contract (fast, CPU only).
  2. Rendered output honors the parameters the style actually sets. For example, if a style sets
     its green floor to 0.76, no non-accent green texel may come out darker than 0.76, minus the
     style's `floor_margin` and the contract's tolerance.
- **Snapshots are alarms, not rules.** `tests/snapshots.rs` renders a few procedural textures with
  the default style and compares them with `tests/golden/`. It uses a perceptual tolerance: mean
  OKLab ΔE plus a small budget of outliers. A failure means the look changed. Decide whether that
  was intended, then re-bless.
  - When running the Windows build from WSL, pass the variable through:
    `WSLENV=PASTELPLASH_BLESS PASTELPLASH_BLESS=1 cargo test --test snapshots`.
- **GPU tests skip cleanly.** Rendered checks need a GPU adapter. Without one (for example in CI)
  they print a message and pass. The CPU checks on configuration and palette math always run.
- **No third-party images.** Test textures are generated procedurally. Never commit texture pack,
  game or museum images to this repository.

## The rules

All lightness (L) and chroma (C) values are OKLCH. "Tolerance" means the `[tolerance]` values in
`rules.toml`.

### Palette

| Rule | Why | Enforced by |
|---|---|---|
| **No dark greens.** A texel that reads as green (hue in the contract's green band, C ≥ `green_min_chroma`) is never darker than the style's green floor. Dark foliage becomes light sage, lime or mint. | This is the core of the pastel *Skyward Sword* look. Dark forest and olive greens read as "realistic OoT", not pastel. | `rules_config::rule_no_dark_greens` (palette math, randomized), `rules_render::rule_no_dark_greens` (rendered), `rules_config::rule_styles_stay_within_the_contract` (green floor ≥ `min_green_floor`) |
| **Pastel floor for every hue.** No texel is darker than the style's global floor. Accent darks are the only exception. | Light pastel overall: no dark reds, blues, purples or browns. | `rule_all_hue_pastel_floor` (CPU), `rule_all_hue_pastel_floor_except_accents` (GPU) |
| **Compress, don't clamp.** Lightness is mapped monotonically, so value order survives. | Shapes stay readable. Flattening everything to the floor would erase form. | `rule_value_order_preserved`, `lut_matches_the_mapping_outside_the_darkest_cell` |
| **Chroma is capped** (`chroma_cap`, plus a bounded vivid allowance). | Soft pastel color. The occasional vivid accent is allowed, but it stays bounded. | `rule_chroma_is_capped`, `rule_vivid_colors_are_bounded` |
| **Only toward pastel.** Palette `strength` is at least 1 (1 = the SS-measured look). | `strength < 1` would lower the floors below the rules. | `rule_styles_stay_within_the_contract` |
| **Identity at zero.** Strength 0 with every effect off leaves the image unchanged. A neutral (empty) config builds no GPU stage at all. | Makes it safe to reason about each effect in isolation. | `rule_palette_is_identity_at_zero_strength`, `rule_identity_when_all_strengths_are_zero`, `rule_neutral_config_is_identity_without_a_gpu` |

### Accent darks (Impressionist colored shadows)

| Rule | Why | Enforced by |
|---|---|---|
| Accents are bounded by `accent_fraction` (≤ the contract's `max_fraction`). They come from high-frequency detail only: crevices and gaps, never low-frequency shading. | Occasional contrast, not a return to dark textures. | `rule_all_hue_pastel_floor_except_accents`, contract check |
| Accents are cool: their hue is inside the contract's accent band (blue to violet). They are never black, brown or green, and never darker than `accent_min_l`. | The Impressionist rule: shadows take the cool complement, with no muddy darks. A green texel on its way to an accent leaves the green family before it darkens. | `rule_accents_are_never_green`, contract check |

### Target: SoH cel-shade fork

See PLAN.md, "Target notes". The toon shader multiplies actor textures by a lit factor that can
exceed 1, and it decides at runtime which side is lit and which is in shadow.

| Rule | Why | Enforced by |
|---|---|---|
| **Actor lightness ceiling.** Actor output never exceeds the target's `lightness_ceiling` (≤ `max_actor_ceiling`), including vivid colors. Vivid is achieved through chroma, never lightness. | Pale pastels would clip and hue-shift under a lit multiplier above 1. | `rule_actor_lightness_ceiling`, `rule_actor_targets_leave_lighting_to_the_renderer` |
| **No baked temperature or shadow tint on actors** (`warm_cool = 0`, `shadow_tint = 0`). | The cel shader decides lit and shadow at runtime. Baked warm/cool would fight it. | `rule_actor_has_no_temperature_shift`, `rule_actor_targets_leave_lighting_to_the_renderer` |
| **Tint-safe textures stay gray.** Grayscale-origin textures (tinted by the engine, such as Link's tunic or HUD hearts) get lightness changes only, never an added hue. | The engine multiplies them by a tint color. Any baked hue would corrupt every tint. | `rule_tint_safe_grayscale_stays_gray` |

### Filter technique

| Rule | Why | Enforced by |
|---|---|---|
| **No blur.** A hard step edge stays within `max_edge_width` texels, while texel-level grit in flat regions decreases. | Edge-preserving, not blurring. This is the project's first principle. | `rule_no_blur_edges_stay_crisp_noise_becomes_flat` |
| **Alpha is preserved exactly, with no halos.** Opaque texels at a cutout border are not darker than the interior. | Foliage and cutouts must drop back into the game unchanged in shape, without dark fringes. | `rule_alpha_preserved_and_no_halos` |
| **Tiling textures stay seamless.** | Wrap addressing and tileable noise. A seam would repeat across every wall and floor. | `rule_tiling_textures_stay_seamless` |
| **Chunking is invisible.** Textures above the GPU limit are processed in overlapping chunks that match whole-image processing. | 8K skyboxes and larger textures must not show chunk grids. | `rule_chunked_processing_matches_whole_image` |
| Technique strengths stay within the contract's `[technique]` bounds. | Keeps the watercolor finish tasteful: no ink outlines or heavy grain. | `rule_styles_stay_within_the_contract` |

### Robustness

| Rule | Enforced by |
|---|---|
| Deterministic: the same input and config give identical output. | `rule_deterministic` |
| Output is in the sRGB gamut, with no NaNs. 16-bit inputs are handled. | `rule_output_in_gamut_and_finite`, `rule_palette_output_is_in_gamut_and_finite`, `rule_sixteen_bit_inputs_are_handled` |
| Configs are strict: unknown keys are rejected, and contradictory ranges (floor > ceiling, a non-monotone tone curve) fail with a clear error. | `rule_unknown_keys_are_rejected`, `rule_invalid_ranges_are_rejected`, `config::tests::contradictory_settings_are_rejected_with_clear_errors` |
| LUT `.cube` round-trip and OKLCH conversions are accurate. | `cube_round_trip`, `oklch_round_trip_is_accurate`, unit tests in `src/color.rs` and `src/lut.rs` |
