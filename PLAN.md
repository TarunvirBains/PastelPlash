# PastelPlash — plan

PastelPlash restyles PNG texture packs into a pastel, watercolor look (first style target: *Skyward Sword*)
without losing the sharpness of high-resolution textures. The first target pack is
[OoT Reloaded](https://github.com/GhostlyDark/OoT-Reloaded) (4K) running in the
[SoH cel-shading fork](https://github.com/roborich/Shipwright) (`9.2.3-celshade0.11`),
but the tool itself is pack-agnostic.

## Principles

- **The core is PNG folder in, PNG folder out.** It knows nothing about OoT, SoH or `.o2r`. The output mirrors
  input relative paths and filenames exactly, so it drops back into whatever loads the pack.
- **Pack-specific handling lives in adapters and config**, never in the core.
- **Edge-preserving, not blurring.** Detail noise becomes brushstrokes; structural edges stay at full resolution.
- **Deterministic and repeatable.** Same input + config ⇒ same output. Rerun when the source pack updates.
- **Don't commit texture data.** Packs are gigabytes and belong to their authors.

## Toolchain

- Rust, developed in WSL (Arch), built as a native Windows `.exe` (`x86_64-pc-windows-gnu`, mingw linker; see
  `.cargo/config.toml`). `cargo run` launches it through WSL interop.
- GPU work via `wgpu` on DirectX 12 (native Windows driver; avoids WSL's immature Vulkan layer). Filters are WGSL
  compute shaders.
- Input/output live on the Windows side (e.g. `Z:\…`) so the `.exe` reads them natively.
- Crates: `wgpu`, `image` (PNG I/O), `palette` (OKLCH), `clap` (CLI), `serde` + `toml` (config), `rayon` (CPU-side
  I/O parallelism).

## CLI

```
pastelplash <input> <output> [options]

  -r, --recursive     walk subfolders (default: only PNGs directly in <input>); output mirrors the tree
  --copy-other        copy non-PNG files through, so the output is a complete drop-in pack
  --follow-links      follow symlinks when walking (off by default to avoid loops)
  --style <file>      style config (e.g. styles/skyward-watercolor.toml)
  --target <file>     target renderer profile (e.g. targets/soh-celshade.toml)
  --pack <file>       pack map (e.g. packs/oot-reloaded.toml)
```

Walking rules: `.png` matched case-insensitively; an output folder nested inside the input is skipped.

## Configuration layers

| Layer | Answers | Example |
|---|---|---|
| **Style** | What should it look like? | `skyward-watercolor.toml` — Kuwahara strength, palette LUT, paper grain, edge darkening |
| **Target** | How will the game render it? | `soh-celshade.toml` — relit actor textures get full de-light and a lightness ceiling |
| **Pack map** | Which file is what? | `oot-reloaded.toml` — path rules → `actor` / `world` / `skybox` / `ui` / `skip` |

### Classification fallback chain

1. Pack-map rules (path / filename globs) when names are meaningful.
2. Explicit CSV list (`filename,category`) for hand-sorted packs.
3. Image heuristics for hash-named packs (Dolphin, GLideN64): alpha ⇒ foliage/cutout; seamless edges ⇒ tiling world
   surface; small, sharp, high-contrast ⇒ UI/text. Low-confidence results are flagged for review.

Non-color maps (`_n`, `_nrm`, `_normal`, `_spec`, `_rough`, …) are skipped by default; pack map can override.

## PNG handling

All variants: 8/16-bit, RGB, RGBA, grayscale, palette-indexed. Output preserves bit depth and alpha. Tiling is
detected per image, not assumed.

## Pipeline (per texture)

1. Load (alpha-aware); pad with wrap-around when the texture tiles.
2. **De-light** — remove baked shading/AO; strength from target + category.
3. **Anisotropic Kuwahara** (GPU) — structure-tensor-guided, edge-preserving painterly smoothing.
4. **Palette LUT** — 3D `.cube` built in OKLCH (see below).
5. **Watercolor finish** — edge darkening, pigment granulation, paper grain (texel-space, tileable).
6. **Lightness ceiling** for relit categories (prevents clipping under the cel shader).
7. Restore alpha, encode.

## Palette (Skyward style)

Per-hue OKLCH remapping, not a global brighten/desaturate:
greens → yellow-green/lime/mint, lighter, capped chroma; browns → warm tan/peach; grays → slight warm or lavender
tint; blues → cyan/turquoise; darks lifted to a floor and cool-tinted. Derived from sampled *Skyward Sword*
screenshots vs. the source pack's hue/lightness histograms, baked to a 3D LUT.

## Target notes: SoH cel-shade fork (`9.2.3-celshade0.11`)

Verified from source (`libultraship` `src/fast/shaders/*/default.shader.*`, `soh/soh/Enhancements/Graphics/ToonLighting.cpp`):

- Toon relight applies **only to actors** (and only lit geometry). Static world geometry keeps vanilla vertex-colour
  shading. Excluded actors (doors, Great Deku Tree, water boxes, `En_Wood02`, all `Bg_Spot*`) also render vanilla —
  treat them as `world`.
- Two tones via smoothstep on half-Lambert N·L; texture is only **multiplied** (`texel.rgb *= mix(shadow, lit, ramp)`),
  never posterized. Defaults: `RampCenter` 0.5, `RampSoftness` 0.02, `HighlightIntensity` 0.6, `ShadowIntensity` 0.6.
  Lit ≈ `ambient + 0.6·key`, shadow ≈ `ambient + 0.24·key`.
- Lit factor can exceed 1 in bright scenes ⇒ pale pastel actor textures can clip ⇒ lightness ceiling for `actor`.
- No outlines, rim light, specular, grading or post-process. `HighlightBands` is a debug view — keep it off.
- `ShadowIntensity` is a tuning lever for SS-style light shadows; calibrate alongside the textures.

## Phases

0. **Setup** ✅ — repo, Windows cross-build, `wgpu` DX12 compute smoke test on the RTX 5090.
1. **Generic core** — CLI, folder walker (`-r`, `--copy-other`, `--follow-links`), PNG I/O for all variants,
   config loading, output mirroring.
2. **Palette** — sample SS screenshots, analyze source pack, design OKLCH mapping, bake LUT, swatch comparison page.
3. **Filter prototype** — GPU pipeline on ~8 representative textures (grass, stone, wood, dirt, a house, Link's
   tunic, water, alpha foliage); before/after slider page for tuning.
4. **Classification** — three config layers + fallback chain; `oot-reloaded.toml` pack map.
5. **In-game calibration** — small test pack (Kokiri Forest + Link) in the cel-shade build; day/night/interior/
   dungeon; tune textures with `ShadowIntensity` / `HighlightIntensity`.
6. **Full build + adapters** — `.o2r` extract/pack adapter via `retro`; content-hash cache so reruns only redo
   what changed.
7. **Release** — check OoT Reloaded's license; README with recommended cel-shade settings; publish `.o2r`.

## Open items

- *Skyward Sword* reference screenshots (5–10: Faron Woods, Skyloft, a dungeon, an interior).
- OoT Reloaded license terms for redistributing a derived pack.
- How OoT Reloaded stores textures inside its `.o2r` and whether paths separate actors from world.
