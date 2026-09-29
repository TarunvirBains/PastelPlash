//! Style rules checked on rendered output (GPU). Every style in `styles/` is rendered with the
//! default target on procedural, license-clean test images and checked against the parameters
//! the style sets, within the bounds and tolerances of `rules.toml`. Skips (passes with a
//! message) when no GPU adapter is available. See docs/RULES.md.
//!
//! Rules run over a [`Matrix`] of cases (style × mood × category) and report every failing case
//! at once. Known failures (see [`EXPECTED_FAILURES`]) are printed, not failed.

#[path = "../common/mod.rs"]
mod common;

mod actor;
mod darks;
mod identity;
mod robustness;
mod technique;

use std::path::PathBuf;

use common::*;
use pastelplash::config::{Category, Config, Mood};
use pastelplash::image::Image;

/// Real rule failures that are known and waiting for a decision (a style or contract change),
/// as (rule, case label). They are printed on every run instead of failing it; a listed case
/// that passes again is reported so it can be removed.
const EXPECTED_FAILURES: &[(&str, &str)] = &[];

/// Every category the stage restyles.
pub const STYLIZED: [Category; 4] = [
    Category::World,
    Category::Actor,
    Category::Background,
    Category::Skybox,
];

/// One render case: a style in a mood, as a category.
pub struct Case {
    pub style: String,
    pub path: PathBuf,
    pub config: Config,
    pub mood: Mood,
    pub category: Category,
}

impl Case {
    pub fn label(&self) -> String {
        format!("{} [{}] {:?}", self.style, self.mood, self.category)
    }

    /// Renders `img` for this case; `None` without a GPU.
    pub fn render(&self, img: &Image) -> Option<Image> {
        render_mood(&self.path, &self.config, self.category, &self.mood, img)
    }
}

/// The cases a rule is checked over.
pub struct Matrix {
    pub cases: Vec<Case>,
}

impl Matrix {
    fn build(categories: &[Category], all_moods: bool) -> Self {
        let mut cases = Vec::new();
        for (_, path, config, mood) in style_moods() {
            if !all_moods && !mood.is_base() {
                continue;
            }
            for &category in categories {
                cases.push(Case {
                    style: name(&path),
                    path: path.clone(),
                    config: config.clone(),
                    mood: mood.clone(),
                    category,
                });
            }
        }
        Self { cases }
    }

    /// Every style in the base mood, as each category.
    pub fn base(categories: &[Category]) -> Self {
        Self::build(categories, false)
    }

    /// Every style in every mood it defines (base, and each mood at half and full strength),
    /// as each category.
    pub fn full(categories: &[Category]) -> Self {
        Self::build(categories, true)
    }

    /// The same cases with the pack map raising every file's brushwork by `strength` (the
    /// props multiplier).
    pub fn with_brushwork(mut self, strength: f32) -> Self {
        for case in &mut self.cases {
            case.config.pack.brushwork = vec![pastelplash::config::BrushworkRule {
                glob: "**".into(),
                strength,
            }];
        }
        self
    }

    /// Renders `img` for every case and checks it with `f`, recording failures in `report`.
    /// Returns false (and checks nothing) without a GPU.
    pub fn check(
        &self,
        report: &mut Report,
        img: &Image,
        f: impl Fn(&Case, &Image) -> Result<(), String>,
    ) -> bool {
        for case in &self.cases {
            let Some(out) = case.render(img) else {
                return false;
            };
            if let Err(e) = f(case, &out) {
                report.fail(&case.label(), e);
            }
        }
        true
    }
}

/// Collects every failing case of one rule, then fails once, listing them all.
pub struct Report {
    rule: &'static str,
    failures: Vec<(String, String)>,
}

impl Report {
    pub fn new(rule: &'static str) -> Self {
        Self {
            rule,
            failures: Vec::new(),
        }
    }

    pub fn fail(&mut self, case: &str, message: String) {
        self.failures.push((case.to_string(), message));
    }

    /// Records `Err` results for `case`.
    pub fn check(&mut self, case: &str, result: Result<(), String>) {
        if let Err(e) = result {
            self.fail(case, e);
        }
    }

    fn expected(&self, case: &str) -> bool {
        EXPECTED_FAILURES
            .iter()
            .any(|(r, c)| *r == self.rule && *c == case)
    }

    /// Panics if any case failed that is not a known failure.
    pub fn finish(self) {
        let (known, new): (Vec<_>, Vec<_>) =
            self.failures.iter().partition(|(c, _)| self.expected(c));
        for (case, e) in &known {
            println!("KNOWN FAILURE {}: {case}: {e}", self.rule);
        }
        for (r, case) in EXPECTED_FAILURES {
            if *r == self.rule && !self.failures.iter().any(|(c, _)| c == case) {
                println!(
                    "known failure now passes (remove it from EXPECTED_FAILURES): {r}: {case}"
                );
            }
        }
        let list: Vec<String> = new.iter().map(|(c, e)| format!("  {c}: {e}")).collect();
        assert!(
            new.is_empty(),
            "{}: {} failing case(s):\n{}",
            self.rule,
            new.len(),
            list.join("\n")
        );
    }
}

