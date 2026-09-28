#!/usr/bin/env bash
# Builds PastelPlash mods from an OoT Reloaded .o2r pack. Works from Git Bash on Windows and from
# bash in WSL. Needs only pastelplash.exe, coreutils and (optionally, for VRAM numbers)
# nvidia-smi. The source pack is only read.
#
#   scripts/make-mod.sh                 # test mods: Kokiri Forest + Link, every style
#   FULL=1 scripts/make-mod.sh          # every texture in the pack
#   STYLES=impressionist TAG=v3 scripts/make-mod.sh
#
# Built mods are copied to $SOH_DIR/pastelplash-variants/ as PastelPlash-<style>-<TAG>.o2r;
# nothing is written to the mods folder unless INSTALL=mods (the game may have it open).
#
# Settings (environment variables):
#   SOH_DIR        Ship of Harkinian folder (default: autodetected, see below)
#   PACK           source pack (default: $SOH_DIR/mods/OoT_Reloaded_v11.0.0_4K.o2r)
#   STYLES         styles to build, space-separated (default: every preset)
#   TAG            name suffix (default: test, or full with FULL=1)
#   INSTALL        variants (default), mods (only INSTALL_STYLE, into $SOH_DIR/mods) or none
#   INSTALL_STYLE  style for INSTALL=mods (default: watercolor)
#   INCLUDE        entry globs, space-separated (default: the Kokiri Forest + Link test set)
#   FULL=1         no INCLUDE filter: restyle the whole pack
#   JOBS           worker threads (default: all cores)
#   OUT_DIR        where built mods and logs go (default: <repo>/target/mods)
set -euo pipefail

REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
EXE="$REPO/target/x86_64-pc-windows-gnu/release/pastelplash.exe"

# Windows path for the .exe (it doesn't understand /mnt/z or /z paths).
winpath() {
    if command -v wslpath >/dev/null 2>&1; then
        wslpath -w "$1"
    elif command -v cygpath >/dev/null 2>&1; then
        cygpath -w "$1"
    else
        printf '%s\n' "$1"
    fi
}

now() { date +%s.%N; }
elapsed() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.1f", b - a }'; }

if [[ -z "${SOH_DIR:-}" ]]; then
    for d in "/mnt/z/Games/Ocarina of Time/SoH-9.2.3-celshade0.11-Win64" \
             "/z/Games/Ocarina of Time/SoH-9.2.3-celshade0.11-Win64"; do
        [[ -d "$d" ]] && SOH_DIR=$d && break
    done
fi
[[ -n "${SOH_DIR:-}" && -d "$SOH_DIR" ]] || { echo "set SOH_DIR to your Ship of Harkinian folder" >&2; exit 1; }
PACK=${PACK:-"$SOH_DIR/mods/OoT_Reloaded_v11.0.0_4K.o2r"}
[[ -f "$PACK" ]] || { echo "pack not found: $PACK" >&2; exit 1; }
STYLES=${STYLES:-"watercolor impressionist ss-baseline pastel"}
INSTALL=${INSTALL:-variants}
INSTALL_STYLE=${INSTALL_STYLE:-watercolor}
OUT_DIR=${OUT_DIR:-"$REPO/target/mods"}
mkdir -p "$OUT_DIR"
LOG="$OUT_DIR/make-mod.log"

if [[ "${FULL:-0}" == 1 ]]; then
    INCLUDE=""
    SCOPE=${TAG:-full}
else
    # Kokiri Forest (scene, interiors and their pre-rendered backdrops), its people and props,
    # Link, and the shared keeps (bushes, grass, rocks, signs, doors, pots).
    INCLUDE=${INCLUDE:-"alt/scenes/*/spot04_scene/** alt/scenes/nonmq/ydan_scene/** alt/scenes/*/kokiri_home*_scene/** alt/scenes/*/link_home_scene/** alt/scenes/*/kokiri_shop_scene/** alt/textures/vr_K3VR_static/** alt/textures/vr_K4VR_static/** alt/textures/vr_K5VR_static/** alt/textures/vr_LHVR_static/** alt/textures/vr_KSVR_static/** alt/objects/object_spot04_objects/** alt/objects/object_link_boy/** alt/objects/object_link_child/** alt/objects/gameplay_field_keep/** alt/objects/gameplay_keep/** alt/objects/object_km1/** alt/objects/object_kw1/** alt/objects/object_sa/** alt/objects/object_mm/** alt/objects/object_kanban/** alt/objects/object_gs/** alt/objects/object_tsubo/** alt/objects/object_masterkokiri*/**"}
    SCOPE=${TAG:-test}
