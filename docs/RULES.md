# PastelPlash style rules

**North star:** it still looks and feels like the source game — for OoT, like OoT — just a
dreamlike version evoking impressionism and watercolor, in the direction of *Skyward Sword*. It
must never feel like a different game. The restyle is a **layer** (painted washes, brushwork, wet
edges, grain, softer internal contrast, colored darks), not a repaint.

These rules define PastelPlash's style. A contribution to this repository must keep them passing.
If you want a different aesthetic, you are welcome to fork the project and change the rules and
tests to match your style.

## Rules, tuning and snapshots

There are three kinds of change, and each has its own place:

| Change | Where | Review |
|---|---|---|
| **Tuning** a style (strengths, sizes, hue nudges, curves inside the bounds) | `styles/*.toml`, `targets/*.toml` | Free. No test edits needed. |
| **Changing a rule** (a hard limit of the style) | `rules.toml` (the *style contract*) | Deliberate, in its own commit. |
| **Intended change of look** | re-bless snapshots: `PASTELPLASH_BLESS=1 cargo test --test snapshots` | Look at the new goldens before committing. |

- **The contract.** `rules.toml` holds the hard limits as bounds, not exact values. The rule
  tests read every limit from it and contain no magic numbers.
- **Every style, every mood.** The tests discover every file in `styles/` (following `extends`)
  and every file in `targets/`, and check each style in the base mood and in every mood it
  defines (at half and full strength). A new style or mood is checked automatically. They check:
  1. The style's parameters lie within the contract (fast, CPU only).
  2. The palette mapping honors the rules over randomized colors (CPU only, proptest).
  3. Rendered output honors the rules on procedural textures (GPU).
- **Snapshots are alarms, not rules.** `tests/snapshots.rs` renders a few procedural textures with
  the default style and compares them with `tests/golden/` using a perceptual tolerance (mean
  OKLab ΔE plus a small outlier budget). A failure means the look changed; if intended, re-bless.
  - When running the Windows build from WSL, pass the variable through:
    `WSLENV=PASTELPLASH_BLESS PASTELPLASH_BLESS=1 cargo test --test snapshots`.
- **GPU tests skip cleanly** without an adapter (for example in CI); the CPU checks always run.
- **No third-party images.** Test textures are generated procedurally. Never commit texture pack,
  game or museum images to this repository.

## Styles and moods

| Style | What it is |
|---|---|
| `watercolor` (default) | The source's own rich color, painted: gentle SS hue nudges, crushed darks lifted into colored shadows, softer internal value contrast, watercolor technique. |
| `impressionist` | Extends `watercolor`; differs only in bolder brushwork, slightly stronger warm/cool and a few more accents. |
| `ss-baseline` | Extends `watercolor`; nudged further toward Skyward Sword (stronger hue pulls, mild lift). |
| `pastel` (opt-in) | Light and dreamy: every hue lifted toward SS's pastel lightness, at SS's chroma — never gray. Moves furthest from the source, so it has its own identity bound. |

A **mood** is a named partial override of a style (`[moods.<name>]` in the style file), assigned
to files by the pack map (`[[moods]]` rules with a glob and a strength 0–1). The style as written
is the `base` mood. Moods are checked by the same rules.

## The rules

All lightness (L) and chroma (C) values are OKLCH. "Tolerance" means `[tolerance]` in `rules.toml`.

### Identity

| Rule | Why | Enforced by |
|---|---|---|
| **Mean color stays.** Each texture's alpha-weighted mean OKLab color stays within `identity.max_mean_delta_e` of the source's (opt-in styles such as `pastel` have their own larger bound). | Kokiri green stays Kokiri green; Death Mountain stays brown. The game must stay recognizable. | `rules_render::rule_identity_is_kept` |
| **Hue families stay.** Per hue group, the mean hue moves by at most `identity.max_group_hue_shift`; any colored texel by at most `palette.max_hue_shift`. | SS hue nudges are nudges, not a repaint. | `rule_identity_is_kept`, `rules_config::rule_hue_shifts_are_bounded` |
| **Recognizable from across the room.** On a 16×16 grid, each cell's dark-half and light-half mean colors (split at the cell's median L) stay close to the source's: chroma change p90 ≤ `identity.coarse_max_color`, lightness change p90 ≤ `identity.coarse_max_lightness` (per-style overrides for opt-in looks). Brushwork only moves texels within a cell and doesn't register. | A mean color can match while the texture is transformed: v2's navy grooves over tan averaged back to brown. The half-means don't. | `rule_coarse_identity_is_kept` |
| **Warmth is targeted.** Earth warmth changes only sources whose hue is in its band (exactly nothing outside it), weighted by chroma, and never pushes an earth hue past its target. | A safe, deterministic warm-up for OoT's olive ground, unlike an untargeted tint. | `rule_warmth_is_targeted`, `rule_warmth_stays_in_band` |
| **Pastel is not gray.** A clearly colored source keeps at least `retention_ratio` of its chroma (capped at `retention_floor`), in every style. | The failure we saw in-game: lifted colors went chalky. Light must stay colorful. | `rules_config::rule_pastel_is_not_gray`, `rules_render::rule_value_contrast_is_compressed_color_is_kept` |

### Darks

