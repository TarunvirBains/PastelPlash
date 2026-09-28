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
- Crates: `wgpu`, `png` (PNG I/O), `palette` (OKLCH), `clap` (CLI), `serde` + `toml` (config), `rayon` (CPU-side
  I/O parallelism).

## CLI

```
pastelplash process <input> <output> [options]

  -r, --recursive     walk subfolders (default: only PNGs directly in <input>); output mirrors the tree
  --copy-other        copy non-PNG files through, so the output is a complete drop-in pack
  --follow-links      follow symlinks when walking (off by default; loops are detected and skipped)
  --style <file>      style config (e.g. styles/skyward-watercolor.toml)
  --target <file>     target renderer profile (e.g. targets/soh-celshade.toml)
  --pack <file>       pack map (e.g. packs/oot-reloaded.toml)
  -j, --jobs <n>      worker threads (default: all cores)

pastelplash gpu-info  print the DX12 adapter and run a compute self-test
```

Walking rules: `.png` matched case-insensitively; an output folder nested inside the input is skipped; files are
processed in sorted order. Per-file errors are reported and processing continues; the exit code is nonzero if any
file failed. Non-color maps are copied through unchanged rather than dropped.

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

Working format (`src/image.rs`): RGBA `f32` in 0..1, straight alpha, gamma-encoded sRGB (not linear), so decode →
encode is exact at 8 and 16 bits and uploads directly as `rgba32float`. Stages convert to linear/OKLCH internally.
Encoding exceptions: palette images are written as RGB8/RGBA8, gray below 8 bits as 8-bit gray, and `tRNS`
transparency becomes an alpha channel. Ancillary chunks (`gAMA`, `sRGB`, `iCCP`, text) are not carried over.

## Pipeline (per texture)

1. Load (alpha-aware); pad with wrap-around when the texture tiles.
2. **De-light** — remove baked shading/AO; strength from target + category.
3. **Anisotropic Kuwahara** (GPU) — structure-tensor-guided, edge-preserving painterly smoothing.
4. **Palette LUT** — 3D `.cube` built in OKLCH (see below). **Tint-safe:** textures the engine colours at runtime
   (stored grayscale, e.g. Link's tunic, HUD hearts) get lightness adjustments only — never an added hue.
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

## Source notes: OoT Reloaded v11 (4K `.o2r`)

Verified by inspecting `OoT_Reloaded_v11.0.0_4K.o2r` (23.4 GB, 11,527 entries):

- Zip archive, every entry **stored uncompressed**, no manifest. All entries under `alt/`.
- Entries are libultraship **OTEX** resources, not PNGs. Little-endian header: `0x04` type `"XETO"`, `0x08` version 1,
  `0x40` original N64 format (1 = RGBA32, 2 = RGBA16, 5–9 = grayscale / grayscale-alpha variants), `0x44` width,
  `0x48` height, `0x4C` flags (= 1, load-as-raw), `0x50`/`0x54` two f32 scale factors, `0x58` data size, then raw
  **RGBA8888** pixels from `0x5C` (size = w × h × 4). ⇒ PastelPlash can read and write `.o2r` natively; `retro`
  (a Flutter GUI with no CLI) is not needed.
- Path taxonomy (maps directly to categories):
  - `alt/objects/object_*/` — actors (`object_link_boy/gLinkAdultTunicTex`)
  - `alt/scenes/{shared,mq,nonmq}/<scene>/` — world (`shared/spot04_scene/…` = Kokiri Forest). `mq` and `nonmq` are
    byte-identical duplicates for 12 dungeons ⇒ content-hash cache processes them once.
  - `alt/textures/vr_*_static/` — skyboxes (up to 8192×2048)
  - `alt/textures/parameter_static/`, `icon_item_*_static/`, `nes_font_static/`, `kanji/`, `font/` — UI/fonts
  - `*Eyes*Tex`, `*Mouth*Tex` inside object folders — eyes/mouths (skip)
  - Toon-excluded actors → `world`: `object_wood02/`, `object_spotNN_*/`, doors (`gameplay_field_keep/gFieldDoor*`,
    `object_bdoor/`, `object_door_gerudo/`, `object_haka_door/`, `object_jya_door/`), Deku Tree (`object_spot04_objects/`)
- Grayscale-origin textures (N64 format 5–9) are tinted in-engine ⇒ flag as tint-safe from the header.
- Sizes: mostly 256–1024 px; ~50% have partial alpha (almost all of `textures/`, about ⅓ of scenes/objects).
- **No license** is stated anywhere (no LICENSE file; many third-party sources credited). Publishing the tool is fine;
  redistributing a restyled pack needs the author's permission. Users can run PastelPlash on their own copy.

## Phases

0. **Setup** ✅ — repo, Windows cross-build, `wgpu` DX12 compute smoke test on the RTX 5090.
1. **Generic core** ✅ — CLI, folder walker (`-r`, `--copy-other`, `--follow-links`), PNG I/O for all variants,
   config loading, output mirroring.
2. **Palette** — sample SS screenshots, analyze source pack, design OKLCH mapping, bake LUT, swatch comparison page.
3. **Filter prototype** — GPU pipeline on ~8 representative textures (grass, stone, wood, dirt, a house, Link's
   tunic, water, alpha foliage); before/after slider page for tuning.
4. **Classification** — three config layers + fallback chain; `oot-reloaded.toml` pack map.
5. **In-game calibration** — small test pack (Kokiri Forest + Link) in the cel-shade build; day/night/interior/
   dungeon; tune textures with `ShadowIntensity` / `HighlightIntensity`.
6. **Full build + adapters** — native `.o2r` adapter (OTEX ↔ PNG, streaming zip read/write); content-hash cache
   so reruns only redo what changed.
7. **Release** — publish the tool; README with recommended cel-shade settings. A restyled pack is released only
   with the source pack author's permission.

## Open items

- *Skyward Sword* reference screenshots (5–10: Faron Woods, Skyloft, a dungeon, an interior).
- Permission from OoT Reloaded's author before distributing any restyled pack.