fi
include_args=()
for g in $INCLUDE; do include_args+=(--include "$g"); done

# Build when a Rust toolchain is available (WSL); otherwise use the existing binary.
if command -v cargo >/dev/null 2>&1; then
    echo "building release binary..."
    (cd "$REPO" && cargo build --release --quiet)
fi
[[ -f "$EXE" ]] || { echo "missing $EXE (build it in WSL with: cargo build --release)" >&2; exit 1; }

SMI=$(command -v nvidia-smi 2>/dev/null || command -v nvidia-smi.exe 2>/dev/null || true)
[[ -z "$SMI" && -x /usr/lib/wsl/lib/nvidia-smi ]] && SMI=/usr/lib/wsl/lib/nvidia-smi

# Samples used VRAM (MiB) every 0.25 s into $1 until stopped.
vram_start() {
    [[ -n "$SMI" ]] || return 0
    (while :; do "$SMI" --query-gpu=memory.used --format=csv,noheader,nounits 2>/dev/null | head -1; sleep 0.25; done) >"$1" &
    VRAM_PID=$!
}
vram_stop() {
    [[ -n "${VRAM_PID:-}" ]] || return 0
    kill "$VRAM_PID" 2>/dev/null || true
    wait "$VRAM_PID" 2>/dev/null || true
    VRAM_PID=
    sort -n "$1" | awk 'NR == 1 { lo = $1 } { hi = $1 } END { if (NR) printf "VRAM used: baseline %d MiB, peak %d MiB (+%d MiB)\n", lo, hi, hi - lo }'
}

{
    echo "== $(date) scope=$SCOPE pack=$PACK"
    echo "styles: $STYLES"
    [[ -n "$INCLUDE" ]] && echo "include: $INCLUDE"
    [[ -n "$SMI" ]] || echo "(nvidia-smi not found: no VRAM numbers)"
} | tee -a "$LOG"

total_start=$(now)
for style in $STYLES; do
    out="$OUT_DIR/PastelPlash-$style-$SCOPE.o2r"
    echo "--- $style -> $out" | tee -a "$LOG"
    start=$(now)
    vram_start "$OUT_DIR/vram-$style.txt"
    "$EXE" o2r "$(winpath "$PACK")" "$(winpath "$out")" \
        --style "$(winpath "$REPO/styles/$style.toml")" \
        --target "$(winpath "$REPO/targets/soh-celshade.toml")" \
        --pack "$(winpath "$REPO/packs/oot-reloaded.toml")" \
        ${JOBS:+--jobs "$JOBS"} "${include_args[@]}" 2>&1 \
        | tee "$OUT_DIR/$style-$SCOPE.log" | grep -v '^  ' | tee -a "$LOG"
    status=${PIPESTATUS[0]}
    vram_stop "$OUT_DIR/vram-$style.txt" | tee -a "$LOG"
    echo "$style: $(elapsed "$start" "$(now)") s, $(du -h "$out" 2>/dev/null | cut -f1) (per-texture lines: $OUT_DIR/$style-$SCOPE.log)" | tee -a "$LOG"
    [[ $status == 0 ]] || { echo "$style failed" | tee -a "$LOG"; exit "$status"; }
done
echo "all styles: $(elapsed "$total_start" "$(now)") s" | tee -a "$LOG"

case "$INSTALL" in
    variants)
        mkdir -p "$SOH_DIR/pastelplash-variants"
        for style in $STYLES; do
            cp "$OUT_DIR/PastelPlash-$style-$SCOPE.o2r" "$SOH_DIR/pastelplash-variants/"
            echo "copied PastelPlash-$style-$SCOPE.o2r to $SOH_DIR/pastelplash-variants" | tee -a "$LOG"
        done
        echo "To use one: quit SoH, move it into mods/, enable it in the Mod Menu after OoT Reloaded." | tee -a "$LOG"
        ;;
    mods)
        cp "$OUT_DIR/PastelPlash-$INSTALL_STYLE-$SCOPE.o2r" "$SOH_DIR/mods/"
        echo "installed PastelPlash-$INSTALL_STYLE-$SCOPE.o2r into $SOH_DIR/mods" | tee -a "$LOG"
        ;;
    none) ;;
    *) echo "INSTALL must be variants, mods or none" >&2; exit 1 ;;
esac
