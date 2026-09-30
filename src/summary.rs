//! What a run did, for the end-of-run summary: counts, failures with their reasons and time per
//! phase, and the machine-readable form written by `--summary-json`.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Serialize;

/// A file (or archive entry) that failed, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failure {
    /// Path relative to the input (an archive entry's name).
    pub path: String,
    pub reason: String,
}

/// Time spent per phase, summed over workers (so it can exceed the wall time).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Phases {
    pub read: Duration,
    pub decode: Duration,
    pub pipeline: Duration,
    pub encode: Duration,
    pub write: Duration,
}

impl std::ops::AddAssign for Phases {
    fn add_assign(&mut self, o: Self) {
        self.read += o.read;
        self.decode += o.decode;
        self.pipeline += o.pipeline;
        self.encode += o.encode;
        self.write += o.write;
    }
}

/// Version of the `--summary-json` layout; raised when a field changes meaning or goes away
/// (new fields may be added without it).
pub const REPORT_VERSION: u32 = 1;

/// The machine-readable end-of-run summary (`--summary-json`).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub version: u32,
    /// The subcommand (`process`, `o2r`).
    pub command: String,
    /// True when the run completed and no file failed (exit code 0).
    pub ok: bool,
    /// Why the run could not start or finish (bad config, missing input, disk space), if so.
    pub error: Option<String>,
    pub input: String,
    pub output: String,
    /// Files restyled by the pipeline.
    pub processed: usize,
    /// Files copied through unchanged (`--copy-other`, `--complete`).
    pub copied: usize,
    /// Files passed through or left out without restyling (non-color maps, `skip` and
    /// unstyled categories).
    pub skipped: usize,
    /// Files whose output was reused from a byte-identical input processed the same way
    /// (counted in `processed` too).
    pub reused: usize,
    /// Files (archive entries) written to the output.
    pub written: usize,
    pub failed: usize,
    pub failures: Vec<Failure>,
    pub timings: Timings,
    /// Bytes read and written (archive runs; `null` for folder runs).
    pub bytes_in: Option<u64>,
    pub bytes_out: Option<u64>,
}

/// Seconds: the wall time and the per-phase sums over workers.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Timings {
    pub wall_s: f64,
    pub read_s: f64,
    pub decode_s: f64,
    pub pipeline_s: f64,
    pub encode_s: f64,
    pub write_s: f64,
}

impl Timings {
    pub fn new(wall: Duration, p: &Phases) -> Self {
        Self {
            wall_s: wall.as_secs_f64(),
            read_s: p.read.as_secs_f64(),
            decode_s: p.decode.as_secs_f64(),
            pipeline_s: p.pipeline.as_secs_f64(),
            encode_s: p.encode.as_secs_f64(),
            write_s: p.write.as_secs_f64(),
        }
    }
}

impl Report {
    /// A report for a run of `command` from `input` to `output` (fill in the rest).
    pub fn new(command: &str, input: &Path, output: &Path) -> Self {
        Self {
            version: REPORT_VERSION,
            command: command.to_string(),
            input: input.display().to_string(),
            output: output.display().to_string(),
            ..Self::default()
        }
    }

    /// Writes the report as pretty-printed JSON (via a temporary file, so a reader never sees
    /// half of it).
    pub fn save(&self, path: &Path) -> Result<()> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        let tmp = path.with_extension("json.partial");
        std::fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_round_trips_as_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("summary.json");
        let mut r = Report::new("o2r", Path::new("in.o2r"), Path::new("out.o2r"));
        r.processed = 3;
        r.failed = 1;
        r.failures.push(Failure {
            path: "alt/a \"b\"".into(),
            reason: "bad\nheader".into(),
        });
        r.timings = Timings::new(
            Duration::from_millis(1500),
            &Phases {
                pipeline: Duration::from_secs(2),
                ..Phases::default()
            },
        );
        r.save(&path).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["version"], REPORT_VERSION);
        assert_eq!(v["command"], "o2r");
        assert_eq!(v["ok"], false);
        assert_eq!(v["error"], serde_json::Value::Null);
        assert_eq!(v["processed"], 3);
        assert_eq!(v["failures"][0]["path"], "alt/a \"b\"");
        assert_eq!(v["failures"][0]["reason"], "bad\nheader");
        assert_eq!(v["timings"]["wall_s"], 1.5);
        assert_eq!(v["timings"]["pipeline_s"], 2.0);
        assert_eq!(v["bytes_in"], serde_json::Value::Null);
        assert!(!dir.path().join("summary.json.partial").exists());
    }
}
