# PastelPlash architecture

How the code is laid out, how a texture flows through it, and the rules that keep it
deterministic and game-agnostic. What the look *is* lives in [docs/RULES.md](docs/RULES.md);
intent and phases in [PLAN.md](PLAN.md).

## Module map

```
src/
  main.rs                 CLI (clap)
  driver.rs               per-file flow shared by every front end: classify → material → mood →
                          resolution floor (enlarge, run, average down) → run
  resample.rs             Lanczos-3 integer upsample, area downsample (alpha-aware)
  preflight.rs            output size estimate and free-disk-space check before a run
  fluid.rs                fluid detection (water, lava) from the pixels, on a thumbnail
  process.rs              front end: PNG folder in, PNG folder out
  adapters/o2r.rs         front end: libultraship .o2r archives (the only place OTEX exists)
  pipeline.rs             Stage trait, Pipeline, FileContext
  config/
    mod.rs                Config::load (style, target, pack map), validation helpers
    layers.rs             merge(), StyleStack, `extends` resolution
    builtin.rs            the shipped styles, compiled in
    category.rs           Category + policy (is_stylized, may_tile, may_group, is_fluid, is_emissive)
    target.rs             Target, Treatment (per category), Exposure
    pack.rs               Pack map: path globs → category, mood, marks, opt-outs (pure data)
    style/                Style and one module per section (struct + Default + validate())
  stylize/
    mod.rs                the Stylize stage: plan → run → exposure → write back
    facts.rs              ImageFacts: size, scale, tiling, tint safety, low-res field, spreads
    plan/                 Planner + one module per stage: plan() → parameters, reach
    params.rs             the Params uniform block (checked against the WGSL struct)
    runner.rs             GPU device, passes, chunking, concurrency slots
    cache.rs              mood styles and palette LUTs, keyed by content fingerprints
  shaders/common/*.wgsl   bindings, addressing, color, noise, low-res field
  shaders/stages/*.wgsl   one file per pass or finish stage, concatenated in a fixed order
  analysis.rs grouping.rs palette.rs exposure.rs lut.rs color.rs   CPU math
  image.rs png_io.rs walk.rs                                        I/O
  report.rs compare.rs audit.rs                                     dev tools
```

## Pass graph

T0..T4 are `rgba32float` textures of the image (or chunk). One stage, so a texture crosses the
bus once each way.

```text
T0 upload ─delight→ T1 [─group→ T3 → copy to T1] ─tensor→ T2 ─blur_h→ T3 ─blur_v→ T2
(T1, T2) [─kuwahara coarse→ T4] ─kuwahara→ T3 ─bleed→ T4 [─accent_hist, accent_threshold]
─finish (T4, T1, T2; T0 = the untouched upload)→ T3 → readback
```

`finish` runs, per texel: smear, value compression, glare calming, palette LUT, a mood's
moonlight cast (per texel: it switches on the source's chroma, which a LUT can't interpolate),
temperature,
accents, strokes, wet edges, granulation, paper, adaptive contrast, chroma retention, floor and
ceiling (`shaders/stages/finish.wgsl`, one function each). Images above the device limit (or
`PASTELPLASH_MAX_CHUNK`) run in overlapping chunks; the overlap is the plan's `halo` (filter
reach), and per-image quantities and noise use full-image coordinates.

## Adding a stage

1. A style section: `src/config/style/<stage>.rs` (struct, `Default`, `validate()`), added to
   `Style` and `Style::validate`. Defaults are neutral (off).
2. A plan: `src/stylize/plan/<stage>.rs` with `plan(...)` deriving texel-space parameters from
   the style, the category treatment and the `ImageFacts`, `write()` into `Params`, and its
   reach if it samples neighbors (add it to the `halo` sum in `plan/mod.rs`).
3. Params fields in `src/stylize/params.rs` **and** the WGSL `Params` struct
   (`shaders/common/bindings.wgsl`); `params_layout_matches_the_shader` checks they agree.
4. The shader code in `shaders/stages/<stage>.wgsl`, added to `SHADER` in `stylize/mod.rs` in
   its place in the order (a new pass also needs a pipeline in `runner.rs`).
5. A rule in `rules.toml` and a test in `tests/rules/`, and an intended change of look (the
   golden hashes change: see docs/RULES.md).

## Layers, moods and categories

- **Layers.** A style is a stack of TOML layers merged at full strength with
  `layers::merge`: a palette base, overlays (e.g. `styles/overlays/impressionist-brushwork.toml`),
  then the file's own keys. `extends = [...]` names the layers; a style that is only `extends` +
  `name` is an alias for that stack. `StyleStack` is the programmatic form; `--style a+b` (`Style::stacked`) is a synthetic stack:
  a style, then layers by name (an overlay in `styles/overlays/`, a built-in style or a file).
- **Merge semantics.** Numbers (and equal-length number arrays such as tone curves) interpolate
  by strength; other values switch at 0.5; keys only in the overlay appear from 0.5. Arrays of
  tables merge **by `name`**: a layer may declare only the palette groups it changes; unnamed
  entries merge by position only when the counts match; anything else is an error.
- **Moods** are ordinary tables in the merge (`[moods.<name>]`, the union of every layer's
  declarations), blended over the resolved style at the mood's strength
  (`Style::for_mood`). A mood inherits every key it does not name.
- **Categories** are the target's treatments (`[categories.<name>]` multipliers), applied in each
  stage's `plan()`. Category policy (what is stylized, may tile, may be grouped) lives only in
  `config/category.rs`.

## Adapter boundary

The core (config, stylize, pipeline, process) never sees a game name or container format.
Game knowledge lives in two places only: adapters (`src/adapters/`, e.g. `.o2r`/OTEX decoding,
the engine-tinted grayscale formats) and pack-map data (`packs/*.toml`: globs → categories,
moods, marks, opt-outs). Every front end hands files to `driver::Driver`, which classifies them
through the pack map (or a CLI override), picks the mood and runs the pipeline.

Moods for other games follow the same layering: the pack map assigns them by path; packs with
hash-named files will need an explicit list (the pack map's `list` CSV, not wired up yet) or
folder-statistics suggestions; anything game-specific (a format's hints) comes from an
adapter, never from the core.

## Determinism

- Same input and config give byte-identical output, on the same adapter, driver and toolchain.
- No parallel float reductions: parallel work collects per-item values in order and sums them
  sequentially (`analysis::seam_ratio`); `-j1` and `-jN` give identical files (golden test).
- The toolchain (`rust-toolchain.toml`) and the DX12 shader compiler (`gpu::DX12_COMPILER`,
  FXC) are pinned; scripts build with `--locked`.
- Caches key entries by a fingerprint of what they derive from (resolved style, palette), so a
  stage serving several configs never returns a stale LUT.
- `tests/golden_hash.rs` proves pure refactors byte-identical (see docs/RULES.md).
