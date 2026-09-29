//! Golden hashes: proof that a pure refactor changes nothing, bit for bit.
//!
//! Unlike the perceptual snapshots (`tests/snapshots.rs`), these compare exact hashes (FNV-1a 64)
//! of:
//!
//! - **CPU** (`tests/golden/hashes-cpu.toml`, checked everywhere): every resolved style × mood
//!   (the typed style the pipeline consumes), the target and pack map, every palette LUT
//!   (style × mood × category), and the HLSL that naga generates from the stylize shader.
//! - **GPU** (`tests/golden/hashes-<adapter>.toml`, checked only on the adapter that captured
//!   them): rendered output (f32 bits, PNG8 and PNG16 bytes) for every style × mood ×
//!   category × procedural image, plus chunked processing, 16-bit files through `process::run`,
//!   a fixture pack exercising the pack map's category, mood, marks, `no_grouping` and
//!   `no_abstraction` rules, a neutral config, an external `.cube`, OTEX encoding and an `.o2r`
//!   run. The manifest records the adapter, driver, wgpu, toolchain and shader compiler.
//!
//! A changed case set fails as well as a changed hash. Capture (only for an intended change of
//! look, in a commit tagged `look-change:`; see docs/RULES.md):
//!
//! `WSLENV=PASTELPLASH_GOLDEN_CAPTURE PASTELPLASH_GOLDEN_CAPTURE=1 cargo test --release --test golden_hash`
//!
//! `PASTELPLASH_GOLDEN_DIR=<folder of exported PNGs>` also checks your own textures (default
//! style, target and the OoT Reloaded pack map); their hashes live under `target/golden-user/`
//! and are captured on the first run.

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use common::*;
use pastelplash::config::{Category, Config, Mood};
use pastelplash::image::{Image, SourceColor};
use pastelplash::palette::Mapping;
use pastelplash::pipeline::{FileContext, Pipeline, Stage};
use pastelplash::stylize::Stylize;
use rayon::prelude::*;

type Section = BTreeMap<String, String>;
type Sections = BTreeMap<String, Section>;

// ------------------------------------------------------------------ hashing

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv(bytes))
}

fn f32_bytes(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values
        .into_iter()
        .flat_map(|v| v.to_bits().to_le_bytes())
        .collect()
}

fn png(image: &Image, sixteen: bool) -> Vec<u8> {
    let mut img = image.clone();
    img.source.bit_depth = if sixteen { 16 } else { 8 };
    let mut out = Vec::new();
    pastelplash::png_io::encode(&img, &mut out).unwrap();
    out
}

/// The f32 bits, PNG8 and PNG16 bytes of a rendered image.
fn image_hash(image: &Image) -> String {
    format!(
        "f32:{} png8:{} png16:{}",
        hex(&f32_bytes(image.pixels.iter().flatten().copied())),
        hex(&png(image, false)),
        hex(&png(image, true))
    )
}

// ------------------------------------------------------------------ files

fn capture() -> bool {
    std::env::var("PASTELPLASH_GOLDEN_CAPTURE").is_ok_and(|v| v == "1")
}

fn golden_dir() -> PathBuf {
    repo().join("tests/golden")
}

fn write_file(path: &Path, header: &str, sections: &Sections) {
    let body = toml::to_string(sections).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("{header}{body}")).unwrap();
    let counts: Vec<String> = sections
        .iter()
        .map(|(k, v)| format!("{k}: {}", v.len()))
        .collect();
    println!("captured {} ({})", path.display(), counts.join(", "));
}

fn read_file(path: &Path) -> Option<Sections> {
    let text = fs::read_to_string(path).ok()?;
    Some(toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display())))
}

