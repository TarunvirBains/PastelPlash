# PastelPlash style rules

**North star:** it still looks and feels like the source game — for OoT, like OoT — just a
dreamlike version evoking impressionism and watercolor, in the direction of *Skyward Sword*. It
must never feel like a different game. The restyle is a **layer** (painted washes, brushwork, wet
edges, grain, softer internal contrast, colored darks), not a repaint.

These rules define PastelPlash's style. A contribution to this repository must keep them passing.
If you want a different aesthetic, you are welcome to fork the project and change the rules and
tests to match your style.

## Rules, tuning and snapshots

There are four kinds of change, and each has its own place:

| Change | Where | Review |
|---|---|---|
| **Tuning** a style (strengths, sizes, hue nudges, curves inside the bounds) | `styles/*.toml`, `targets/*.toml` | Free. No rule-test edits needed; the golden hashes change (re-capture them). |
| **Changing a rule** (a hard limit of the style) | `rules.toml` (the *style contract*) | Deliberate, in its own commit. |
| **Intended change of look** | re-bless snapshots: `PASTELPLASH_BLESS=1 cargo test --test snapshots`, re-capture golden hashes | Look at the new goldens before committing; tag the commit `look-change:`. |
| **Pure refactor** (restructuring code, no intended output change) | anywhere in `src/` | The golden hashes must stay **byte-identical**. |

- **The contract.** `rules.toml` holds the hard limits as bounds, not exact values. The rule
  tests read every limit from it and contain no magic numbers.
- **Every style, every mood.** The tests discover every file in `styles/` (following `extends`)
  and every file in `targets/`, and check each style in the base mood and in every mood it
  defines (at half and full strength). A new style or mood is checked automatically. They check:
  1. The style's parameters lie within the contract (fast, CPU only).
  2. The palette mapping honors the rules over randomized colors (CPU only, proptest).
  3. Rendered output honors the rules on procedural textures (GPU, `tests/rules/`): each rule
     runs over a matrix of style × mood × category and reports every failing case at once.
     Known failures awaiting a decision are listed in `EXPECTED_FAILURES`
     (`tests/rules/main.rs`) and printed on every run instead of failing it; the list is empty.
- **Snapshots are alarms, not rules.** `tests/snapshots.rs` renders a few procedural textures with
  the default style (`impressionist`) and compares them with `tests/golden/` using a perceptual tolerance (mean
  OKLab ΔE plus a small outlier budget). A failure means the look changed; if intended, re-bless.
  - When running the Windows build from WSL, pass the variable through:
    `WSLENV=PASTELPLASH_BLESS PASTELPLASH_BLESS=1 cargo test --test snapshots`.
- **Golden hashes prove refactors.** `tests/golden_hash.rs` compares exact FNV-1a hashes: on the
  CPU (`tests/golden/hashes-cpu.toml`) every resolved style × mood, the targets and pack maps,
  every palette LUT, every job's plan (uniform bytes, low-res field, LUT, filter reach) and the
  HLSL naga generates from the shader; on the GPU (`tests/golden/hashes-<adapter>.toml`, checked
  only on the adapter that captured it) the f32, PNG8 and PNG16 output of every style × mood ×
  category × procedural image, plus chunking, 16-bit files, a fixture pack (categories, moods,
  marks, opt-outs, `-j1` = `-jN`), neutral configs, an external `.cube`, OTEX and an `.o2r` run.
  A changed hash or case set fails. Capture (only for a tuning or an intended change of look):
  `WSLENV=PASTELPLASH_GOLDEN_CAPTURE PASTELPLASH_GOLDEN_CAPTURE=1 cargo test --release --test golden_hash`.
  `PASTELPLASH_GOLDEN_DIR=<exported PNGs>` also checks your own textures (hashes kept under
  `target/golden-user/`, never committed).
  - Byte identity holds only while the generated shader code is unchanged. If a refactor must
    change it (the HLSL hash changes) and output bits move, revert it, or apply the fallback in a
    commit tagged `look-change:` with the reason: CPU hashes (styles, LUTs, plans) exact; f32
    within 4 ULP per channel; PNG8 exact in ≥ 99.99% of texels and within 1 LSB elsewhere; every
    rule passes. Then re-capture.
  - A new driver, wgpu or toolchain can move bits too: the manifest in the hashes file records
    adapter, driver, wgpu, toolchain and shader compiler, and a failure names what drifted.
