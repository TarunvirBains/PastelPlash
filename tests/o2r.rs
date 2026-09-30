//! End-to-end `.o2r` conversion on a small synthetic pack: entry selection, classification,
//! header preservation, and identity under a neutral config.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use pastelplash::adapters::o2r::{self, Options};
use pastelplash::config::{Category, Config};
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

fn run_pack(
    dir: &Path,
    config: &Config,
    entries: &[(&str, Vec<u8>)],
    include: &[&str],
) -> (Vec<(String, Vec<u8>)>, o2r::Summary) {
    let input = dir.join("in.o2r");
    write_pack(&input, entries);
    let output = dir.join("out.o2r");
    let opts = Options {
        input,
        output: output.clone(),
        include: include.iter().map(|s| s.to_string()).collect(),
        category: None,
        mood: None,
        complete: false,
        jobs: Some(2),
    };
    let summary = o2r::run(&opts, config, &Pipeline::from_config(config).unwrap()).unwrap();
    (read_pack(&output), summary)
}

fn entry<'a>(pack: &'a [(String, Vec<u8>)], name: &str) -> &'a [u8] {
    &pack.iter().find(|(n, _)| n == name).unwrap().1
}

#[test]
fn copies_take_the_output_of_their_original() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = pack_config(
        Some(&root.join(format!(
            "styles/{}.toml",
            pastelplash::config::DEFAULT_STYLE
        ))),
        true,
    );
    if pastelplash::stylize::Stylize::new(&config).is_err() {
        eprintln!("skipping: no GPU adapter");
        return;
    }
    let grass = otex(1, 64, 64, 1);
    // Two copies in the same area (the same pack-map profile) and one in the Deku Tree, whose
    // mood differs: that one is restyled on its own.
    let entries = [
        ("alt/scenes/nonmq/spot04_scene/grass", grass.clone()),
        ("alt/scenes/shared/spot04_scene/grass", grass.clone()),
        ("alt/scenes/shared/ydan_scene/grass", grass.clone()),
    ];
    let dir = tempfile::tempdir().unwrap();
    let (out, summary) = run_pack(dir.path(), &config, &entries, &[]);
    assert_eq!((summary.processed, summary.reused), (3, 1));
    let first = entry(&out, "alt/scenes/nonmq/spot04_scene/grass");
    assert_eq!(first, entry(&out, "alt/scenes/shared/spot04_scene/grass"));
    assert_ne!(first, entry(&out, "alt/scenes/shared/ydan_scene/grass"));
    // The reused output is what the copy gets when it is restyled alone.
    let alone = tempfile::tempdir().unwrap();
    let (out1, s1) = run_pack(
        alone.path(),
        &config,
        &entries,
        &["alt/scenes/shared/spot04_scene/**"],
    );
    assert_eq!(s1.reused, 0);
    assert_eq!(first, entry(&out1, "alt/scenes/shared/spot04_scene/grass"));
}

/// CRC-32 (IEEE) register update, without the initial and final inversion.
fn crc_update(table: &[u32; 256], mut reg: u32, bytes: &[u8]) -> u32 {
    for &b in bytes {
        reg = table[((reg ^ b as u32) & 0xFF) as usize] ^ (reg >> 8);
    }
    reg
}

/// `data` with its last four bytes changed so its CRC-32 is `target`.
fn force_crc(data: &mut [u8], target: u32) {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    let n = data.len() - 4;
    let start = crc_update(&table, !0, &data[..n]);
    // Backwards from the final register: each step's table entry is known by its top byte.
    let mut reg = !target;
    let mut idx = [0usize; 4];
    for k in (0..4).rev() {
        let j = (0..256).find(|&j| table[j] >> 24 == reg >> 24).unwrap();
        idx[k] = j;
        reg = (reg ^ table[j]) << 8;
    }
    // Forwards: the bytes that select those entries.
    let mut reg = start;
    for (k, &j) in idx.iter().enumerate() {
        data[n + k] = ((reg ^ j as u32) & 0xFF) as u8;
        reg = table[j] ^ (reg >> 8);
    }
    assert_eq!(!reg, target);
}

#[test]
fn a_crc_collision_is_not_taken_for_a_copy() {
    // Two different textures of the same size and CRC-32. Under the neutral config every
    // texture comes back unchanged, so each must come back as itself.
    let a = otex(1, 16, 16, 1);
    let mut b = otex(1, 16, 16, 0x5A_5A00);
    let crc = |d: &[u8]| {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("x", opts).unwrap();
        zip.write_all(d).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        archive.by_index(0).unwrap().crc32()
    };
    force_crc(&mut b, crc(&a));
    assert_ne!(a, b);
    assert_eq!(crc(&a), crc(&b));
    let entries = [
        ("alt/scenes/shared/spot04_scene/a", a.clone()),
        ("alt/scenes/shared/spot04_scene/b", b.clone()),
    ];
    let dir = tempfile::tempdir().unwrap();
    let (out, summary) = run_pack(dir.path(), &pack_config(None, false), &entries, &[]);
    assert_eq!((summary.processed, summary.reused), (2, 0));
    assert_eq!(entry(&out, "alt/scenes/shared/spot04_scene/a"), a);
    assert_eq!(entry(&out, "alt/scenes/shared/spot04_scene/b"), b);
}

#[test]
fn mod_contains_only_selected_restyled_textures_with_consistent_headers() {
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
    // Below the target's resolution floor every texture is written 4x larger with a consistent
    // header: dimensions, the HD scale factors at 0x50/0x54 and the data size scaled, the rest
    // carried over.
    let src: Vec<(&str, Vec<u8>)> = sample();
    let u32_at = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let f32_at = |b: &[u8], o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    for (name, data) in &out {
        let orig = &src.iter().find(|(n, _)| n == name).unwrap().1;
        let (w, h, k) = (u32_at(orig, 0x44), u32_at(orig, 0x48), 4);
        let size = w * h * k * k * 4;
        assert_eq!(
            (u32_at(data, 0x44), u32_at(data, 0x48)),
            (w * k, h * k),
            "{name}: size"
        );
        assert_eq!(f32_at(data, 0x50), f32_at(orig, 0x50) * k as f32, "{name}");
        assert_eq!(f32_at(data, 0x54), f32_at(orig, 0x54) * k as f32, "{name}");
        assert_eq!(u32_at(data, 0x58), size, "{name}: data size");
        assert_eq!(data.len(), 0x5C + size as usize, "{name}: length");
        assert_eq!(data[..0x44], orig[..0x44], "{name}: header");
        assert_eq!(data[0x4C..0x50], orig[0x4C..0x50], "{name}: flags");
    }
}
