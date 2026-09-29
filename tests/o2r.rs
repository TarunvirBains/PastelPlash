//! End-to-end `.o2r` conversion on a small synthetic pack: entry selection, classification,
//! header preservation, and identity under a neutral config.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use pastelplash::config::{Category, Config};
use pastelplash::o2r::{self, Options};
use pastelplash::pipeline::Pipeline;

fn otex(format: u32, w: u32, h: u32, seed: u32) -> Vec<u8> {
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
    for i in 0..w * h {
        let v = (i.wrapping_mul(2_654_435_761) ^ seed) >> 8;
        b.extend([
            (v % 200) as u8 + 20,
            (v / 7 % 200) as u8 + 20,
            (v / 13 % 200) as u8,
            255,
        ]);
    }
    b
}

fn write_pack(path: &Path, entries: &[(&str, Vec<u8>)]) {
    let mut zip = zip::ZipWriter::new(File::create(path).unwrap());
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        zip.start_file(*name, opts).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

fn read_pack(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
    let mut out: Vec<(String, Vec<u8>)> = (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            (f.name().to_string(), data)
        })
        .collect();
    out.sort();
    out
}

fn sample() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("alt/scenes/shared/spot04_scene/grass", otex(1, 64, 64, 1)),
        ("alt/objects/object_link_boy/gTunicTex", otex(6, 32, 64, 2)),
        (
            "alt/objects/object_link_boy/gLinkEyesOpenTex",
            otex(1, 32, 32, 3),
        ),
        (
            "alt/textures/parameter_static/gHeartTex",
            otex(1, 32, 32, 4),
        ),
        ("alt/scenes/shared/other_scene/stone", otex(1, 32, 32, 5)),
        ("alt/misc/not_a_texture", b"hello".to_vec()),
    ]
}

fn run(dir: &Path, config: &Config, include: &[&str], complete: bool) -> Vec<(String, Vec<u8>)> {
    let input = dir.join("in.o2r");
    write_pack(&input, &sample());
    let output = dir.join("out.o2r");
    let opts = Options {
        input,
        output: output.clone(),
        include: include.iter().map(|s| s.to_string()).collect(),
        category: None,
        mood: None,
        complete,
        jobs: Some(2),
    };
    o2r::run(&opts, config, &Pipeline::from_config(config).unwrap()).unwrap();
    read_pack(&output)
}

fn pack_config(style: Option<&Path>, target: bool) -> Config {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let target_file = root.join("targets/soh-celshade.toml");
    Config::load(
        style,
        target.then_some(target_file.as_path()),
        Some(&root.join("packs/oot-reloaded.toml")),
    )
    .unwrap()
}

#[test]
fn neutral_config_complete_pack_is_byte_identical() {
    // No style and no target: nothing to do, so the complete pack must come back unchanged.
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &pack_config(None, false), &[], true);
    let mut want: Vec<(String, Vec<u8>)> = sample()
        .into_iter()
        .map(|(n, d)| (n.to_string(), d))
        .collect();
    want.sort();
    let names = |v: &[(String, Vec<u8>)]| v.iter().map(|e| e.0.clone()).collect::<Vec<_>>();
    assert_eq!(names(&out), names(&want));
    for ((name, got), (_, exp)) in out.iter().zip(&want) {
        let first = got.iter().zip(exp).position(|(a, b)| a != b);
        assert!(
            got == exp,
            "{name}: {} vs {} bytes, first difference at {first:?}",
            got.len(),
            exp.len()
        );
    }
}

#[test]
fn mod_contains_only_selected_restyled_textures_with_original_headers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = pack_config(
        Some(&root.join(format!(
            "styles/{}.toml",
            pastelplash::config::DEFAULT_STYLE
        ))),
        true,
    );
    assert_eq!(
        config
            .pack
            .classify(Path::new("alt/objects/object_link_boy/gLinkEyesOpenTex")),
        Category::Skip
    );
    let dir = tempfile::tempdir().unwrap();
    // Without a GPU the stylize stage can't be created; nothing to check then.
    if pastelplash::stylize::Stylize::new(&config).is_err() {
        eprintln!("skipping: no GPU adapter");
        return;
    }
    let out = run(
        dir.path(),
        &config,
        &["alt/scenes/**", "alt/objects/**"],
        false,
    );
    let names: Vec<&str> = out.iter().map(|(n, _)| n.as_str()).collect();
    // Eyes (skip), UI and non-texture entries are left out so the source pack's versions load.
    assert_eq!(
        names,
        [
            "alt/objects/object_link_boy/gTunicTex",
            "alt/scenes/shared/other_scene/stone",
            "alt/scenes/shared/spot04_scene/grass",
        ]
    );
    let src: Vec<(&str, Vec<u8>)> = sample();
    for (name, data) in &out {
        let orig = &src.iter().find(|(n, _)| n == name).unwrap().1;
        assert_eq!(data.len(), orig.len(), "{name}: size");
        assert_eq!(data[..0x5C], orig[..0x5C], "{name}: header");
        assert_ne!(data[0x5C..], orig[0x5C..], "{name}: pixels unchanged");
    }
}
