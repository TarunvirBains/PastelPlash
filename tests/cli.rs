//! The command line as a build pipeline sees it: `--quiet`, `--summary-json` and exit codes.
//! A neutral style (every stage off) keeps these runs off the GPU.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;

fn pastelplash(args: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pastelplash"))
        .args(args)
        .output()
        .expect("running pastelplash")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn write_png(path: &Path) {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 4, 3);
    encoder.set_color(png::ColorType::Rgba);
    let mut writer = encoder.write_header().unwrap();
    let data: Vec<u8> = (0..4 * 3 * 4).map(|i| (i * 37 % 256) as u8).collect();
    writer.write_image_data(&data).unwrap();
    writer.finish().unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, out).unwrap();
}

/// A style with every stage off (the identity pipeline, which opens no GPU).
fn neutral_style(dir: &Path) -> std::path::PathBuf {
    let path = dir.join("neutral.toml");
    fs::write(&path, "name = \"neutral\"\n").unwrap();
    path
}

fn json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

/// in/a.png, in/sub/b.png, in/rock_n.png (a non-color map), in/broken.png (not a PNG)
fn folder(dir: &Path) -> std::path::PathBuf {
    let input = dir.join("in");
    write_png(&input.join("a.png"));
    write_png(&input.join("sub/b.png"));
    write_png(&input.join("rock_n.png"));
    fs::write(input.join("broken.png"), b"not a png").unwrap();
    input
}

#[test]
fn quiet_drops_the_per_texture_lines_and_keeps_the_summary() {
    let dir = tempfile::tempdir().unwrap();
    let (input, style) = (folder(dir.path()), neutral_style(dir.path()));
    fs::remove_file(input.join("broken.png")).unwrap();
    let run = |quiet: bool, out: &str| {
        let out = dir.path().join(out);
        let flag = Path::new("--quiet");
        let mut args = vec![
            Path::new("process"),
            &input,
            &out,
            Path::new("-r"),
            Path::new("--style"),
            &style,
        ];
        if quiet {
            args.push(flag);
        }
        let o = pastelplash(&args);
        assert!(o.status.success(), "{o:?}");
        stdout(&o)
    };
    let loud = run(false, "loud");
    assert!(loud.contains("a.png: 4x3 total"), "{loud}");
    let quiet = run(true, "quiet");
    assert!(!quiet.contains("a.png"), "{quiet}");
    assert!(!quiet.contains("b.png"), "{quiet}");
    assert!(
        quiet.contains("2 processed, 0 copied, 1 skipped, 0 failed"),
        "{quiet}"
    );
    assert!(dir.path().join("quiet/sub/b.png").exists());
}

#[test]
fn summary_json_reports_counts_failures_and_timings() {
    let dir = tempfile::tempdir().unwrap();
    let (input, style) = (folder(dir.path()), neutral_style(dir.path()));
    let (out, summary) = (dir.path().join("out"), dir.path().join("summary.json"));
    let o = pastelplash(&[
        Path::new("process"),
        &input,
        &out,
        Path::new("--recursive"),
        Path::new("--quiet"),
        Path::new("--style"),
        &style,
        Path::new("--summary-json"),
        &summary,
    ]);
    // One file failed: exit code 1, the reason on stderr and in the report.
    assert_eq!(o.status.code(), Some(1), "{o:?}");
    assert!(String::from_utf8_lossy(&o.stderr).contains("broken.png"));
    let v = json(&summary);
    assert_eq!(v["version"], 1);
    assert_eq!(v["command"], "process");
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"], Value::Null);
    assert_eq!(v["output"], out.display().to_string());
    assert_eq!(
        [
            &v["processed"],
            &v["copied"],
            &v["skipped"],
            &v["written"],
            &v["failed"]
        ],
        [2, 0, 1, 3, 1]
    );
    let failures = v["failures"].as_array().unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0]["path"], "broken.png");
    assert!(
        failures[0]["reason"].as_str().unwrap().contains("decoding"),
        "{failures:?}"
    );
    let t = &v["timings"];
    assert!(t["wall_s"].as_f64().unwrap() > 0.0, "{t}");
    for phase in ["read_s", "decode_s", "pipeline_s", "encode_s", "write_s"] {
        assert!(t[phase].as_f64().unwrap() >= 0.0, "{phase}: {t}");
    }

    // Without failures the run succeeds and says so.
    fs::remove_file(input.join("broken.png")).unwrap();
    let o = pastelplash(&[
        Path::new("process"),
        &input,
        &out,
        Path::new("-r"),
        Path::new("-q"),
        Path::new("--style"),
        &style,
        Path::new("--summary-json"),
        &summary,
    ]);
    assert!(o.status.success(), "{o:?}");
    let v = json(&summary);
    assert_eq!(
        (&v["ok"], &v["failed"]),
        (&Value::Bool(true), &Value::from(0))
    );
    assert_eq!(v["failures"].as_array().unwrap().len(), 0);
}

