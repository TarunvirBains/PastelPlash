| Engine-tinted gray water stays gray and darkens by ≤ `tint_safe_max_darkening` beyond the mood. | The engine supplies the color and multiplies the value.# PastelPlash style rules

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
| `ss-terracotta` | `ss-baseline` + the terracotta layer (`styles/overlays/terracotta.toml`): Skyward Sword's Faron Woods earth, a warm rose-sienna on grain-free, mostly earth, mid-value brown world textures (dirt, clods, rock), per texture. Bark, planks, light sand, moss, actors, backgrounds, fluids, skies and UI keep their color; off in nocturne. The pack map may opt areas in or out (`terracotta_only`, `no_terracotta`). |
| `ss-terracotta-impressionist` | `ss-terracotta` + the brushwork overlay. |

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
  midnight (small lift, chroma capped by lightness); near-black sources of any color just go
  darker (below `cast.black` their chroma fades to `cast.black_chroma`: the midnight tints the
  mid-darks, never the fade into black); warm darks are held at the no-mud chroma instead; earth
  warmth, the night's tone curve and floor are held at full value whatever the
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
  Dark rooms are no camouflage: the day's bigger dabs on tiling, speckled ground (3×) are held
  to `moods.<name>.max_tiling_multiplier` at any strength (`rule_dark_rooms_are_no_camouflage`):
  the Deku Tree basement's mottled floor had turned into big hard-edged blotches.

## The rules

All lightness (L) and chroma (C) values are OKLCH. "Tolerance" means `[tolerance]` in `rules.toml`.

### Identity

| Rule | Why | Enforced by |
|---|---|---|
| **Mean color stays.** Each texture's alpha-weighted mean OKLab color stays within `identity.max_mean_delta_e` of the source's (`ss-baseline`, which moves further toward SS, has its own larger bound). | Kokiri green stays Kokiri green; Death Mountain stays brown. The game must stay recognizable. | `rules::rule_identity_is_kept` |
| **Hue families stay.** Per hue group, the mean hue moves by at most `identity.max_group_hue_shift`; any colored texel by at most `palette.max_hue_shift`. | SS hue nudges are nudges, not a repaint. | `rule_identity_is_kept`, `rules_config::rule_hue_shifts_are_bounded` |
| **Recognizable from across the room.** On a 16×16 grid, each cell's dark-half and light-half mean colors (split at the cell's median L) stay close to the source's: chroma change p90 ≤ `identity.coarse_max_color`, lightness change p90 ≤ `identity.coarse_max_lightness` (per-style overrides for opt-in looks). Brushwork only moves texels within a cell and doesn't register. | A mean color can match while the texture is transformed: v2's navy grooves over tan averaged back to brown. The half-means don't. | `rule_coarse_identity_is_kept` |
| **Hue families survive.** On a two-material texture (green moss over brown wood), each family's share of the texels changes by at most `identity.family_share_max_change` and their hue separation keeps ≥ `identity.family_min_separation` of the source's, in every mood. | A moonlight that shifts every color the same way keeps the moss greener than the wood; a pull that repaints one family into the other doesn't. | `rule_hue_families_survive` |
| **Moss is not warmed into brown.** An olive moss texture (hues in `identity.moss_hue`) moves toward the warm earth hues by at most `identity.moss_max_warm_shift` degrees on average. | Earth warmth is for earth: the v5 band (50–115°) turned OoT Reloaded's olive mossy ground tan. The band now fades out before olive (full to 90°, none from 100°): Kokiri Forest's golden vines (~90°) stay golden. | `rule_moss_is_not_warmed_into_brown` |
| **World and sky colors bring no new hue.** A world texel colored at least `identity.new_hue_min_chroma` stays within `identity.new_hue_max_gap` degrees of the hue of some texel of its 5×5 source neighborhood colored at least half that. The only exception is a mood's moonlight cast: near the cast hue, up to `moods.<name>.cast_max_chroma`. | Navy flecks on Kokiri Forest's warm treehouse bark and the Deku Tree's olive moss (v6a2): cool accent darks on small pits, which once the grit around them was cleaned stood alone as blue dots. Skies too: the umber floor for neutral darks turned the black gaps of the night and sunset skies brown (the target gives skies no dark chroma floor). | `rule_world_colors_bring_no_new_hue`, `rule_sky_colors_bring_no_new_hue` |
| **Warmth is targeted.** Earth warmth changes only sources whose hue is in its band (exactly nothing outside it), weighted by chroma, and never pushes an earth hue past its target. | A safe, deterministic warm-up for OoT's olive ground, unlike an untargeted tint. | `rule_warmth_is_targeted`, `rule_warmth_stays_in_band` |
| **Terracotta only on grain-free earth.** With a terracotta layer, a mid-value, grain-free earth world texture's mean earth hue turns at least `terracotta.min_shift` degrees toward the target (in `terracotta.hue`); bark and wood, light sand, olive moss and every other category render exactly as without it. | Faron Woods' warm earth without turning the forest's wood or the desert's sand red; never on actors or finished paintings. | `rule_terracotta_only_on_grain_free_earth`, `config::style::stack_tests` |
| **Pastel is not gray.** A clearly colored source keeps at least `retention_ratio` of its chroma (capped at `retention_floor`), in every style. | The failure we saw in-game: lifted colors went chalky. Light must stay colorful. | `rules_config::rule_pastel_is_not_gray`, `rules::rule_value_contrast_is_compressed_color_is_kept` |

