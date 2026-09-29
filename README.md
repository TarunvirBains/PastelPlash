# PastelPlash

PastelPlash is a command-line tool that restyles game texture packs (folders of PNGs, and
`.o2r` archives with OTEX textures) into a painterly watercolor and impressionist look. It runs
WGSL compute shaders on the GPU through [wgpu](https://wgpu.rs).

The restyle is a layer over the source art, not a repaint. The textures keep their colors,
values and shapes, so the game still reads as itself. On top of that they get:

- soft painted washes and brushwork that follow the texture's own structure;
- wet edges, paper grain and granulation;
- softer contrast inside a surface (the big light and dark shapes stay);
- colored shadows in place of flat black.

Edges stay crisp (edge-preserving filters, no blur). Alpha is kept exactly and tiling textures
stay seamless. The output is deterministic: the same input and config give byte-identical files
on the same GPU, driver and build.

The style is defined by testable rules (color identity, colored darks, no mud, no new clipping,
and so on). They are listed in [docs/RULES.md](docs/RULES.md).

## Install

### From GitHub Releases

Download the archive for your platform from the
[Releases](https://github.com/TarunvirBains/PastelPlash/releases) page:

- `pastelplash-<version>-x86_64-pc-windows-gnu.zip` (Windows, DirectX 12)
- `pastelplash-<version>-x86_64-unknown-linux-gnu.tar.gz` (Linux, Vulkan)

Check the download against `SHA256SUMS` (`sha256sum -c SHA256SUMS --ignore-missing`) and unpack
it. Each archive holds the `pastelplash` binary, the licenses, this README and the sample config
folders `styles/`, `targets/`, `packs/` and `reference/`. The shipped styles are also built into
the binary, so `--style <name>` works from any directory. Targets and pack maps are always read
from the file path you pass.

### From crates.io

```sh
cargo install pastelplash --locked
```

### From source

```sh
git clone https://github.com/TarunvirBains/PastelPlash
cd PastelPlash
cargo build --release --locked
```

`rust-toolchain.toml` pins the Rust toolchain (rustup installs it on first use).
`.cargo/config.toml` makes the default build target `x86_64-pc-windows-gnu` with the mingw-w64
linker, because development happens in WSL and the tool runs as a native Windows `.exe`. For a
native build on Linux, pass the target:

```sh
cargo build --release --locked --target x86_64-unknown-linux-gnu
# binary: target/x86_64-unknown-linux-gnu/release/pastelplash
```

## Quick start

Check that a GPU adapter works:

```sh
pastelplash gpu-info
```

Restyle a folder of PNGs. The output mirrors the input's relative paths and file names:

```sh
pastelplash process textures/ restyled/ --recursive --copy-other
```

- `--recursive` walks subfolders.
- `--copy-other` copies non-PNG files through, so the output is a complete drop-in pack.
- Normal, specular and roughness maps (`_n`, `_nrm`, `_normal`, `_spec`, `_rough`) are copied
  unchanged.

Without a pack map every PNG is treated as world geometry. To override the category and mood
for the whole run:

```sh
pastelplash process ui-textures/ out/ --category ui
pastelplash process dungeon/ out/ --mood nocturne:0.6
```

### Example: an `.o2r` pack for Ship of Harkinian

Ship of Harkinian loads texture packs as `.o2r` archives. PastelPlash reads the pack (without
modifying it) and writes a new `.o2r` mod that holds only the restyled textures. The repository
ships a target profile for the SoH cel-shading fork and a pack map for OoT Reloaded:

```sh
pastelplash o2r "mods/OoT_Reloaded_v11.0.0_4K.o2r" PastelPlash.o2r \
    --target targets/soh-celshade.toml \
    --pack packs/oot-reloaded.toml
```

Put `PastelPlash.o2r` in SoH's `mods/` folder and enable it in the Mod Menu **after** the source
pack, so its textures take priority. Other options:

- `--complete` writes a standalone pack that also holds every unprocessed entry.
- `--include '<glob>'` (repeatable) restyles only matching entries, e.g.
  `--include 'alt/scenes/*/spot04_scene/**'`.
- `o2r-export` writes a pack's textures out as PNGs, keeping their archive paths:
  `pastelplash o2r-export pack.o2r pngs/`.

## Styles, targets and pack maps

Three layers of TOML configuration decide the result:

| Layer | Flag | Answers |
|---|---|---|
| Style | `--style <FILE\|NAME>` | What should it look like? |
| Target | `--target <FILE>` | How will the renderer draw it (for example, actors relit by a cel shader)? |
| Pack map | `--pack <FILE>` | Which file is what: path globs to categories, moods and opt-outs. |

### Styles

| Name | Look |
|---|---|
| `impressionist` (default) | `watercolor` plus bolder impressionist brushwork. |
| `watercolor` | The source's own color, painted: gentle hue nudges, colored shadows, softer internal contrast, watercolor technique. |
| `ss-baseline` | `watercolor`, nudged further toward a *Skyward Sword* palette. |
| `ss-impressionist` | `ss-baseline` with the impressionist brushwork. |
| `ss-terracotta` | `ss-baseline` with warm rose-sienna earth on grain-free brown ground textures. |
| `ss-terracotta-impressionist` | `ss-terracotta` with the impressionist brushwork. |

`--style` accepts any of these:

- a built-in name, e.g. `--style watercolor`;
- a path to a TOML file, e.g. `--style my-style.toml`;
- a stack `a+b`, e.g. `--style ss-terracotta+impressionist`.

A stack is a style followed by layers merged over it in order. Each layer can be one of:

- an overlay from `styles/overlays/` by name (`impressionist` names the
  `impressionist-brushwork` overlay, `terracotta` the terracotta one);
- another built-in style;
- a file.

A style file can build on other style files with `extends`, with paths relative to the file.
For example, `styles/my-style.toml` next to the shipped styles:

```toml
name = "my-style"
extends = ["watercolor.toml", "overlays/impressionist-brushwork.toml"]

[kuwahara]
radius = 12.0
```

Configs are strict. Unknown keys and out-of-range values fail with an error naming the file.
The keys and their bounds live in `styles/*.toml` and [docs/RULES.md](docs/RULES.md).

**Moods** are named partial overrides inside a style (`[moods.<name>]`, for example `nocturne`,
a dim moonlit cast). The pack map assigns them to files by path, or `--mood NAME[:STRENGTH]`
applies one to every file.

### Targets and pack maps

- **Target** (`targets/*.toml`): a treatment per category, a lightness ceiling for relit
  actors, and an output resolution floor (low-resolution textures are written larger). Without
  `--target` the neutral defaults apply.
- **Pack map** (`packs/*.toml`): `[[rules]]` with path globs that map files to one of the
  categories `actor`, `world`, `skybox`, `background`, `ui`, `effect`, `skip`, `water`,
  `lava` or `liquid`. It also holds mood rules, brushwork scales, fluid overrides and opt-outs
  (`no_grouping`, `no_abstraction`, `cues`). The first matching rule wins. Unmatched files get
  `default_category`, which is `world` unless the map sets it.

`--category` and `--mood` override the pack map for a whole run.

### Other commands

- `bake-lut --style <FILE> out.cube` bakes a style's palette into a `.cube` 3D LUT. It also
  takes `--target`, `--category` and `--mood`.
- `palette-report --reference reference/ss-lit.toml <DIR>` prints OKLCH statistics per hue
  group next to a reference palette. With `--source <DIR>` it also prints each file's
  mean-color shift from its source.

## Environment variables

| Variable | Effect |
|---|---|
| `WGPU_BACKEND` | GPU backend, e.g. `dx12` or `vulkan`. Default: `dx12` on Windows, `vulkan` elsewhere. |
| `WGPU_ADAPTER_NAME` | Use the first adapter whose name contains this text (case-insensitive), e.g. `llvmpipe` or `Microsoft Basic Render Driver`. The run fails if none matches. Default: the high-performance adapter. |
| `PASTELPLASH_MAX_CHUNK` | Process images whose side is above this many texels in overlapping chunks (at least 64; clamped to the device limit). Chunking does not change the output. |

The test suite reads a few more variables (`PASTELPLASH_BLESS`, `PASTELPLASH_GOLDEN_CAPTURE`,
`PASTELPLASH_GOLDEN_DIR`, `PASTELPLASH_PROPTEST_CASES`). They are documented in
[docs/RULES.md](docs/RULES.md).

## Use in build pipelines

PastelPlash runs without a display or window system and never prompts for input.

| Exit code | Meaning |
|---|---|
| `0` | Success: every file was processed or copied. |
| `1` | Failure. Either the run could not start (bad config, missing input, no GPU adapter, not enough disk space), or at least one file failed. A failed file does not stop the others. |
| `2` | Invalid command line (unknown flag, missing argument). |

Output streams:

- **stdout**: an output-size estimate, per-texture lines (timings, and indented analysis
  details) and a final summary line with the processed, copied and failed counts.
- **stderr**: errors (`error: ...`) and warnings (`warning: ...`).

The per-texture lines are diagnostics and their format may change. Scripts should rely on the
exit code.

```sh
#!/usr/bin/env sh
set -eu
pastelplash process assets/textures build/textures \
    --recursive --copy-other \
    --style impressionist --pack config/my-pack.toml --jobs 8 \
    > build/pastelplash.log
echo "textures restyled"
```

A GitHub Actions step on a runner without a GPU can use a software adapter. On Linux that is
lavapipe:

```yaml
- run: sudo apt-get install -y mesa-vulkan-drivers
- run: pastelplash process in/ out/ --recursive
  env:
    WGPU_ADAPTER_NAME: llvmpipe
```

## GPU requirements

- **Windows:** a DirectX 12 adapter. Shaders are compiled with FXC, which ships with Windows.
- **Linux:** a Vulkan driver.
- The device must support compute shaders and `rgba32float` storage textures. Images larger
  than the device's texture limit are processed in chunks.

**Software fallback.** Without a GPU, a software rasterizer works:

- WARP on Windows: `WGPU_ADAPTER_NAME="Microsoft Basic Render Driver"`.
- lavapipe (Mesa's `mesa-vulkan-drivers` or `vulkan-swrast`) on Linux: `WGPU_ADAPTER_NAME=llvmpipe`.

Software adapters are much slower, and their output is not bit-identical to a hardware GPU's.
Run `pastelplash gpu-info` to see which adapter is picked; it also runs a compute self-test.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this work, as defined in the Apache-2.0 license, shall be dual licensed as
above, without any additional terms or conditions.

This repository contains no game or texture-pack images. Test textures are generated
procedurally.

## Contributing

[docs/RULES.md](docs/RULES.md) defines the style. Every change must keep its rules passing.
The document explains how to tell tuning, rule changes, intended changes of look and pure
refactors apart, and how to re-bless snapshots and re-capture golden hashes.
[ARCHITECTURE.md](ARCHITECTURE.md) describes the code layout.

Enable the git hooks once per clone:

```sh
git config core.hooksPath scripts/hooks
```

- `pre-commit` runs `cargo fmt --check`, clippy (`-D warnings`) and the fast CPU-only tests.
- `pre-push` runs the full test suite. The GPU tests skip cleanly when no adapter is available.

CI runs the same checks on software GPUs: lavapipe on Linux (the full suite) and WARP on
Windows (everything but the slow GPU rule matrix).