/// Compares every section except `manifest`: the case sets must match and every hash must be
/// equal. Returns a report of all differences (empty when identical).
fn compare(want: &Sections, got: &Sections) -> String {
    let mut report = String::new();
    let names: std::collections::BTreeSet<&String> = want.keys().chain(got.keys()).collect();
    for name in names.into_iter().filter(|n| *n != "manifest") {
        let empty = Section::new();
        let (w, g) = (
            want.get(name).unwrap_or(&empty),
            got.get(name).unwrap_or(&empty),
        );
        let missing: Vec<&String> = w.keys().filter(|k| !g.contains_key(*k)).collect();
        let added: Vec<&String> = g.keys().filter(|k| !w.contains_key(*k)).collect();
        let changed: Vec<&String> = w
            .iter()
            .filter(|(k, v)| g.get(*k).is_some_and(|x| x != *v))
            .map(|(k, _)| k)
            .collect();
        if !missing.is_empty() {
            report += &format!("[{name}] {} cases missing: {missing:?}\n", missing.len());
        }
        if !added.is_empty() {
            report += &format!("[{name}] {} new cases: {added:?}\n", added.len());
        }
        if !changed.is_empty() {
            report += &format!(
                "[{name}] {} of {} hashes changed:\n",
                changed.len(),
                w.len()
            );
            for k in changed.iter().take(40) {
                report += &format!("  {k}: {} -> {}\n", w[*k], g[*k]);
            }
        }
    }
    report
}

const CAPTURE_HINT: &str = "capture with WSLENV=PASTELPLASH_GOLDEN_CAPTURE \
     PASTELPLASH_GOLDEN_CAPTURE=1 cargo test --release --test golden_hash (only for an intended \
     change of look, in a `look-change:` commit; see docs/RULES.md)";

// ------------------------------------------------------------------ CPU hashes

fn target() -> PathBuf {
    default_target()
}

/// Every style and its mood list (base first).
fn style_mood_list() -> Vec<(String, PathBuf, Config, Vec<Mood>)> {
    styles()
        .into_iter()
        .map(|path| {
            let config = load(&path, &target());
            let mut moods = vec![Mood::default()];
            for name in config.style.moods.keys() {
                for strength in [0.5, 1.0] {
                    moods.push(Mood::new(name, strength));
                }
            }
            (name(&path), path, config, moods)
        })
        .collect()
}

const CATEGORIES: [Category; 7] = [
    Category::World,
    Category::Actor,
    Category::Background,
    Category::Skybox,
    Category::Water,
    Category::Lava,
    Category::Liquid,
];

fn cat_name(c: Category) -> String {
    format!("{c:?}").to_lowercase()
}

fn cpu_sections() -> Sections {
    let mut styles = Section::new();
    let mut luts = Section::new();
    let lists = style_mood_list();
    for (n, _, config, moods) in &lists {
        for mood in moods {
            let mut s = config.style.for_mood(mood).unwrap();
            s.raw = None;
            styles.insert(format!("{n}/{mood}"), hex(format!("{s:?}").as_bytes()));
        }
        let mut denied = config
            .style
            .for_mood(&Mood {
                dark_greens: Some(false),
                ..Mood::default()
            })
            .unwrap();
        denied.raw = None;
        styles.insert(
            format!("{n}/base-dark-greens"),
            hex(format!("{denied:?}").as_bytes()),
        );
    }
    let jobs: Vec<(String, pastelplash::config::Style, Category, Config)> = lists
        .iter()
        .flat_map(|(n, _, config, moods)| {
            moods.iter().flat_map(move |mood| {
                let style = config.style.for_mood(mood).unwrap();
                CATEGORIES.map(|c| {
                    (
                        format!("{n}/{mood}/{}", cat_name(c)),
                        style.clone(),
                        c,
                        config.clone(),
                    )
                })
            })
        })
        .collect();
    let baked: Vec<(String, String)> = jobs
        .par_iter()
        .map(|(key, style, cat, config)| {
            let lut = Mapping::new(&style.palette, &config.target.treatment(*cat)).bake();
            (
                key.clone(),
                hex(&f32_bytes(lut.data.iter().flatten().copied())),
            )
        })
        .collect();
    luts.extend(baked);

    let mut config = Section::new();
    for path in targets() {
        let t = Config::load(None, Some(&path), None).unwrap().target;
        config.insert(
            format!("target/{}", name(&path)),
            hex(format!("{t:?}").as_bytes()),
        );
    }
    for path in toml_dir("packs") {
        let p = Config::load(None, None, Some(&path)).unwrap().pack;
        config.insert(
            format!("pack/{}", name(&path)),
            hex(format!("{p:?}").as_bytes()),
        );
    }

    let mut shader = Section::new();
    shader.insert("hlsl".into(), hex(hlsl().as_bytes()));

    Sections::from([
        ("styles".into(), styles),
        ("luts".into(), luts),
        ("config".into(), config),
        ("shader".into(), shader),
        ("plans".into(), plan_section()),
    ])
}