| Rule | Why | Enforced by |
|---|---|---|
| **No crushed black:** nothing (except accent darks) maps below `palette.min_l`. | Watercolor darks are deep transparent washes, not black. | `rule_darks_are_colored_never_black` (CPU and GPU) |
| **Darks are colored:** below `dark_l`, output carries at least `dark_min_chroma` — the source's own hue when it has one, else a warm umber (SS's measured shadow hue). | Painted shadows, never neutral near-black. | same |
| **Lifted darks keep their hue:** a dark, colored source moves by at most `palette.dark_max_hue_shift`; dark bark never comes out blue. | v2 lifted OoT's bark and cliff darks toward navy: "a completely different place". Dark brown stays brown, dark green stays green. | `rule_lifted_darks_keep_their_hue`, `rule_bark_does_not_turn_blue` |
| **No brown mud:** no dark, dull brown/olive output (`mud_l`, `mud_hue`, `mud_max_chroma`). | Mud is the classic watercolor failure. | `rule_no_brown_mud` (CPU and GPU) |
| **Accent darks** are bounded (`accents.max_fraction`), cool (hue in `accents.hue`), never below `accents.min_l`, and come from structural crevices (band-pass measure), not fine noise. | Occasional Impressionist contrast, not dark textures. | `rule_styles_stay_within_the_contract` |

### Value and technique

| Rule | Why | Enforced by |
|---|---|---|
| **Softer internal contrast.** Fine-scale light/dark variation drops by at least `value_min_effect × value_contrast.fine`, while the texture's mean L and chroma stay. | SS surfaces sit in a narrow lightness range (measured: ~0.4× OoT Reloaded's local L std); detail moves into color and brushwork. | `rule_value_contrast_is_compressed_color_is_kept` |
| **Busy textures calm down.** With adaptive compression on, a high-contrast (bark-like) texture's mid-scale L std drops by at least `adaptive_min_effect`; textures below the style's target spread (the ground) are untouched by adaptivity. | OoT Reloaded's photographic grooves clash with flat cel-shaded characters; keep the big groove shapes, drop the photographic depth. | `rule_adaptive_contrast_targets_high_contrast_textures` |
| **No blur.** A hard step edge stays within `max_edge_width` texels while texel-level grit in flat regions decreases. | Edge-preserving, not blurring — "dreamlike" never means blurry. | `rule_no_blur_edges_stay_crisp_noise_becomes_flat` |
| **Value order preserved** (monotone lightness mapping). | Shapes stay readable. | `rule_value_order_preserved`, `lut_matches_the_mapping_outside_the_darkest_cell` |
| **Alpha preserved exactly, no halos.** | Cutouts must drop back into the game unchanged in shape. | `rule_alpha_preserved_and_no_halos` |
| **Tiling textures stay seamless; chunking is invisible.** | Seams repeat across every wall; 8K skyboxes are processed in chunks. | `rule_tiling_textures_stay_seamless`, `rule_chunked_processing_matches_whole_image` |
| Technique strengths stay within `[technique]`. | Tasteful: no ink outlines, heavy grain or smeared mush. | `rule_styles_stay_within_the_contract` |
| `impressionist` extends the default and only overrides brushwork, warm/cool and accents. | Tuning the default carries over; impressionist is never "OoT with brushstrokes". | `impressionist_inherits_the_default_look` |

### Target: SoH cel-shade fork

See PLAN.md, "Target notes". The toon shader multiplies actor textures by a lit factor that can
exceed 1, and it decides at runtime which side is lit and which is in shadow.

| Rule | Why | Enforced by |
|---|---|---|
| **Actors keep their color and value:** colored actor texels keep ≥ `actor.retention_ratio` of their chroma and move by at most `actor.max_lightness_shift` (beyond the ceiling). Actor de-light, lift and hue nudges are bounded (`target.max_actor_*`). | Warm skin, golden hair, brown leather — an early build turned Link's skin gray. | `rule_actor_keeps_color_and_value`, `rule_actor_targets_leave_lighting_to_the_renderer` |
| **Actor lightness ceiling** (≤ `max_actor_ceiling`), vivid colors included; vividness via chroma, never lightness. | Pale colors would clip under a lit multiplier above 1. | `rule_actor_lightness_ceiling` |
| **No baked temperature or shadow tint on actors.** | The cel shader decides lit and shadow at runtime. | `rule_actor_has_no_temperature_shift` |
| **Tint-safe textures stay gray:** grayscale-origin textures (engine-tinted, e.g. Link's tunic, hearts) get lightness changes only. | The engine multiplies them by a tint; any baked hue would corrupt every tint. | `rule_tint_safe_grayscale_stays_gray` |

### Robustness

| Rule | Enforced by |
|---|---|
| Deterministic: the same input and config give identical output. | `rule_deterministic` |
| Output is in the sRGB gamut, with no NaNs; 16-bit inputs are handled. | `rule_output_in_gamut_and_finite`, `rule_palette_output_is_in_gamut_and_finite`, `rule_sixteen_bit_inputs_are_handled` |
| Identity when every strength is 0; a neutral config builds no GPU stage. | `rule_palette_is_identity_at_zero_strength`, `rule_identity_when_all_strengths_are_zero`, `rule_neutral_config_is_identity_without_a_gpu` |
| Configs are strict: unknown keys rejected, contradictory ranges fail with a clear error, every mood validated on load. | `rule_unknown_keys_are_rejected`, `rule_invalid_ranges_are_rejected`, `config::tests` |
| LUT `.cube` round-trip and OKLCH conversions are accurate. | `cube_round_trip`, `oklch_round_trip_is_accurate`, unit tests in `src/color.rs` and `src/lut.rs` |
