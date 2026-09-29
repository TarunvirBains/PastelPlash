//! Adapter for libultraship `.o2r` archives (Ship of Harkinian texture packs): a zip of OTEX
//! texture resources. Reads textures straight out of a pack, runs the pipeline, and writes a new
//! pack with the same entry names, so the result loads as a mod.
//!
//! OTEX layout (little-endian): `0x04` type `"XETO"`, `0x08` version, `0x40` original N64 format
//! (5–9 are grayscale/intensity formats the engine tints), `0x44` width, `0x48` height, `0x4C`
//! flags (1 = load as raw), `0x58` data size, then raw RGBA8888 pixels from `0x5C`. Everything up
//! to `0x5C` is carried over unchanged.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use rayon::prelude::*;

use crate::config::{Category, Config, Mood, glob_match};
use crate::driver::Driver;
use crate::image::{Image, SourceColor, SourceFormat};
use crate::pipeline::Pipeline;

const HEADER: usize = 0x5C;

/// An OTEX texture's header, kept verbatim for re-encoding.
pub struct Otex {
    pub header: Vec<u8>,
    pub format: u32,
    pub width: u32,
    pub height: u32,
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

/// Parses an OTEX resource holding raw RGBA8888 pixels. Returns `None` for other resources.
pub fn decode(bytes: &[u8]) -> Option<(Otex, Image)> {
    if bytes.len() < HEADER || &bytes[4..8] != b"XETO" || u32_at(bytes, 0x4C) & 1 == 0 {
        return None;
    }
    let (format, width, height) = (
        u32_at(bytes, 0x40),
        u32_at(bytes, 0x44),
        u32_at(bytes, 0x48),
    );
    let size = u32_at(bytes, 0x58) as usize;
    let n = width as usize * height as usize;
    if size != n * 4 || bytes.len() < HEADER + size || n == 0 {
        return None;
    }
    let pixels = bytes[HEADER..HEADER + size]
        .chunks_exact(4)
        .map(|p| [p[0], p[1], p[2], p[3]].map(|v| v as f32 / 255.0))
        .collect();
    let image = Image {
        width,
        height,
        pixels,
        source: SourceFormat {
            color: SourceColor::Rgba,
            bit_depth: 8,
            has_alpha: true,
        },
        source_scale: None,
        // Grayscale/intensity originals are tinted by the engine.
        tint_safe: (5..=9).contains(&format).then_some(true),
    };
    let otex = Otex {
        header: bytes[..HEADER].to_vec(),
        format,
        width,
        height,
    };
    Some((otex, image))
}

/// Re-encodes pixels with the original header. A texture enlarged by an integer factor (the
/// same on both sides) gets its header rescaled: width and height, the two HD scale factors at
/// `0x50`/`0x54` (multiplied by the same factor, or the game samples the wrong part of it) and
/// the data size.
pub fn encode(otex: &Otex, image: &Image) -> Result<Vec<u8>> {
    let k = image.width / otex.width.max(1);
    ensure!(
        k >= 1 && (image.width, image.height) == (otex.width * k, otex.height * k),
        "texture size changed from {}x{} to {}x{} (only an integer enlargement is allowed)",
        otex.width,
        otex.height,
        image.width,
        image.height
    );
    let mut out = Vec::with_capacity(HEADER + image.pixels.len() * 4);
    out.extend_from_slice(&otex.header);
    if k > 1 {
        let put = |out: &mut Vec<u8>, off: usize, b: [u8; 4]| out[off..off + 4].copy_from_slice(&b);
        let scaled = |off: usize| f32::from_le_bytes(otex.header[off..off + 4].try_into().unwrap());
        put(&mut out, 0x44, image.width.to_le_bytes());
        put(&mut out, 0x48, image.height.to_le_bytes());
        put(&mut out, 0x50, (scaled(0x50) * k as f32).to_le_bytes());
        put(&mut out, 0x54, (scaled(0x54) * k as f32).to_le_bytes());
        let size = u32::try_from(image.pixels.len() * 4).context("texture too large for OTEX")?;
        put(&mut out, 0x58, size.to_le_bytes());
    }
    for p in &image.pixels {
        out.extend(p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
    }
    Ok(out)
}

pub struct Options {
    pub input: PathBuf,
    pub output: PathBuf,
    /// Only entries matching one of these globs (all when empty).
    pub include: Vec<String>,
    /// Category for every entry, overriding the pack map.
    pub category: Option<Category>,
    /// Mood for every entry, overriding the pack map's mood rules.
    pub mood: Option<Mood>,
    /// Also copy unprocessed entries, producing a complete standalone pack.
    pub complete: bool,
    pub jobs: Option<usize>,
}

/// Wall-clock time spent per phase, summed over workers.
#[derive(Default)]
struct Timers {
    read: AtomicU64,
    decode: AtomicU64,
    pipeline: AtomicU64,
    encode: AtomicU64,
    write: AtomicU64,
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
}

fn add(t: &AtomicU64, d: Duration) {
    t.fetch_add(d.as_nanos() as u64, Ordering::Relaxed);
}

fn secs(t: &AtomicU64) -> f64 {
    t.load(Ordering::Relaxed) as f64 / 1e9
}

enum Out {
    Entry(String, Vec<u8>),
    Failed,
}

pub fn run(opts: &Options, config: &Config, pipeline: &Pipeline) -> Result<()> {
    let start = Instant::now();
    let archive = zip::ZipArchive::new(
        File::open(&opts.input).with_context(|| format!("opening {}", opts.input.display()))?,
    )
    .with_context(|| format!("reading {}", opts.input.display()))?;
    let names: Vec<String> = archive
        .file_names()
        .filter(|n| !n.ends_with('/'))
        .filter(|n| opts.include.is_empty() || opts.include.iter().any(|g| glob_match(g, n)))
        .map(String::from)
        .collect();
    drop(archive);
    if names.is_empty() {
        bail!("no entries match");
    }
    let t_index = start.elapsed();
    println!("{} entries selected (index {:.2?})", names.len(), t_index);

    let driver = Driver {
        config,
        pipeline,
        category: opts.category,
        mood: opts.mood.clone(),
    };
    let est = estimate(opts, &names, &driver)?;
    println!("{}", est.summary());
    crate::preflight::check_disk(&opts.output, est.bytes)?;
    let timers = Timers::default();
    let processed = AtomicUsize::new(0);
    let copied = AtomicUsize::new(0);
    let (tx, rx) = mpsc::sync_channel::<Out>(32);

    // Writer thread: stored (uncompressed) entries, like the source packs.
    let output = opts.output.clone();
    let timers_ref = &timers;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opts.jobs.unwrap_or(0))
        .build()?;
    let (written, failed) = std::thread::scope(|scope| -> Result<(usize, usize)> {
        let writer = scope.spawn(move || -> Result<(usize, usize)> {
            if let Some(dir) = output.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = output.with_extension("o2r.partial");
            let file = File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
            let mut zip = zip::ZipWriter::new(BufWriter::with_capacity(8 << 20, file));
            let (mut written, mut failed) = (0, 0);
            for msg in rx {
                let (name, data) = match msg {
                    Out::Entry(n, d) => (n, d),
                    Out::Failed => {
                        failed += 1;
                        continue;
                    }
                };
                let t = Instant::now();
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .large_file(data.len() as u64 >= u32::MAX as u64);
                zip.start_file(name.as_str(), options)?;
                zip.write_all(&data)?;
                timers_ref
                    .bytes_out
                    .fetch_add(data.len() as u64, Ordering::Relaxed);
                add(&timers_ref.write, t.elapsed());
                written += 1;
            }
            let t = Instant::now();
            zip.finish()?.flush()?;
            add(&timers_ref.write, t.elapsed());
            std::fs::rename(&tmp, &output)?;
            Ok((written, failed))
        });

        pool.install(|| {
            names.par_iter().for_each_init(
                || zip::ZipArchive::new(File::open(&opts.input).unwrap()).unwrap(),
                |archive, name| {
                    let result = handle(archive, name, opts, &driver, &timers);
                    let msg = match result {
                        Ok(Some((data, did_process))) => {
                            if did_process {
                                processed.fetch_add(1, Ordering::Relaxed);
                            } else {
                                copied.fetch_add(1, Ordering::Relaxed);
                            }
                            Out::Entry(name.clone(), data)
                        }
                        Ok(None) => return,
                        Err(e) => {
                            eprintln!("error: {name}: {e:#}");
                            Out::Failed
                        }
                    };
                    let _ = tx.send(msg);
                },
            );
        });
        drop(tx);
        writer.join().unwrap()
    })?;

    let wall = start.elapsed().as_secs_f64();
    let mb = |t: &AtomicU64| t.load(Ordering::Relaxed) as f64 / 1e6;
    println!(
        "{} processed, {} copied, {written} written, {failed} failed -> {}",
        processed.load(Ordering::Relaxed),
        copied.load(Ordering::Relaxed),
        opts.output.display()
    );
    println!(
        "wall {wall:.2}s | summed over {} workers: read {:.2}s, decode {:.2}s, pipeline {:.2}s, \
         encode {:.2}s | writer {:.2}s | in {:.0} MB, out {:.0} MB ({:.0} MB/s in)",
        pool.current_num_threads(),
        secs(&timers.read),
        secs(&timers.decode),
        secs(&timers.pipeline),
        secs(&timers.encode),
        secs(&timers.write),
        mb(&timers.bytes_in),
        mb(&timers.bytes_out),
        mb(&timers.bytes_in) / wall,
    );
    if failed > 0 {
        bail!("{failed} entries failed");
    }
    Ok(())
}

/// Width and height of a raw OTEX texture from its header, or `None` for other resources.
fn texture_size(header: &[u8]) -> Option<(u32, u32)> {
    if header.len() < HEADER || &header[4..8] != b"XETO" || u32_at(header, 0x4C) & 1 == 0 {
        return None;
    }
    let (w, h) = (u32_at(header, 0x44), u32_at(header, 0x48));
    (u64::from(u32_at(header, 0x58)) == u64::from(w) * u64::from(h) * 4 && w * h > 0)
        .then_some((w, h))
}

/// What a run will write, from the entry headers alone (enlarged textures counted at their
/// output size).
fn estimate(
    opts: &Options,
    names: &[String],
    driver: &Driver,
) -> Result<crate::preflight::Estimate> {
    let mut archive = zip::ZipArchive::new(
        File::open(&opts.input).with_context(|| format!("opening {}", opts.input.display()))?,
    )?;
    let mut est = crate::preflight::Estimate::default();
    let mut head = vec![0u8; HEADER];
    for name in names {
        let mut entry = archive.by_name(name)?;
        let size = entry.size();
        let texture = match driver.category(Path::new(name)) {
            Some(c) if entry.read_exact(&mut head).is_ok() => texture_size(&head).map(|d| (c, d)),
            _ => None,
        };
        match texture {
            Some((c, (w, h))) => {
                let (k, internal) = driver.resolution_plan(c, w, h);
                let (w, h) = (u64::from(w), u64::from(h));
                let (k, internal) = (u64::from(k), u64::from(internal));
                est.add(
                    HEADER as u64 + w * h * 4 * k * k,
                    k as u32,
                    w * h * internal * internal,
                );
            }
            None if opts.complete => est.add(size, 1, 0),
            None => {}
        }
    }
    Ok(est)
}

/// Exports matching textures as PNGs under `out_dir`, keeping their archive paths (plus
/// `.png`), so pack-map rules still apply to them. The pack is only read. Returns the count.
pub fn export(input: &Path, out_dir: &Path, include: &[String]) -> Result<usize> {
    let archive = zip::ZipArchive::new(
        File::open(input).with_context(|| format!("opening {}", input.display()))?,
    )?;
    let names: Vec<String> = archive
        .file_names()
        .filter(|n| include.is_empty() || include.iter().any(|g| glob_match(g, n)))
        .map(String::from)
        .collect();
    drop(archive);
    let count = AtomicUsize::new(0);
    names.par_iter().try_for_each_init(
        || zip::ZipArchive::new(File::open(input).unwrap()).unwrap(),
        |archive, name| -> Result<()> {
            let mut bytes = Vec::new();
            archive.by_name(name)?.read_to_end(&mut bytes)?;
            let Some((_, image)) = decode(&bytes) else {
                return Ok(());
            };
            let dst = out_dir.join(format!("{name}.png"));
            std::fs::create_dir_all(dst.parent().unwrap())?;
            crate::png_io::write(&image, &dst)?;
            count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        },
    )?;
    Ok(count.into_inner())
}

/// Calls `f` with every texture of a pack whose path matches `include` (all when empty), in
/// parallel on the current rayon pool. The pack is only read.
pub fn for_each_texture(
    input: &Path,
    include: &[String],
    f: &(dyn Fn(&str, Image) -> Result<()> + Sync),
) -> Result<()> {
    let archive = zip::ZipArchive::new(
        File::open(input).with_context(|| format!("opening {}", input.display()))?,
    )?;
    let names: Vec<String> = archive
        .file_names()
        .filter(|n| !n.ends_with('/'))
        .filter(|n| include.is_empty() || include.iter().any(|g| glob_match(g, n)))
        .map(String::from)
        .collect();
    drop(archive);
    names.par_iter().try_for_each_init(
        || zip::ZipArchive::new(File::open(input).unwrap()).unwrap(),
        |archive, name| -> Result<()> {
            let mut bytes = Vec::new();
            archive.by_name(name)?.read_to_end(&mut bytes)?;
            match decode(&bytes) {
                Some((_, image)) => f(name, image).with_context(|| name.clone()),
                None => Ok(()),
            }
        },
    )
}

/// Returns the bytes to write (and whether they were processed), or `None` to leave the entry
/// out of a mod.
fn handle(
    archive: &mut zip::ZipArchive<File>,
    name: &str,
    opts: &Options,
    driver: &Driver,
    timers: &Timers,
) -> Result<Option<(Vec<u8>, bool)>> {
    let t = Instant::now();
    let mut bytes = Vec::new();
    archive.by_name(name)?.read_to_end(&mut bytes)?;
    timers
        .bytes_in
        .fetch_add(bytes.len() as u64, Ordering::Relaxed);
    add(&timers.read, t.elapsed());

    let rel = Path::new(name);
    let category = driver.category(rel);
    let t = Instant::now();
    // Only textures that will be restyled are decoded.
    let decoded = match category {
        Some(c) => decode(&bytes).map(|d| (c, d)),
        None => None,
    };
    add(&timers.decode, t.elapsed());
    let Some((category, (otex, mut image))) = decoded else {
        return Ok(opts.complete.then_some((bytes, false)));
    };

    let t = Instant::now();
    driver.run(&mut image, rel, category)?;
    add(&timers.pipeline, t.elapsed());

    let t = Instant::now();
    let out = encode(&otex, &image)?;
    add(&timers.encode, t.elapsed());
    Ok(Some((out, true)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn otex(format: u32, w: u32, h: u32, px: &[u8]) -> Vec<u8> {
        let mut b = vec![0u8; HEADER];
        b[4..8].copy_from_slice(b"XETO");
        b[0x40..0x44].copy_from_slice(&format.to_le_bytes());
        b[0x44..0x48].copy_from_slice(&w.to_le_bytes());
        b[0x48..0x4C].copy_from_slice(&h.to_le_bytes());
        b[0x4C..0x50].copy_from_slice(&1u32.to_le_bytes());
        b[0x50..0x54].copy_from_slice(&64.0f32.to_le_bytes());
        b[0x58..0x5C].copy_from_slice(&(px.len() as u32).to_le_bytes());
        b.extend_from_slice(px);
        b
    }

    #[test]
    fn otex_round_trips_exactly() {
        let px: Vec<u8> = (0..2 * 3 * 4).map(|i| (i * 37 % 256) as u8).collect();
        let bytes = otex(1, 2, 3, &px);
        let (o, img) = decode(&bytes).unwrap();
        assert_eq!((img.width, img.height), (2, 3));
        assert_eq!(img.tint_safe, None);
        assert_eq!(encode(&o, &img).unwrap(), bytes);
        let (_, gray) = decode(&otex(6, 2, 3, &px)).unwrap();
        assert_eq!(gray.tint_safe, Some(true));
    }

    #[test]
    fn an_enlarged_texture_gets_a_consistent_header() {
        let px: Vec<u8> = (0..2 * 3 * 4).map(|i| (i * 37 % 256) as u8).collect();
        let mut bytes = otex(6, 2, 3, &px);
        bytes[0x54..0x58].copy_from_slice(&16.0f32.to_le_bytes());
        let (o, img) = decode(&bytes).unwrap();
        let big = crate::resample::upsample(&img, 4, [false; 2]);
        let out = encode(&o, &big).unwrap();
        let f32_at = |off: usize| f32::from_le_bytes(out[off..off + 4].try_into().unwrap());
        assert_eq!((u32_at(&out, 0x44), u32_at(&out, 0x48)), (8, 12));
        assert_eq!((f32_at(0x50), f32_at(0x54)), (256.0, 64.0));
        assert_eq!(u32_at(&out, 0x58), 8 * 12 * 4);
        assert_eq!(out.len(), HEADER + 8 * 12 * 4);
        // Everything else in the header is carried over.
        for (i, (a, b)) in out[..HEADER].iter().zip(&bytes[..HEADER]).enumerate() {
            if !(0x44..0x5C).contains(&i) || (0x4C..0x50).contains(&i) {
                assert_eq!(a, b, "header byte {i:#x}");
            }
        }
        // It decodes again as the enlarged texture.
        let (o2, again) = decode(&out).unwrap();
        assert_eq!((o2.width, o2.height, o2.format), (8, 12, 6));
        assert_eq!(again.tint_safe, Some(true));
        // Anything but an integer enlargement of both sides is refused.
        let mut odd = big.clone();
        odd.width = 6;
        odd.pixels.truncate(6 * 12);
        assert!(encode(&o, &odd).is_err());
    }

    #[test]
    fn non_textures_are_ignored() {
        assert!(decode(b"not a texture").is_none());
        let mut bytes = otex(1, 1, 1, &[1, 2, 3, 4]);
        bytes[0x4C] = 0; // not raw
        assert!(decode(&bytes).is_none());
    }
}