- **GPU tests skip cleanly** without an adapter (for example in CI); the CPU checks always run.
- **No third-party images.** Test textures are generated procedurally. Never commit texture pack,
  game or museum images to this repository.

## Styles and moods

| Style | What it is |
|---|---|
| `watercolor` (base) | The source's own rich color, painted: gentle SS hue nudges, crushed darks lifted into colored shadows, softer internal value contrast, watercolor technique. |
| `impressionist` (**default**) | `watercolor` + the impressionist brushwork overlay (`styles/overlays/impressionist-brushwork.toml`): bolder brushwork, slightly stronger warm/cool, a few more accents. The CLI uses it when no `--style` is given; `make-mod.sh` installs it. |
| `ss-baseline` | Extends `watercolor`; nudged further toward Skyward Sword (stronger hue pulls, mild lift). |
| `ss-impressionist` | `ss-baseline` + the same brushwork overlay: SS palette, impressionist brushwork. |

Styles compose: `extends` takes a path or a list (`extends = ["ss-baseline.toml",
"overlays/impressionist-brushwork.toml"]`), merged in order, then the file itself; palette
groups (arrays of tables) merge by `name`, so a layer names only the groups it changes (see
ARCHITECTURE.md). Overlays in `styles/overlays/` are not styles on their own and are not tested
alone. Every shipped style is also built into the binary (`--style impressionist` works from
anywhere).

A **mood** is a named partial override of a style (`[moods.<name>]` in the style file), assigned
to files by the pack map (`[[moods]]` rules with a glob and a strength 0–1). The style as written
is the `base` mood. Moods are checked by the same rules, at half and full strength.

- **Moods are overlays.** A mood's effective settings equal the base style's for every key it
  doesn't name, so base improvements flow into it (`moods_inherit_everything_they_do_not_override`).