### Darks

| Rule | Why | Enforced by |
|---|---|---|
| **No crushed black:** nothing (except accent darks) maps below `palette.min_l`. | Watercolor darks are deep transparent washes, not black. | `rule_darks_are_colored_never_black` (CPU and GPU) |
| **Darks are colored:** below `dark_l`, output carries at least `dark_min_chroma` (not for the `palette.colored_darks_exempt` categories: actors, whose shading the cel shader supplies; their near-neutral darks keep their own faint hue, target actor `dark_chroma = 0`; nor, in a mood with a cast, for sources below `moods.<name>.black_fade_l`, which just go darker) — the source's own hue when it has one, else a warm umber (SS's measured shadow hue). | Painted shadows, never neutral near-black. | same |
| **Lifted darks keep their hue:** a dark, colored source moves by at most `palette.dark_max_hue_shift`; dark bark never comes out blue. | v2 lifted OoT's bark and cliff darks toward navy: "a completely different place". Dark brown stays brown, dark green stays green. | `rule_lifted_darks_keep_their_hue`, `rule_bark_does_not_turn_blue` |
| **No brown mud:** no dark, dull brown/olive output (`mud_l`, `mud_hue`, `mud_max_chroma`); for actors (`palette.mud_relative`) the rule is relative: they introduce no mud (an output may be in the band only if the source was, within `mud_relative_margin`/`mud_relative_hue`, or it is the source's own color within `mud_relative_max_dc`/`mud_relative_max_dh`: dull leather stays leather, Link's green never turns to mud). | Mud is the classic watercolor failure. | `rule_no_brown_mud` (CPU and GPU) |
| **Near-black darks take a muted midnight** (moods with a cast): a near-black, near-neutral source (`moods.<name>.near_black_l`, `near_neutral_c`) is lifted at most `near_black_max_lift` above max(its L, `min_l`), and unless held at the no-mud chroma as a warm dark, carries at most `dark_chroma_per_l × L`. | Midnight is fine, ink is not: the Deku Tree's near-neutral darks turned saturated teal; fades to black must stay the darkest part. | `rule_near_black_darks_take_a_muted_midnight` |
| **Near-black colors just go darker** (moods with a cast): a near-black source of any color (L below `moods.<name>.black_l`) comes out with chroma at most `black_max_chroma` and moves toward the cast hue by at most `black_max_cast` (OKLab a/b along it), unless held at the no-mud chroma as a warm dark (the cast's warm band, give or take `black_warm_slack`). Above `black_fade_l` colored sources keep their color again (pastel is not gray). | The Deku Tree's central hole: its dark green cobbles read teal-blue going down into the dark (the cast kept their chroma and pushed it toward blue-teal); a pit should just go darker. | `rule_near_black_colors_just_go_darker`, `rules_config::rule_pastel_is_not_gray` |
| **Moonlight only where the mood allows.** The base look has no cast; a mood's cast needs `min_exposure` in the contract and dims at most down to it. Actors never get one (`target.max_actor_cast`). | The night is an intended, bounded change of exposure; everything else is judged against it. | `rule_moonlight_cast_only_where_the_mood_allows`, `rule_actor_targets_leave_lighting_to_the_renderer` |
| **Accent darks** are bounded (none on engine-tinted textures) (`accents.max_fraction`), never below `accents.min_l`, and come from structural crevices (band-pass measure), not fine noise. Their cool hue (in `accents.hue`) is taken only where the surface is already in or near the cool family; elsewhere an accent deepens the surface's own color and adds no chroma. | Occasional Impressionist contrast, not dark textures; a navy accent on warm bark is a new hue (see below). | `rule_styles_stay_within_the_contract`, `rule_world_colors_bring_no_new_hue` |

### Value and technique

| Rule | Why | Enforced by |
|---|---|---|
| **No "negative": dark and light stay one family.** Split at the source's smoothed median lightness, the hue difference between the light and dark halves' mean colors changes by at most `technique.split_max_hue_change` degrees, and the light-minus-dark lightness keeps ≥ `technique.split_min_separation` of the source's (as the mood dims it, darks no lower than `min_l`). Darks may gain chroma along their own hue. | Warm lights over cool darks, or darks lifted up toward the lights, read as a photo negative (Link's house, the neighbor house's grooves in v5). Value stays the main carrier. | `rule_no_negative_dark_and_light_stay_one_family` |
| **Softer internal contrast.** Fine-scale light/dark variation drops by at least `value_min_effect × value_contrast.fine`, while the texture's mean L and chroma stay. | SS surfaces sit in a narrow lightness range (measured: ~0.4× OoT Reloaded's local L std); detail moves into color and brushwork. | `rule_value_contrast_is_compressed_color_is_kept` |
| **Busy textures calm down.** With adaptive compression on, a high-contrast (bark-like) texture's mid-scale L std drops by at least `adaptive_min_effect`; textures below the style's target spread (the ground) are untouched by adaptivity. | OoT Reloaded's photographic grooves clash with flat cel-shaded characters; keep the big groove shapes, drop the photographic depth. | `rule_adaptive_contrast_targets_high_contrast_textures` |
| **Value grouping (notan) for busy textures only.** Busy, photographic world textures are simplified into 2–4 value masses found on an edge-aware smoothed lightness; values within a mass move toward the mass, the masses keep their separation and the coarse pattern stays. Actors (the cel shader bands them) and pre-rendered backgrounds (finished paintings) are never grouped, and calm, shape-based textures are untouched. | Big readable value shapes like a painter's notan, instead of photographic grit; no double banding on actors. | `rule_grouping_forms_value_masses`, `rule_grouping_only_touches_busy_world_textures` |
| **Small objects and lettering stay.** Thin sticks and small bright shapes on a busy wall keep ≥ `small_object_min_contrast` of their contrast; small lettering on a busy sign keeps ≥ `text_min_contrast`. Signs can also be opted out of grouping and abstraction in the pack map (`no_grouping`, `no_abstraction`). | Abstraction and grouping simplify texture, never content: tools, bowls, signs and symbols must stay readable. | `rule_small_objects_survive`, `rule_text_stays_legible`, `packs::oot_reloaded_signs_are_never_grouped_or_abstracted` |
| **Thin structures survive.** Thin, elongated objects (tool handles, poles, rails, ropes: on the unsmoothed texels, a ridge in color or value across one of five directions, continuing six radii along it and without look-alikes a few widths to the side) keep their value contrast and get part of their source contrast back after the tone mapping (`marks.thin_protect`; world and background, not tint-safe): a 3-texel shaft on a busy wall that differs by its color keeps at least `technique.thin_min_contrast` of its color contrast, a lighter or darker one `technique.thin_min_value_contrast` of its lightness contrast (against the source as the mood dims it), in every style, mood and category. | The pitchfork and rake handles in Link's house dissolved into the wall: the room's tone mapping flattens the whole value range, and the small-object protection only covered compact objects. | `rule_thin_structures_survive` |
| **Specks are cleaned.** Dark or bright dots of a few texels on world and background textures (up to `marks.speck_radius` reference texels, at least `speck_depth` darker or brighter than two rings of neighbors, with at most one ring sample sharing their value) take their surroundings' color before painting: they keep at most `technique.speck_max_contrast` of their contrast. Lines, cracks, handles, rims and lettering are larger or connected and keep theirs. Actors are never despeckled (their dots are eyes, rivets, studs). | Photographic grit (dirt in the Deku Tree's webs) reads as noise once painted; the painting should be cleaner than the photo. | `rule_specks_are_cleaned`, `rule_small_objects_survive`, `rule_text_stays_legible` |
| **No ink lines on tinted ground.** Engine-tinted (gray, vertex-colored) textures get no accent darks; thin cracks on them stay soft painted cracks: their darkest texels (p2 of L) darken by at most `technique.ink_max_darkening` (against the source as the mood dims it). | A gray accent cannot take its cool hue, so on Kokiri Forest's vertex-colored cracked path (v5 through v6a) it became a near-black line the engine's tint darkened further: ink, not paint. | `rule_no_ink_lines_on_tinted_ground` |
| **Backgrounds keep their exposure.** Pre-rendered rooms keep the murk lift (crushed darks become readable colored shadow), then a smooth monotone tone curve (slope ≥ `exposure::MIN_SLOPE`, black and white fixed) restores the source's mean lightness within `identity.background_max_mean_l`; the coarse light/dark range keeps ≥ `identity.background_min_pattern_range`. Per category: `exposure.preserve_mean` in the target. | The designers painted the room's lighting and mood; a lifted room felt too light in-game. | `rule_backgrounds_keep_their_exposure`, `rule_exposure_curve_is_monotone_and_restores_the_mean` |
| **Backgrounds get the gentlest treatment.** Pre-rendered rooms (finished paintings) get no value grouping, accents or warm/cool split (`target.max_background_*`: one color family), and the palette's hue pulls, earth warmth, chroma floor and dark chroma are held back to at most `target.max_background_palette` (target `hue`, `warmth`, `chroma_floor`, `dark_chroma`); light brushwork, and local contrast recedes a little while the light and dark masses stay. | Link's house in v5 read as a photo negative and turned orange: the designers painted the light, and the style should only soften it. The painted tools and bowls must stay. | `rules_config::rule_backgrounds_get_the_gentlest_treatment`, `rule_no_negative_dark_and_light_stay_one_family`, `rule_small_objects_survive` |
| **No blur.** A hard step edge stays within `max_edge_width` texels while texel-level grit in flat regions decreases. | Edge-preserving, not blurring — "dreamlike" never means blurry. | `rule_no_blur_edges_stay_crisp_noise_becomes_flat` |
| **Low-res textures are painted at a resolution floor.** Below the target's floor (`[resolution]`, per category `[categories.<name>.resolution]`) a texture is enlarged by an integer factor (Lanczos-3, alpha-aware, no ringing, wrapping where it tiles), painted at `internal` times the output size and area-averaged down: a long side of at least min(`resolution.min_floor`, `resolution.max_factor` × its own), no texel grid (steps across source-texel borders ≤ `max_blockiness` × the steps inside), and the same painting as at the source size (16×16 cells within `max_coarse_delta_e`; marks keep their size relative to the source, value spread, speckle and value masses are measured on the source), tiling textures still tile. The `.o2r` adapter scales the OTEX header (size, HD scale factors at 0x50/0x54, data size). | The Deku Tree's 128×256 ring walls and 256×256 floors showed blocky pixel steps in a 4K game; SoH accepts larger replacements when the HD scale factors grow with them (verified in-game). | `rule_low_res_textures_are_painted_at_the_floor`, `adapters::o2r::tests::an_enlarged_texture_gets_a_consistent_header`, `o2r::mod_contains_only_selected_restyled_textures_with_consistent_headers` |
| **No new clipping.** An output channel stays inside 8-bit 1..254 wherever the source channel was (in `finish` and again after the exposure curve), and brushwork scales with the headroom toward black or white. Source-blown whites: small compact ones (glints, sparkles, snow, stars: fewer than half of a ring of neighbors blown) keep their clean white unless they are speck-sized grit (isolated at the speck radius: cleaned like dark specks, no glitter) or sit inside an already bright surface (source ring mean L ≥ 0.85: they only darken with it); large blown regions may only darken. | Clipped highlights and crushed blacks read as damage and lose all color under the cel shader; the painted look lives in the middle. | `rule_no_new_clipping`, `rule_no_glitter_on_bright_surfaces` |
| **No glitter on gritty surfaces.** A blown-white texel with at most two blown texels on the rings at the speck radius and 1.5× it, on a textured surface, is painted with its surroundings' painted color: at most `technique.glitter_max_contrast` lighter than the surface around it. Larger highlights and glints on smooth surfaces keep their white. | The light treehouse bark kept pure-white flecks from blown highlights in its grit: glitter. | `rule_no_glitter_on_light_textured_surfaces` |
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
| **No baked temperature, shadow tint or cool accent darks on actors** (`target.max_actor_accent`). | The cel shader decides lit and shadow at runtime. | `rule_actor_has_no_temperature_shift` |
| **Actor colors bring no new hue.** A colored actor texel stays within `actor.max_local_hue_change` degrees of its 9×9 source neighborhood's mean hue, where that neighborhood is clearly colored. | Blue flecks at a gold chest stud's highlight (cool accents) read as damage, not paint. (Near-neutral steel is not covered yet: see the open steel-vs-mud conflict.) | `rule_actor_colors_bring_no_new_hue` |
| **Steel stays steel.** Near-neutral, faintly cool actor darks (source hue in `actor.cool_dark_hue`) keep their hue family: a colored output stays in `actor.cool_dark_family` (blue-gray to violet) or within `actor.max_cool_dark_hue_shift` of its source: no brown, no teal. The olive neutral tint fades out for cool casts (`palette::cool_cast`). | The chest's steel bands came out copper-brown (the umber dark floor), with teal flecks where the LUT interpolated between umber and blue: iron turned into bronze. | `rules_config::rule_cool_actor_darks_keep_their_hue_family` |
| **Tint-safe textures stay gray:** grayscale-origin textures (engine-tinted, e.g. Link's tunic, hearts) get lightness changes only. | The engine multiplies them by a tint; any baked hue would corrupt every tint. | `rule_tint_safe_grayscale_stays_gray` |
| **Tint-safe actors are raised, painted in brightness only.** With a target `tint_safe_gray`, an engine-tinted grayscale actor texture is shifted so its median gray sits there (at most `actor.max_tint_safe_gray`), gets soft, broad brightness-only strokes along its own folds (amplitude `tint_safe_strokes` ≤ `actor.max_tint_safe_strokes`) and a few highlights, never above `actor.max_tint_safe_l`; its folds stay (16×16 cell correlation ≥ `actor.tint_safe_min_structure`). It may exceed the actor ceiling. | The engine multiplies these by a tint (green, red, blue tunics): a raised gray gives the tint headroom so the tunic reads light and pastel rather than dark and muddy. | `rule_tint_safe_actors_are_raised_and_keep_their_folds`, `rules_config::rule_actor_targets_leave_lighting_to_the_renderer` |
| **Dark tint-safe actors keep their value.** The raise lifts the median to at most `tint_safe_max_gain` (≤ `actor.max_tint_safe_gain`) times its own lightness, and lettering on a tinted plate keeps ≥ `technique.text_min_contrast` of its contrast. Light cloth (a median of about 0.58 or more) still reaches `tint_safe_gray`. | The survey of the whole pack: a black Poe silhouette, a black fishing-rod segment and dark horse legs came out near white (L 0.0–0.15 to 0.87), and black lettering on the prescription's page was clipped away at the cap. | `rule_dark_tint_safe_actors_keep_their_value`, `rules_config::rule_actor_targets_leave_lighting_to_the_renderer` |
| **Actor brushwork makes no large patches.** On a 16×16 grid, the cell-mean lightness change less the texture's mean change stays within `actor.max_patch_l` (p95). The pack map may scale a file's brushwork on top of its category (`[[brushwork]]`, at most `actor.max_brushwork`: faces and skin lighter); the rule is checked at that maximum too. | The cel shader bands actors into lit and shadow at runtime; painted light and dark patches read as extra shading (camouflage). | `rule_actor_brushwork_makes_no_large_patches`, `rules_config::rule_pack_brushwork_stays_within_the_contract` |
| **Solid actors read painted.** Actor brushwork (target actor `strokes`, tint-safe `tint_safe_strokes`) adds at least `actor.min_mark_energy` times the world's fine-scale marks on the same smooth surface; faces and skin get a lighter touch through the pack map (`[[brushwork]]`, at most `actor.max_brushwork` on top of the category). | Gear, props and clothing looked like clean game models on top of the painted world. The zombie lesson still applies to skin. | `rule_solid_actors_read_painted`, `rule_actor_brushwork_makes_no_large_patches` |
| **Effects are left untouched.** Category `effect` (glows, flames, sparks, dust, shadow blobs; named in the pack map) is copied through, and a gray actor or world texture that falls off radially to nothing (`analysis::radial_falloff` ≥ `facts::EFFECT_FALLOFF`) through soft alpha (`analysis::soft_alpha_share` ≥ `facts::EFFECT_SOFT_ALPHA`) is treated the same; hard-edged gray cutouts (a bomb, a statue) are objects and are restyled. | An effect's gray is light intensity and falloff, often drawn additively; brushwork turns a glow into a smudge. | `rule_effects_are_left_untouched`, `analysis::tests::soft_glows_fall_off_radially_patterns_do_not` |

### Fluids

Water, lava and other liquids are their own material family (categories `water`, `lava`,
`liquid`). Painted packs draw them as soft light over depth: thin bright connected lines
(caustics, glowing veins) over a smooth darker body, usually tiling and often scrolling in-game.
The world treatment (value grouping, coarse simplification, big paint marks, wet edges) cut them
into outlined cells that slide around (cracked stone, lizard skin).

- **Which textures are fluids.** World textures are detected by their look (`src/fluid.rs`, on a
  256 px thumbnail: skewed band-pass lightness, line-like and networked ridges spread over the
  sheet, a clean body, tiling; water hue cyan to blue or near-gray; lava saturated red-orange with
  bright veins over a darker crust). Other categories and whatever the detector misses (soft
  flows, foam, waterfalls, translucent sheets) are named by pack-map `[[fluids]]` rules
  (`kind = "water" | "lava" | "liquid" | "none"`), which also override the detector. Other
  liquids are never detected alone (without a hue prior the pattern matches reliefs too).
  Audit on OoT Reloaded, every texture labeled by eye: world precision 1.00, recall 0.81 by file
  (all caustic water and all lava; the misses are soft flows and foam, covered by the pack map).
- **Treatment** (built in, `Treatment::default_for`; a target may override within the contract):
  no value grouping (policy), abstraction, value compression, accents, wet edges or granulation;
  small paint marks (no tiling multiplier), gentle strokes. Engine-tinted gray fluids take the
  tint-safe path (lightness only). **Water** leans toward the style's reference water tone
  (`palette.water`, per-area override in the pack map: `water_hue`, `water_chroma`, `water_pull`,
  `water_lightness` on a `[[fluids]]` rule): hue and chroma in the palette; the **body
  lightness** per image (`lightness`, `lightness_pull`, after the palette and the mood's night
  exposure): the median and everything darker shift toward the reference, the shift fades out
  toward the caustic highlights (98th percentile), which stay where they are. Engine-tinted
  gray water darkens by at most `tint_safe_max_darkening` (the engine tint supplies its value).
  The water sheets are often drawn translucent by the game (OoT: the Kokiri pond at prim alpha
  0.39; the texture's own alpha is 1 and unused by its combiner), so the bed shows through and
  the texture's lightness moves the on-screen water only by that share. The shipped tone is
  dark muted jade, hue 140, chroma 0.035–0.06, pulled halfway, body lightness 0.36 pulled 0.8:
  Skyward Sword's Ancient Cistern water (sun and shade: h 115–140, C 0.036–0.06, L 0.45–0.50)
  and the N64 Deku Tree basement water (h 143, C 0.04, L 0.34–0.36) agree on the hue; the
  lightness follows the N64 (the pond read light over its pale bed). **Lava is emissive**: no
  palette, no moonlight, no mood at all (`Category::is_emissive`): glow and heat colors stay.

| Rule | Why | Enforced by |
|---|---|---|
| Fluid treatments stay within `[fluid]` (no wet edges, granulation, value compression, abstraction, accents; small marks); lava has no palette and no cast; fluids are never grouped. | Caustics are light, not cells. | `fluid_treatments_stay_within_the_contract` |
| **Caustics stay luminous, depth stays smooth, no cell outlines:** on a synthetic caustic texture, highlight-line contrast keeps ≥ `highlight_min_contrast` of the (mood-dimmed) source's, depth grit ≤ `depth_max_grit`, at most `max_outline_share` of depth texels darker than the source by `outline_drop` beyond the depth's median change; the body moves toward the reference lightness by ≥ `body_min_lean` of the pull and never past it (no pull: within `max_mean_l`), the highlight lines drop ≤ `highlight_max_drop`. Rendered as World, the same texture fails. | "Why is our water so ugly? The N64 water looks better." | `rule_caustic_water_stays_luminous_and_smooth` |
| Engine-tinted gray water stays gray and darkens by ≤ `tint_safe_max_darkening` beyond the mood. | The engine supplies the color and multiplies the value. | `rule_engine_tinted_gray_water_stays_gray` |
| Water leans toward the reference tone (when the style pulls). | SS and the N64 agree on dark muted jade. | `rule_water_leans_toward_the_reference_tone` |
| **Lava keeps its glow and heat colors** in every style and mood (nocturne included): vein mean L drops ≤ `lava_max_darkening`, vein chroma kept ≥ `lava_min_chroma_retention`, vein hue moves ≤ `lava_max_hue_shift`. | Glowing things stay glowing. | `rule_lava_keeps_its_glow_and_heat_colors` |

### Robustness

| Rule | Enforced by |
|---|---|
| Deterministic: the same input and config give identical output. | `rule_deterministic` |
| Output is in the sRGB gamut, with no NaNs; 16-bit inputs are handled. | `rule_output_in_gamut_and_finite`, `rule_palette_output_is_in_gamut_and_finite`, `rule_sixteen_bit_inputs_are_handled` |
| Identity when every strength is 0; a neutral config builds no GPU stage. | `rule_palette_is_identity_at_zero_strength`, `rule_identity_when_all_strengths_are_zero`, `rule_neutral_config_is_identity_without_a_gpu` |
| Configs are strict: unknown keys rejected, contradictory ranges fail with a clear error, every mood validated on load. | `rule_unknown_keys_are_rejected`, `rule_invalid_ranges_are_rejected`, `config::tests` |
| LUT `.cube` round-trip and OKLCH conversions are accurate. | `cube_round_trip`, `oklch_round_trip_is_accurate`, unit tests in `src/color.rs` and `src/lut.rs` |