#[test]
fn summary_json_is_written_when_the_run_cannot_start() {
    let dir = tempfile::tempdir().unwrap();
    let style = neutral_style(dir.path());
    let summary = dir.path().join("summary.json");
    let missing = dir.path().join("missing");
    let o = pastelplash(&[
        Path::new("process"),
        &missing,
        &dir.path().join("out"),
        Path::new("--style"),
        &style,
        Path::new("--summary-json"),
        &summary,
    ]);
    assert_eq!(o.status.code(), Some(1), "{o:?}");
    let v = json(&summary);
    assert_eq!(v["ok"], false);
    assert!(
        v["error"].as_str().unwrap().contains("is not a folder"),
        "{v}"
    );
    assert_eq!(v["processed"], 0);
}

fn otex(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let mut b = vec![0u8; 0x5C];
    b[4..8].copy_from_slice(b"XETO");
    b[0x40..0x44].copy_from_slice(&1u32.to_le_bytes());
    b[0x44..0x48].copy_from_slice(&w.to_le_bytes());
    b[0x48..0x4C].copy_from_slice(&h.to_le_bytes());
    b[0x4C..0x50].copy_from_slice(&1u32.to_le_bytes());
    b[0x58..0x5C].copy_from_slice(&(w * h * 4).to_le_bytes());
    b.extend((0..w * h * 4).map(|i| (i.wrapping_mul(2_654_435_761) ^ seed) as u8));
    b
}

#[test]
fn o2r_summary_json_counts_entries() {
    let dir = tempfile::tempdir().unwrap();
    let style = neutral_style(dir.path());
    let input = dir.path().join("in.o2r");
    let mut zip = zip::ZipWriter::new(fs::File::create(&input).unwrap());
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in [
        ("alt/a/tex", otex(8, 8, 1)),
        ("alt/b/tex", otex(8, 8, 1)),
        ("alt/c/tex", otex(8, 8, 2)),
        ("alt/misc/not_a_texture", b"hello".to_vec()),
    ] {
        zip.start_file(name, stored).unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap();
    let (out, summary) = (dir.path().join("out.o2r"), dir.path().join("s.json"));
    let o = pastelplash(&[
        Path::new("o2r"),
        &input,
        &out,
        Path::new("--style"),
        &style,
        Path::new("--quiet"),
        Path::new("--summary-json"),
        &summary,
    ]);
    assert!(o.status.success(), "{o:?}");
    let v = json(&summary);
    assert_eq!(v["command"], "o2r");
    assert_eq!(v["ok"], true);
    // The non-texture is left out of the mod.
    assert_eq!(
        [
            &v["processed"],
            &v["copied"],
            &v["skipped"],
            &v["written"],
            &v["failed"]
        ],
        [3, 0, 1, 3, 0]
    );
    assert!(v["bytes_in"].as_u64().unwrap() > 0);
    assert!(v["bytes_out"].as_u64().unwrap() > 0);
    assert_eq!(v["output"], out.display().to_string());
}