- **Nocturne** (dark, haunting areas: Deku Tree (milder), Forest/Shadow Temple, Bottom of the
  Well, Ganon's Castle, graves, ruined Castle Town) is **a shared moonlight cast**
  (`palette.cast`): every color shifts the same way — darker (a lower exposure), proportionally
  less saturated, nudged by one shared a/b vector toward the cast hue — so hue *differences*
  survive (moss stays greener than wood). The cast is applied per texel after the palette LUT
  (`palette::apply_cast`, `finish_cast`). Near-black and near-neutral darks take at most a muted
  midnight (small lift, chroma capped by lightness); warm darks are held at the no-mud chroma
  instead; earth warmth, the night's tone curve and floor are held at full value whatever the
  mood's strength (`hold`). The cast hue (default indigo) and strength can be set per area in the
  pack map (`cast = "midnight-purple"`, `cast_strength`). Only moods listed in `rules.toml` with
  `min_exposure` may have a cast (`rule_moonlight_cast_only_where_the_mood_allows`); rules on
  value and identity judge such a mood against the source dimmed by its declared exposure
  (`dimmed` in the rule tests). Actors get no cast (target `cast = 0`: the renderer lights
  them). A cool bias on darks (`dark_cool_bias`) is still bounded per mood by `max_cool_bias`
  and `dark_max_hue_shift`; the base look keeps darks hue-true
  (`rule_cool_darks_only_where_the_mood_allows`). Darks still never go neutral black or brown
  mud, and the identity rules still apply: dungeons stay recognizably themselves, just dimmer
  and more atmospheric.

## The rules

All lightness (L) and chroma (C) values are OKLCH. "Tolerance" means `[tolerance]` in `rules.toml`.

### Identity

| Rule | Why | Enforced by |
|---|---|---|
| **Mean color stays.** Each texture's alpha-weighted mean OKLab color stays within `identity.max_mean_delta_e` of the source's (`ss-baseline`, which moves further toward SS, has its own larger bound). | Kokiri green stays Kokiri green; Death Mountain stays brown. The game must stay recognizable. | `rules::rule_identity_is_kept` |
| **Hue families stay.** Per hue group, the mean hue moves by at most `identity.max_group_hue_shift`; any colored texel by at most `palette.max_hue_shift`. | SS hue nudges are nudges, not a repaint. | `rule_identity_is_kept`, `rules_config::rule_hue_shifts_are_bounded` |
| **Recognizable from across the room.** On a 16×16 grid, each cell's dark-half and light-half mean colors (split at the cell's median L) stay close to the source's: chroma change p90 ≤ `identity.coarse_max_color`, lightness change p90 ≤ `identity.coarse_max_lightness` (per-style overrides for opt-in looks). Brushwork only moves texels within a cell and doesn't register. | A mean color can match while the texture is transformed: v2's navy grooves over tan averaged back to brown. The half-means don't. | `rule_coarse_identity_is_kept` |
| **Hue families survive.** On a two-material texture (green moss over brown wood), each family's share of the texels changes by at most `identity.family_share_max_change` and their hue separation keeps ≥ `identity.family_min_separation` of the source's, in every mood. | A moonlight that shifts every color the same way keeps the moss greener than the wood; a pull that repaints one family into the other doesn't. | `rule_hue_families_survive` |
| **Warmth is targeted.** Earth warmth changes only sources whose hue is in its band (exactly nothing outside it), weighted by chroma, and never pushes an earth hue past its target. | A safe, deterministic warm-up for OoT's olive ground, unlike an untargeted tint. | `rule_warmth_is_targeted`, `rule_warmth_stays_in_band` |
| **Pastel is not gray.** A clearly colored source keeps at least `retention_ratio` of its chroma (capped at `retention_floor`), in every style. | The failure we saw in-game: lifted colors went chalky. Light must stay colorful. | `rules_config::rule_pastel_is_not_gray`, `rules::rule_value_contrast_is_compressed_color_is_kept` |

### Darks

| Rule | Why | Enforced by |
|---|---|---|
| **No crushed black:** nothing (except accent darks) maps below `palette.min_l`. | Watercolor darks are deep transparent washes, not black. | `rule_darks_are_colored_never_black` (CPU and GPU) |
| **Darks are colored:** below `dark_l`, output carries at least `dark_min_chroma` — the source's own hue when it has one, else a warm umber (SS's measured shadow hue). | Painted shadows, never neutral near-black. | same |
| **Lifted darks keep their hue:** a dark, colored source moves by at most `palette.dark_max_hue_shift`; dark bark never comes out blue. | v2 lifted OoT's bark and cliff darks toward navy: "a completely different place". Dark brown stays brown, dark green stays green. | `rule_lifted_darks_keep_their_hue`, `rule_bark_does_not_turn_blue` |
| **No brown mud:** no dark, dull brown/olive output (`mud_l`, `mud_hue`, `mud_max_chroma`). | Mud is the classic watercolor failure. | `rule_no_brown_mud` (CPU and GPU) |
| **Near-black darks take a muted midnight** (moods with a cast): a near-black, near-neutral source (`moods.<name>.near_black_l`, `near_neutral_c`) is lifted at most `near_black_max_lift` above max(its L, `min_l`), and unless held at the no-mud chroma as a warm dark, carries at most `dark_chroma_per_l × L`. | Midnight is fine, ink is not: the Deku Tree's near-neutral darks turned saturated teal; fades to black must stay the darkest part. | `rule_near_black_darks_take_a_muted_midnight` |
| **Moonlight only where the mood allows.** The base look has no cast; a mood's cast needs `min_exposure` in the contract and dims at most down to it. Actors never get one (`target.max_actor_cast`). | The night is an intended, bounded change of exposure; everything else is judged against it. | `rule_moonlight_cast_only_where_the_mood_allows`, `rule_actor_targets_leave_lighting_to_the_renderer` |
| **Accent darks** are bounded (`accents.max_fraction`), cool (hue in `accents.hue`), never below `accents.min_l`, and come from structural crevices (band-pass measure), not fine noise. | Occasional Impressionist contrast, not dark textures. | `rule_styles_stay_within_the_contract` |