/// The job digest of every render case, on the CPU: uniform bytes, low-res field, LUT bytes,
/// filter reach and wrap. A CPU-side regression shows here without a GPU.
fn plan_section() -> Section {
    use pastelplash::stylize::Planner;
    let imgs = images();
    let mut cases = Vec::new();
    for (n, _, config, moods) in style_mood_list() {
        let planner = std::sync::Arc::new(Planner::new(&config).unwrap());
        for mood in moods {
            let style = config.style.for_mood(&mood).unwrap();
            for cat in CATEGORIES {
                cases.push((
                    format!("{n}/{mood}/{}", cat_name(cat)),
                    planner.clone(),
                    config.clone(),
                    style.clone(),
                    mood.clone(),
                    cat,
                ));
            }
        }
    }
    cases
        .par_iter()
        .flat_map_iter(|(prefix, planner, config, style, mood, cat)| {
            let mut lut_hash = None;
            imgs.iter()
                .map(|(label, img)| {
                    let ctx = FileContext {
                        rel: Path::new("golden.png"),
                        category: *cat,
                        mood: mood.clone(),
                        config,
                    };
                    let plan = planner.plan(img, &ctx, style).unwrap();
                    let lut = lut_hash
                        .get_or_insert_with(|| {
                            plan.lut.as_ref().map_or("none".to_string(), |spec| {
                                hex(&f32_bytes(spec.bake().data.iter().flatten().copied()))
                            })
                        })
                        .clone();
                    let digest = format!(
                        "params:{} lowres:{} lut:{lut} halo:{} wrap:{}{}",
                        hex(plan.params_bytes()),
                        hex(&f32_bytes(plan.lowres.iter().copied())),
                        plan.halo,
                        u8::from(plan.wrap[0]),
                        u8::from(plan.wrap[1]),
                    );
                    (format!("{prefix}/{label}"), digest)
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn toml_dir(dir: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(repo().join(dir))
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    v.sort();
    v
}

/// The HLSL naga generates for the stylize shader (all entry points, shader model 5.1 as used
/// with FXC). Unchanged HLSL means the DX12 compiler sees the same program.
fn hlsl() -> String {
    let source = pastelplash::stylize::SHADER;
    let module = naga::front::wgsl::parse_str(source).unwrap();
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap();
    let options = naga::back::hlsl::Options::default();
    let pipeline = naga::back::hlsl::PipelineOptions { entry_point: None };
    let mut out = String::new();
    naga::back::hlsl::Writer::new(&mut out, &options, &pipeline)
        .write(&module, &info, None)
        .unwrap();
    out
}

#[test]
fn cpu_hashes_are_unchanged() {
    let path = golden_dir().join("hashes-cpu.toml");
    let got = cpu_sections();
    if capture() {
        write_file(
            &path,
            "# Golden CPU hashes (tests/golden_hash.rs). Generated; do not edit by hand.\n\n",
            &got,
        );
        return;
    }
    let want =
        read_file(&path).unwrap_or_else(|| panic!("{} missing: {CAPTURE_HINT}", path.display()));
    let report = compare(&want, &got);
    assert!(report.is_empty(), "CPU golden hashes differ:\n{report}");
}

// ------------------------------------------------------------------ GPU hashes

/// The procedural images every style × mood × category renders.
fn images() -> Vec<(&'static str, Image)> {
    vec![
        ("bark", bark(192, 1)),
        ("dark_brown_bark", dark_brown_bark(192, 2)),
        ("blocks", gritty_blocks(192, 3)),
        ("foliage", mid_foliage(192, 4)),
        ("dark_foliage", dark_foliage(192, 5)),
        ("sign", sign(256, 6)),
        ("room", room(192, 7)),
        ("cutout", cutout(128, 8)),
        ("pale_skin", pale_skin(128, 9)),
        ("grayscale", grayscale(128, 10)),
        ("tiling320", tiling(320, 11)),
        ("dark_hues", dark_hues(128, 12)),
        ("step_edge", step_edge(128, 13)),
    ]
}

fn apply(stage: &Stylize, config: &Config, category: Category, mood: &Mood, img: &Image) -> Image {
    let mut out = img.clone();
    let ctx = FileContext {
        rel: Path::new("golden.png"),
        category,
        mood: mood.clone(),
        config,
    };
    stage.apply(&mut out, &ctx).unwrap();
    out
}

fn adapter() -> Option<wgpu::AdapterInfo> {
    pollster::block_on(pastelplash::gpu::Gpu::new())
        .ok()
        .map(|g| g.adapter.get_info())
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn lock_version(package: &str) -> String {
    let lock = fs::read_to_string(repo().join("Cargo.lock")).unwrap();
    let needle = format!("name = \"{package}\"\nversion = \"");
    let start = lock.find(&needle).unwrap() + needle.len();
    lock[start..start + lock[start..].find('"').unwrap()].to_string()
}

fn toolchain() -> String {
    let text = fs::read_to_string(repo().join("rust-toolchain.toml")).unwrap();
    let t: toml::Table = toml::from_str(&text).unwrap();
    t["toolchain"]["channel"].as_str().unwrap().to_string()
}

fn manifest(info: &wgpu::AdapterInfo) -> Section {
    Section::from([
        ("adapter".into(), info.name.clone()),
        ("driver".into(), info.driver_info.clone()),
        ("wgpu".into(), lock_version("wgpu")),
        ("rustc".into(), toolchain()),
        (
            "compiler".into(),
            format!("{:?}", pastelplash::gpu::DX12_COMPILER),
        ),
    ])
}

/// Renders every style × mood × category × image.
fn render_section() -> Section {
    let lists = style_mood_list();
    let stages: Vec<Stylize> = lists
        .iter()
        .map(|(_, _, config, _)| Stylize::new(config).unwrap())
        .collect();
    let imgs = images();
    let mut cases = Vec::new();
    for ((n, _, config, moods), stage) in lists.iter().zip(&stages) {
        for mood in moods {
            for cat in CATEGORIES {
                for (label, img) in &imgs {
                    cases.push((
                        format!("{n}/{mood}/{}/{label}", cat_name(cat)),
                        stage,
                        config,
                        cat,
                        mood,
                        img,
                    ));
                }
            }
        }
    }
    cases
        .par_iter()
        .map(|(key, stage, config, cat, mood, img)| {
            (
                key.clone(),
                image_hash(&apply(stage, config, *cat, mood, img)),
            )
        })
        .collect()
}

/// Chunked processing of 320 px textures, per style: a tiling one in 160 px chunks and a busy
/// one (grouping, abstraction: a larger filter reach) in 256 px chunks.
fn chunk_section() -> Section {
    let mut out = Section::new();
    for (n, _, config, _) in style_mood_list() {
        for (label, chunk, img) in [
            ("tiling320", 160, tiling(320, 11)),
            ("bark320", 256, bark(320, 14)),
        ] {
            let stage = Stylize::with_max_chunk(&config, Some(chunk)).unwrap();
            let r = apply(&stage, &config, Category::World, &Mood::default(), &img);
            out.insert(format!("{n}/{label}"), image_hash(&r));
        }
    }
    out
}

fn default_style() -> PathBuf {
    repo().join(format!(
        "styles/{}.toml",
        pastelplash::config::DEFAULT_STYLE
    ))
}

fn write_png(path: &Path, img: &Image) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    pastelplash::png_io::write(img, path).unwrap();
}

/// Hashes every file under `dir` by relative path.
fn hash_tree(dir: &Path, prefix: &str, out: &mut Section) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(format!("{prefix}{rel}"), hex(&fs::read(&p).unwrap()));
            }
        }
    }
}

/// 16-bit and 8-bit gray files through `process::run` with the default style.
fn process16_section() -> Section {
    use pastelplash::process::{self, Options};
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in");
    let mut rgb16 = dark_foliage(64, 1);
    (
        rgb16.source.bit_depth,
        rgb16.source.color,
        rgb16.source.has_alpha,
    ) = (16, SourceColor::Rgb, false);
    write_png(&input.join("rgb16.png"), &rgb16);
    let mut rgba16 = cutout(64, 2);
    rgba16.source.bit_depth = 16;
    write_png(&input.join("rgba16.png"), &rgba16);
    let mut gray8 = grayscale(64, 3);
    (gray8.source.color, gray8.source.has_alpha) = (SourceColor::Gray, false);
    write_png(&input.join("gray8.png"), &gray8);
    let config = load(&default_style(), &target());
    let pipeline = Pipeline::from_config(&config).unwrap();
    let opts = Options {
        input,
        output: dir.path().join("out"),
        category: Some(Category::World),
        ..Default::default()
    };
    let summary = process::run(&opts, &config, &pipeline).unwrap();
    assert_eq!((summary.processed, summary.failed), (3, 0));
    let mut out = Section::new();
    hash_tree(&dir.path().join("out"), "", &mut out);
    out
}

/// A generic pack map exercising categories, moods (with a dark-greens override), marks,
/// `no_grouping`, `no_abstraction`, non-color maps and a pack-wide source scale.
const FIXTURE_PACK: &str = r#"
name = "fixture"
default_category = "world"
source_scale = 8.0
no_grouping = ["**/*Sign*"]
no_abstraction = ["**/*Sign*"]

[[rules]]
glob = "objects/**/*Eyes*"
category = "skip"

[[rules]]
glob = "objects/**"
category = "actor"

[[rules]]
glob = "**/*Background_*"
category = "background"

[[rules]]
glob = "sky/**"
category = "skybox"

[[rules]]
glob = "ui/**"
category = "ui"

[[moods]]
glob = "dark/**/*Moss*"
mood = "nocturne"
dark_greens = false

[[moods]]
glob = "dark/**"
mood = "nocturne"
strength = 0.6

[[marks]]
glob = "ground/**"
scale = 2.0
"#;

fn fixture_input(root: &Path) -> PathBuf {
    let input = root.join("in");
    for (rel, img) in [
        ("scenes/bark.png", bark(192, 21)),
        ("scenes/SignPost.png", sign(256, 22)),
        ("scenes/rock_n.png", gritty_blocks(64, 31)),
        ("dark/wall.png", gritty_blocks(128, 23)),
        ("dark/gMossTex.png", dark_foliage(128, 24)),
        ("ground/grass.png", mid_foliage(128, 25)),
        ("objects/o/skin.png", pale_skin(128, 26)),
        ("objects/o/gEyesTex.png", dark_hues(64, 27)),
        ("rooms/x_Background_1.png", room(192, 28)),
        ("sky/s.png", tiling(128, 29)),
        ("ui/icon.png", cutout(64, 30)),
    ] {
        write_png(&input.join(rel), &img);
    }
    fs::write(input.join("notes.txt"), "not a texture").unwrap();
    input
}

fn fixture_run(input: &Path, output: &Path, jobs: Option<usize>) -> Section {
    use pastelplash::process::{self, Options};
    let dir = input.parent().unwrap();
    let pack = dir.join("fixture.toml");
    fs::write(&pack, FIXTURE_PACK).unwrap();
    let config = Config::load(Some(&default_style()), Some(&target()), Some(&pack)).unwrap();
    let pipeline = Pipeline::from_config(&config).unwrap();
    let opts = Options {
        input: input.to_path_buf(),
        output: output.to_path_buf(),
        recursive: true,
        copy_other: true,
        jobs,
        ..Default::default()
    };
    let summary = process::run(&opts, &config, &pipeline).unwrap();
    assert_eq!(summary.failed, 0);
    let mut out = Section::new();
    hash_tree(output, "", &mut out);
    out
}

/// Neutral configs, an external `.cube`, OTEX encoding and an `.o2r` run.
fn misc_section() -> Section {
    let mut out = Section::new();
    let dir = tempfile::tempdir().unwrap();

    // Neutral: no stages at all; palette at strength 0.
    let config = Config::default();
    let mut img = dark_hues(64, 1);
    let ctx = FileContext {
        rel: Path::new("x.png"),
        category: Category::World,
        mood: Mood::default(),
        config: &config,
    };
    Pipeline::from_config(&config)
        .unwrap()
        .run(&mut img, &ctx)
        .unwrap();
    out.insert("neutral/empty".into(), image_hash(&img));
    let zero = dir.path().join("zero.toml");
    fs::write(&zero, "[palette]\nenabled = true\nstrength = 0.0\n").unwrap();
    let config = load(&zero, &target());
    let stage = Stylize::new(&config).unwrap();
    for cat in [Category::World, Category::Actor] {
        let r = apply(&stage, &config, cat, &Mood::default(), &dark_hues(96, 8));
        out.insert(
            format!("neutral/zero-strength/{}", cat_name(cat)),
            image_hash(&r),
        );
    }

    // External .cube (baked from the watercolor palette) plus a little technique.
    let base = load(&repo().join("styles/watercolor.toml"), &target());
    let lut = Mapping::new(&base.style.palette, &base.target.treatment(Category::World)).bake();
    lut.save(&dir.path().join("p.cube"), "golden").unwrap();
    let cube = dir.path().join("cube.toml");
    fs::write(
        &cube,
        "lut = 'p.cube'\n[kuwahara]\nradius = 6.0\n[watercolor]\nedge_darkening = 0.03\n",
    )
    .unwrap();
    let config = load(&cube, &target());
    let stage = Stylize::new(&config).unwrap();
    for cat in [Category::World, Category::Actor] {
        for (label, img) in [("bark", bark(128, 3)), ("dark_hues", dark_hues(96, 4))] {
            let r = apply(&stage, &config, cat, &Mood::default(), &img);
            out.insert(format!("cube/{}/{label}", cat_name(cat)), image_hash(&r));
        }
    }

    // OTEX: decode, stylize, encode (RGBA and an engine-tinted grayscale format).
    let config = load(&default_style(), &target());
    let stage = Stylize::new(&config).unwrap();
    for (label, format, img) in [
        ("rgba", 1u32, dark_foliage(64, 5)),
        ("gray", 6, grayscale(64, 6)),
    ] {
        let bytes = otex(format, &img);
        let (o, mut decoded) = pastelplash::adapters::o2r::decode(&bytes).unwrap();
        let ctx = FileContext {
            rel: Path::new("t"),
            category: Category::World,
            mood: Mood::default(),
            config: &config,
        };
        stage.apply(&mut decoded, &ctx).unwrap();
        out.insert(
            format!("otex/{label}"),
            hex(&pastelplash::adapters::o2r::encode(&o, &decoded).unwrap()),
        );
    }

    // An .o2r run with the OoT Reloaded pack map.
    let entries: Vec<(&str, Vec<u8>)> = vec![
        (
            "alt/scenes/shared/spot04_scene/bark",
            otex(1, &bark(128, 7)),
        ),
        (
            "alt/scenes/shared/ydan_scene/gMossTex",
            otex(1, &dark_foliage(96, 8)),
        ),
        (
            "alt/scenes/shared/link_home_scene/link_home_room_0Background_1",
            otex(1, &room(128, 9)),
        ),
        (
            "alt/objects/object_link_boy/gTunicTex",
            otex(6, &grayscale(64, 10)),
        ),
        (
            "alt/objects/object_link_boy/gSkinTex",
            otex(1, &pale_skin(64, 11)),
        ),
        (
            "alt/objects/object_link_boy/gLinkEyesOpenTex",
            otex(1, &dark_hues(32, 12)),
        ),
        (
            "alt/textures/vr_fine0_static/gSky",
            otex(1, &tiling(128, 13)),
        ),
        ("alt/misc/not_a_texture", b"hello".to_vec()),
    ];
    let input = dir.path().join("in.o2r");
    let mut zip = zip::ZipWriter::new(fs::File::create(&input).unwrap());
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in &entries {
        zip.start_file(*name, stored).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
    let config = Config::load(
        Some(&default_style()),
        Some(&target()),
        Some(&repo().join("packs/oot-reloaded.toml")),
    )
    .unwrap();
    let output = dir.path().join("out.o2r");
    let opts = pastelplash::adapters::o2r::Options {
        input,
        output: output.clone(),
        include: Vec::new(),
        category: None,
        mood: None,
        complete: true,
        jobs: None,
    };
    pastelplash::adapters::o2r::run(&opts, &config, &Pipeline::from_config(&config).unwrap())
        .unwrap();
    let mut archive = zip::ZipArchive::new(fs::File::open(&output).unwrap()).unwrap();
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).unwrap();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut data).unwrap();
        out.insert(format!("o2r/{}", f.name()), hex(&data));
    }
    out
}