/// Fails if more than the contract's outlier budget (plus `extra`, e.g. a style's accent darks
/// that have their own rule) of opaque texels fail `bad`.
pub fn few(out: &Image, extra: f32, bad: impl Fn([f32; 4]) -> bool) -> Result<(), String> {
    let opaque: Vec<[f32; 4]> = out.pixels.iter().copied().filter(|p| p[3] > 0.5).collect();
    let failing: Vec<[f32; 4]> = opaque.iter().copied().filter(|&p| bad(p)).collect();
    let budget = (opaque.len() as f32 * (contract().tolerance.outliers + extra)).ceil() as usize;
    if failing.len() <= budget {
        return Ok(());
    }
    Err(format!(
        "{} of {} texels fail (budget {budget}); e.g. {:?} = LCh {:?}",
        failing.len(),
        opaque.len(),
        failing[0],
        lch(failing[0])
    ))
}

/// The source as the case's mood intends to dim it: its lightness through the mood's moonlight
/// exposure (`palette::cast_exposure`; the source itself without a cast). The contract bounds
/// that exposure (`moods.<name>.min_exposure`, `rule_moonlight_cast_only_where_the_mood_allows`),
/// and rules on value and identity judge the rest of the look against this reference.
pub fn dimmed(case: &Case, img: &Image) -> Image {
    let style = case.config.style.for_mood(&case.mood).unwrap();
    let scale = case.config.target.treatment(case.category).cast;
    if pastelplash::palette::cast_strength(&style.palette, scale) <= 0.0 {
        return img.clone();
    }
    let mut out = img.clone();
    for p in &mut out.pixels {
        let [l, a, b] = pastelplash::color::srgb_to_oklab([p[0], p[1], p[2]]);
        let l2 = pastelplash::palette::cast_exposure(&style.palette, scale, l);
        let rgb = pastelplash::color::oklab_to_srgb([l2, a, b]);
        for k in 0..3 {
            p[k] = rgb[k].clamp(0.0, 1.0);
        }
    }
    out
}

/// `Err(message)` unless `ok`.
pub fn ensure(ok: bool, message: impl FnOnce() -> String) -> Result<(), String> {
    if ok { Ok(()) } else { Err(message()) }
}

pub fn median(mut v: Vec<f32>) -> f32 {
    let i = v.len() / 2;
    *v.select_nth_unstable_by(i, f32::total_cmp).1
}

/// Median lightness standard deviation over 7×7 windows on a grid.
pub fn local_std(img: &Image) -> f32 {
    let (w, h) = (img.width as usize, img.height as usize);
    let l: Vec<f32> = img.pixels.iter().map(|&p| lch(p)[0]).collect();
    let mut v = Vec::new();
    for y in (3..h - 3).step_by(5) {
        for x in (3..w - 3).step_by(5) {
            let (mut s, mut s2) = (0.0, 0.0);
            for yy in y - 3..=y + 3 {
                for xx in x - 3..=x + 3 {
                    let t = l[yy * w + xx];
                    s += t;
                    s2 += t * t;
                }
            }
            let m = s / 49.0;
            v.push((s2 / 49.0 - m * m).max(0.0f32).sqrt());
        }
    }
    median(v)
}

/// Median lightness standard deviation over 25×25 windows on a grid.
pub fn mid_std(img: &Image) -> f32 {
    let (w, h) = (img.width as usize, img.height as usize);
    let l: Vec<f32> = img.pixels.iter().map(|&p| lch(p)[0]).collect();
    let mut v = Vec::new();
    for y in (12..h - 12).step_by(9) {
        for x in (12..w - 12).step_by(9) {
            let (mut s, mut s2, mut n) = (0.0f32, 0.0f32, 0.0f32);
            for yy in (y - 12..=y + 12).step_by(2) {
                for xx in (x - 12..=x + 12).step_by(2) {
                    let t = l[yy * w + xx];
                    s += t;
                    s2 += t * t;
                    n += 1.0;
                }
            }
            let m = s / n;
            v.push((s2 / n - m * m).max(0.0).sqrt());
        }
    }
    median(v)
}

#[test]
#[should_panic(expected = "2 failing case(s)")]
fn a_report_fails_listing_every_failing_case() {
    let mut report = Report::new("self-test");
    report.fail("a", "first".into());
    report.check("b", Err("second".into()));
    report.check("c", Ok(()));
    report.finish();
}

/// `config` with `[section] key = value` set in the style, in the base and in every mood (moods
/// derive from the style's TOML, so a typed field alone would not reach them).
pub fn tweaked(config: &Config, section: &str, key: &str, value: f64) -> Config {
    use toml::{Table, Value};
    let mut raw = config.style.raw.clone().expect("style loaded from TOML");
    raw.entry(section)
        .or_insert_with(|| Value::Table(Table::new()))
        .as_table_mut()
        .unwrap()
        .insert(key.into(), Value::Float(value));
    if let Some(moods) = raw.get_mut("moods").and_then(Value::as_table_mut) {
        for (_, mood) in moods.iter_mut() {
            if let Some(s) = mood.get_mut(section).and_then(Value::as_table_mut) {
                s.remove(key);
            }
        }
    }
    let mut style = pastelplash::config::Style::parse(&toml::to_string(&raw).unwrap()).unwrap();
    style.lut = config.style.lut.clone();
    Config {
        style,
        ..config.clone()
    }
}

#[test]
fn the_full_matrix_covers_every_mood_at_half_and_full_strength() {
    let want: usize = styles()
        .iter()
        .map(|p| 1 + 2 * load(p, &default_target()).style.moods.len())
        .sum();
    let m = Matrix::full(&STYLIZED);
    assert_eq!(m.cases.len(), want * STYLIZED.len());
    assert!(
        m.cases
            .iter()
            .any(|c| c.mood.name == "nocturne" && c.mood.strength == 0.5)
    );
    assert!(want > styles().len(), "no style defines a mood");
}