### Value and technique

| Rule | Why | Enforced by |
|---|---|---|
| **Softer internal contrast.** Fine-scale light/dark variation drops by at least `value_min_effect × value_contrast.fine`, while the texture's mean L and chroma stay. | SS surfaces sit in a narrow lightness range (measured: ~0.4× OoT Reloaded's local L std); detail moves into color and brushwork. | `rule_value_contrast_is_compressed_color_is_kept` |
| **Busy textures calm down.** With adaptive compression on, a high-contrast (bark-like) texture's mid-scale L std drops by at least `adaptive_min_effect`; textures below the style's target spread (the ground) are untouched by adaptivity. | OoT Reloaded's photographic grooves clash with flat cel-shaded characters; keep the big groove shapes, drop the photographic depth. | `rule_adaptive_contrast_targets_high_contrast_textures` |
| **Value grouping (notan) for busy textures only.** Busy, photographic world textures (and, gentler, pre-rendered backgrounds) are simplified into 2–4 value masses found on an edge-aware smoothed lightness; values within a mass move toward the mass, the masses keep their separation and the coarse pattern stays. Actors are never grouped (the cel shader bands them), and calm, shape-based textures are untouched. | Big readable value shapes like a painter's notan, instead of photographic grit; no double banding on actors. | `rule_grouping_forms_value_masses`, `rule_grouping_only_touches_busy_world_textures` |
| **Small objects and lettering stay.** Thin sticks and small bright shapes on a busy wall keep ≥ `small_object_min_contrast` of their contrast; small lettering on a busy sign keeps ≥ `text_min_contrast`. Signs can also be opted out of grouping and abstraction in the pack map (`no_grouping`, `no_abstraction`). | Abstraction and grouping simplify texture, never content: tools, bowls, signs and symbols must stay readable. | `rule_small_objects_survive`, `rule_text_stays_legible`, `packs::oot_reloaded_signs_are_never_grouped_or_abstracted` |
| **Backgrounds keep their exposure.** Pre-rendered rooms keep the murk lift (crushed darks become readable colored shadow), then a smooth monotone tone curve (slope ≥ `exposure::MIN_SLOPE`, black and white fixed) restores the source's mean lightness within `identity.background_max_mean_l`; the coarse light/dark range keeps ≥ `identity.background_min_pattern_range`. Per category: `exposure.preserve_mean` in the target. | The designers painted the room's lighting and mood; a lifted room felt too light in-game. | `rule_backgrounds_keep_their_exposure`, `rule_exposure_curve_is_monotone_and_restores_the_mean` |
| **No blur.** A hard step edge stays within `max_edge_width` texels while texel-level grit in flat regions decreases. | Edge-preserving, not blurring — "dreamlike" never means blurry. | `rule_no_blur_edges_stay_crisp_noise_becomes_flat` |
| **Value order preserved** (monotone lightness mapping). | Shapes stay readable. | `rule_value_order_preserved`, `lut_matches_the_mapping_outside_the_darkest_cell` |
| **Alpha preserved exactly, no halos.** | Cutouts must drop back into the game unchanged in shape. | `rule_alpha_preserved_and_no_halos` |
| **Tiling textures stay seamless; chunking is invisible.** | Seams repeat across every wall; 8K skyboxes are processed in chunks. | `rule_tiling_textures_stay_seamless`, `rule_chunked_processing_matches_whole_image` |
| Technique strengths stay within `[technique]`. | Tasteful: no ink outlines, heavy grain or smeared mush. | `rule_styles_stay_within_the_contract` |
| `impressionist` and `ss-impressionist` are their palette base plus the brushwork overlay, which may only set brushwork, warm/cool and accents; built-in styles equal their files. | Tuning a base carries over; the brushwork is one shared layer, never a hand-copied variant. | `impressionist_inherits_the_default_look`, `ss_impressionist_is_ss_baseline_palette_with_impressionist_brushwork`, `builtin_styles_are_the_shipped_files` |

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