fn otex(format: u32, img: &Image) -> Vec<u8> {
    let (w, h) = (img.width, img.height);
    let mut b = vec![0u8; 0x5C];
    b[4..8].copy_from_slice(b"XETO");
    b[8..12].copy_from_slice(&1u32.to_le_bytes());
    b[0x40..0x44].copy_from_slice(&format.to_le_bytes());
    b[0x44..0x48].copy_from_slice(&w.to_le_bytes());
    b[0x48..0x4C].copy_from_slice(&h.to_le_bytes());
    b[0x4C..0x50].copy_from_slice(&1u32.to_le_bytes());
    b[0x50..0x54].copy_from_slice(&64.0f32.to_le_bytes());
    b[0x54..0x58].copy_from_slice(&16.0f32.to_le_bytes());
    b[0x58..0x5C].copy_from_slice(&(w * h * 4).to_le_bytes());
    for p in &img.pixels {
        b.extend(p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
    }
    b
}

#[test]
fn gpu_hashes_are_unchanged() {
    let Some(info) = adapter() else {
        eprintln!("skipping GPU golden hashes: no usable GPU adapter");
        return;
    };
    let path = golden_dir().join(format!("hashes-{}.toml", slug(&info.name)));
    let existing = read_file(&path);
    if !capture() && existing.is_none() {
        let others: Vec<String> = fs::read_dir(golden_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("hashes-") && n != "hashes-cpu.toml")
            .collect();
        eprintln!(
            "skipping GPU golden hashes: none for adapter {:?} (have: {others:?}); {CAPTURE_HINT}",
            info.name
        );
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let input = fixture_input(dir.path());
    let pack = fixture_run(&input, &dir.path().join("out-n"), None);
    let pack_j1 = fixture_run(&input, &dir.path().join("out-1"), Some(1));
    assert_eq!(pack, pack_j1, "-j1 and -jN outputs differ");

    let timed = |label: &str, f: fn() -> Section| {
        let t = std::time::Instant::now();
        let s = f();
        eprintln!("golden: {label}: {} cases in {:.1?}", s.len(), t.elapsed());
        s
    };
    let got = Sections::from([
        ("manifest".into(), manifest(&info)),
        ("renders".into(), timed("renders", render_section)),
        ("chunked".into(), timed("chunked", chunk_section)),
        ("process16".into(), timed("process16", process16_section)),
        ("pack".into(), pack),
        ("misc".into(), timed("misc", misc_section)),
    ]);
    if capture() {
        write_file(
            &path,
            "# Golden GPU hashes for one adapter (tests/golden_hash.rs). Generated; do not edit.\n\n",
            &got,
        );
        return;
    }
    let want = existing.unwrap();
    let report = compare(&want, &got);
    let drift: Vec<String> = want
        .get("manifest")
        .map(|m| {
            m.iter()
                .filter(|(k, v)| got["manifest"].get(*k) != Some(*v))
                .map(|(k, v)| format!("{k}: {v:?} -> {:?}", got["manifest"].get(k)))
                .collect()
        })
        .unwrap_or_default();
    if !drift.is_empty() {
        println!("note: environment differs from the baseline: {drift:?}");
    }
    assert!(
        report.is_empty(),
        "GPU golden hashes differ{}:\n{report}",
        if drift.is_empty() {
            String::new()
        } else {
            format!(" (environment drifted from the baseline: {drift:?})")
        }
    );
}

/// Your own textures (`PASTELPLASH_GOLDEN_DIR`), rendered like the mod: default style, target
/// and the OoT Reloaded pack map. Hashes are kept under `target/golden-user/` (never committed).
#[test]
fn user_textures_are_unchanged() {
    let Ok(src) = std::env::var("PASTELPLASH_GOLDEN_DIR") else {
        return;
    };
    let Some(info) = adapter() else {
        eprintln!("skipping user golden hashes: no usable GPU adapter");
        return;
    };
    use pastelplash::process::{self, Options};
    let work = repo().join("target/golden-user");
    let output = work.join("out");
    let _ = fs::remove_dir_all(&output);
    let config = Config::load(
        Some(&default_style()),
        Some(&target()),
        Some(&repo().join("packs/oot-reloaded.toml")),
    )
    .unwrap();
    let pipeline = Pipeline::from_config(&config).unwrap();
    let opts = Options {
        input: PathBuf::from(src),
        output: output.clone(),
        recursive: true,
        ..Default::default()
    };
    let summary = process::run(&opts, &config, &pipeline).unwrap();
    assert_eq!(summary.failed, 0);
    let mut files = Section::new();
    hash_tree(&output, "", &mut files);
    let got = Sections::from([
        ("manifest".into(), manifest(&info)),
        ("files".into(), files),
    ]);
    let path = work.join(format!("hashes-{}.toml", slug(&info.name)));
    match read_file(&path) {
        Some(want) if !capture() => {
            let report = compare(&want, &got);
            assert!(report.is_empty(), "user golden hashes differ:\n{report}");
        }
        _ => write_file(&path, "# User texture hashes (not committed).\n\n", &got),
    }
}
