#!/usr/bin/env bash
# Renders review material straight from a .o2r pack (read-only) through the same path the mods
# use: for each style, restyle the selected entries into a temporary .o2r, export source and
# result as PNGs, and write the compare layout (downsized before/after + 1:1 crops) and a
# palette report against the SS reference.
#
#   OUT=/path/to/review scripts/review.sh 'alt/scenes/**/spot04_scene/*Tex_014B08' ...
#
# Settings (environment variables): PACK (default: the SoH OoT Reloaded pack), OUT (required),
# STYLES (default: the default style and the other presets), TARGET, PACK_MAP.
set -euo pipefail

REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
EXE="$REPO/target/x86_64-pc-windows-gnu/release/pastelplash.exe"
winpath() {
    if command -v wslpath >/dev/null 2>&1; then wslpath -w "$1"
    elif command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"
    else printf '%s\n' "$1"; fi
}

: "${OUT:?set OUT to an output folder}"
for d in "/mnt/z/Games/Ocarina of Time/SoH-9.2.3-celshade0.11-Win64" \
         "/z/Games/Ocarina of Time/SoH-9.2.3-celshade0.11-Win64"; do
    [[ -z "${PACK:-}" && -d "$d" ]] && PACK="$d/mods/OoT_Reloaded_v11.0.0_4K.o2r"
done
[[ -f "${PACK:-}" ]] || { echo "set PACK to a .o2r pack" >&2; exit 1; }
STYLES=${STYLES:-"impressionist watercolor ss-baseline ss-impressionist"}
TARGET=${TARGET:-"$REPO/targets/soh-celshade.toml"}
PACK_MAP=${PACK_MAP:-"$REPO/packs/oot-reloaded.toml"}
(( $# > 0 )) || { echo "usage: OUT=dir $0 GLOB..." >&2; exit 1; }
include=()
for g in "$@"; do include+=(--include "$g"); done

command -v cargo >/dev/null 2>&1 && (cd "$REPO" && cargo build --release --quiet)
mkdir -p "$OUT"
rm -rf "$OUT/src" "$OUT/out" "$OUT/compare" "$OUT/tmp"
mkdir -p "$OUT/tmp"
"$EXE" o2r-export "$(winpath "$PACK")" "$(winpath "$OUT/src")" "${include[@]}"

for style in $STYLES; do
    echo "=== $style"
    start=$(date +%s.%N)
    "$EXE" o2r "$(winpath "$PACK")" "$(winpath "$OUT/tmp/$style.o2r")" \
        --style "$(winpath "$REPO/styles/$style.toml")" --target "$(winpath "$TARGET")" \
        --pack "$(winpath "$PACK_MAP")" "${include[@]}" \
        | if [[ -n "${VERBOSE:-}" ]]; then cat; else grep -vE '^  |entries selected'; fi
    "$EXE" o2r-export "$(winpath "$OUT/tmp/$style.o2r")" "$(winpath "$OUT/out/$style")" >/dev/null
    "$EXE" dev-compare "$(winpath "$OUT/src")" "$(winpath "$OUT/out/$style")" \
        "$(winpath "$OUT/compare/$style")" >/dev/null
    awk -v a="$start" -v b="$(date +%s.%N)" 'BEGIN { printf "%s: %.1f s\n", "'"$style"'", b - a }'
done
rm -rf "$OUT/tmp"
