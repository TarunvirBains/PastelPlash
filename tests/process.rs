//! End-to-end runs of the identity pipeline over small folders.

use std::fs;
use std::io::Cursor;
use std::path::Path;

use pastelplash::config::Config;
use pastelplash::image::Image;
use pastelplash::pipeline::{FileContext, Pipeline, Stage};
use pastelplash::process::{self, Options, Summary};
use png::{BitDepth, ColorType, Transformations};

fn write_png(path: &Path, color: ColorType, depth: BitDepth) {
    let (w, h) = (4, 3);
    let len = w * h * color.samples() * depth as usize / 8;
    let data: Vec<u8> = (0..len).map(|i| (i * 37 % 256) as u8).collect();
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w as u32, h as u32);
    encoder.set_color(color);
    encoder.set_depth(depth);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&data).unwrap();
    writer.finish().unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, out).unwrap();
}

fn raw(path: &Path) -> (ColorType, BitDepth, Vec<u8>) {
    let mut decoder = png::Decoder::new(Cursor::new(fs::read(path).unwrap()));
    decoder.set_transformations(Transformations::IDENTITY);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buf).unwrap();
    let (color, depth) = reader.output_color_type();
    (color, depth, buf)
}

/// in/a.png, in/rock_n.png, in/notes.txt, in/sub/b.PNG (16-bit), in/sub/readme.txt
fn sample_input(root: &Path) -> std::path::PathBuf {
    let input = root.join("in");
    write_png(&input.join("a.png"), ColorType::Rgba, BitDepth::Eight);
    write_png(&input.join("rock_n.png"), ColorType::Rgb, BitDepth::Eight);
    write_png(&input.join("sub/b.PNG"), ColorType::Rgb, BitDepth::Sixteen);
    fs::write(input.join("notes.txt"), "hi").unwrap();
    fs::write(input.join("sub/readme.txt"), "hello").unwrap();
    input
}

fn run(opts: &Options) -> Summary {
    let config = Config::default();
    process::run(opts, &config, &Pipeline::from_config(&config).unwrap()).unwrap()
}

fn counts(s: &Summary) -> [usize; 4] {
    [s.processed, s.copied, s.skipped, s.failed]
}

#[test]
fn recursive_run_mirrors_tree_into_nested_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = sample_input(dir.path());
    let output = input.join("out");
    let opts = Options {
        input: input.clone(),
        output: output.clone(),
        recursive: true,
        copy_other: true,
        jobs: Some(2),
        ..Default::default()
    };

    // The second run must not pick up the first run's output.
    for _ in 0..2 {
        assert_eq!(counts(&run(&opts)), [2, 2, 1, 0]);
    }
    assert!(!output.join("out").exists());

    for png in ["a.png", "sub/b.PNG"] {
        assert_eq!(raw(&input.join(png)), raw(&output.join(png)), "{png}");
    }
    for copied in ["rock_n.png", "notes.txt", "sub/readme.txt"] {
        assert_eq!(
            fs::read(input.join(copied)).unwrap(),
            fs::read(output.join(copied)).unwrap()
        );
    }
}

#[test]
fn flat_run_ignores_subfolders_and_other_files() {
    let dir = tempfile::tempdir().unwrap();
    let input = sample_input(dir.path());
    let output = dir.path().join("out");
    let opts = Options {
        input,
        output: output.clone(),
        ..Default::default()
    };

    assert_eq!(counts(&run(&opts)), [1, 0, 1, 0]);
    assert!(output.join("a.png").exists());
    assert!(!output.join("notes.txt").exists());
    assert!(!output.join("sub").exists());
}

#[test]
fn bad_file_is_reported_and_others_continue() {
    let dir = tempfile::tempdir().unwrap();
    let input = sample_input(dir.path());
    fs::write(input.join("broken.png"), b"not a png").unwrap();
    let opts = Options {
        input,
        output: dir.path().join("out"),
        ..Default::default()
    };
    assert_eq!(counts(&run(&opts)), [1, 0, 1, 1]);
}

#[test]
fn output_must_differ_from_input() {
    let dir = tempfile::tempdir().unwrap();
    let input = sample_input(dir.path());
    let opts = Options {
        input: input.clone(),
        output: input,
        ..Default::default()
    };
    let config = Config::default();
    assert!(process::run(&opts, &config, &Pipeline::default()).is_err());
}

struct Invert;

impl Stage for Invert {
    fn name(&self) -> &str {
        "invert"
    }

    fn apply(&self, image: &mut Image, _: &FileContext) -> anyhow::Result<()> {
        for p in &mut image.pixels {
            for c in &mut p[..3] {
                *c = 1.0 - *c;
            }
        }
        Ok(())
    }
}

#[test]
fn stages_are_applied() {
    let dir = tempfile::tempdir().unwrap();
    let input = sample_input(dir.path());
    let output = dir.path().join("out");
    let opts = Options {
        input: input.clone(),
        output: output.clone(),
        ..Default::default()
    };
    let mut pipeline = Pipeline::default();
    pipeline.push(Invert);
    process::run(&opts, &Config::default(), &pipeline).unwrap();

    let (_, _, before) = raw(&input.join("a.png"));
    let (_, _, after) = raw(&output.join("a.png"));
    for (i, (b, a)) in before.iter().zip(&after).enumerate() {
        let expected = if i % 4 == 3 { *b } else { 255 - b };
        assert_eq!(*a, expected);
    }
}
