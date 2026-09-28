//! Folder walker: finds the files to process under an input folder.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct WalkOptions {
    /// Descend into subfolders.
    pub recursive: bool,
    /// Follow symlinks (and junctions). Loops are detected and skipped.
    pub follow_links: bool,
    /// Folder to leave out, typically the output folder when it is nested inside the input.
    pub exclude: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Path relative to the walk root.
    pub rel: PathBuf,
    pub is_png: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// A symlink that was not followed.
    Symlink,
    /// A followed symlink pointing back at one of its own ancestors.
    Loop,
    /// The excluded (output) folder.
    Excluded,
}

#[derive(Debug, Default)]
pub struct Walk {
    /// Every file found, sorted by relative path.
    pub entries: Vec<Entry>,
    /// Paths deliberately not visited, relative to the root.
    pub skipped: Vec<(PathBuf, SkipReason)>,
    /// Paths that could not be read, relative to the root.
    pub errors: Vec<(PathBuf, io::Error)>,
}

pub fn is_png(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png"))
}

/// Lists files under `root`. Fails only if `root` itself cannot be read; problems further down are
/// collected in [`Walk::errors`].
pub fn walk(root: &Path, opts: &WalkOptions) -> io::Result<Walk> {
    let root_canon = fs::canonicalize(root)?;
    let exclude = opts.exclude.as_deref().and_then(|p| fs::canonicalize(p).ok());
    let mut walker = Walker { opts, exclude, out: Walk::default() };
    fs::read_dir(root)?;
    walker.visit(root, Path::new(""), &mut vec![root_canon]);
    walker.out.entries.sort_by(|a, b| a.rel.cmp(&b.rel));
    walker.out.skipped.sort_by(|a, b| a.0.cmp(&b.0));
    walker.out.errors.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(walker.out)
}

struct Walker<'a> {
    opts: &'a WalkOptions,
    exclude: Option<PathBuf>,
    out: Walk,
}

impl Walker<'_> {
    /// `ancestors` holds the canonical paths of `dir` and every folder above it, for loop detection.
    fn visit(&mut self, dir: &Path, rel_dir: &Path, ancestors: &mut Vec<PathBuf>) {
        let read = match fs::read_dir(dir) {
            Ok(read) => read,
            Err(e) => return self.out.errors.push((rel_dir.to_path_buf(), e)),
        };
        for entry in read {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    self.out.errors.push((rel_dir.to_path_buf(), e));
                    continue;
                }
            };
            let path = entry.path();
            let rel = rel_dir.join(entry.file_name());
            let file_type = match entry.file_type() {
                Ok(t) if t.is_symlink() && !self.opts.follow_links => {
                    self.out.skipped.push((rel, SkipReason::Symlink));
                    continue;
                }
                Ok(t) if t.is_symlink() => fs::metadata(&path).map(|m| m.file_type()),
                other => other,
            };
            let file_type = match file_type {
                Ok(t) => t,
                Err(e) => {
                    self.out.errors.push((rel, e));
                    continue;
                }
            };

            if file_type.is_file() {
                let is_png = is_png(&rel);
                self.out.entries.push(Entry { rel, is_png });
            } else if file_type.is_dir() && self.opts.recursive {
                let canon = match fs::canonicalize(&path) {
                    Ok(c) => c,
                    Err(e) => {
                        self.out.errors.push((rel, e));
                        continue;
                    }
                };
                if self.exclude.as_ref() == Some(&canon) {
                    self.out.skipped.push((rel, SkipReason::Excluded));
                } else if ancestors.contains(&canon) {
                    self.out.skipped.push((rel, SkipReason::Loop));
                } else {
                    ancestors.push(canon);
                    self.visit(&path, &rel, ancestors);
                    ancestors.pop();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, rel: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"x").unwrap();
    }

    fn rels(walk: &Walk) -> Vec<String> {
        walk.entries
            .iter()
            .map(|e| e.rel.to_string_lossy().replace('\\', "/"))
            .collect()
    }

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for rel in ["b.png", "A.PNG", "notes.txt", "sub/c.Png", "sub/deeper/d.png"] {
            touch(dir.path(), rel);
        }
        dir
    }

    #[test]
    fn non_recursive_lists_only_top_level() {
        let dir = tree();
        let walk = walk(dir.path(), &WalkOptions::default()).unwrap();
        assert_eq!(rels(&walk), ["A.PNG", "b.png", "notes.txt"]);
    }

    #[test]
    fn recursive_lists_everything_sorted() {
        let dir = tree();
        let opts = WalkOptions { recursive: true, ..Default::default() };
        let walk = walk(dir.path(), &opts).unwrap();
        assert_eq!(
            rels(&walk),
            ["A.PNG", "b.png", "notes.txt", "sub/c.Png", "sub/deeper/d.png"]
        );
    }

    #[test]
    fn png_extension_is_case_insensitive() {
        let dir = tree();
        let opts = WalkOptions { recursive: true, ..Default::default() };
        let walk = walk(dir.path(), &opts).unwrap();
        let pngs: Vec<_> = walk.entries.iter().map(|e| e.is_png).collect();
        assert_eq!(pngs, [true, true, false, true, true]);
        assert!(!is_png(Path::new("png")));
        assert!(!is_png(Path::new("a.png.bak")));
    }

    #[test]
    fn nested_output_is_skipped() {
        let dir = tree();
        touch(dir.path(), "out/sub/c.png");
        let opts = WalkOptions {
            recursive: true,
            exclude: Some(dir.path().join("out")),
            ..Default::default()
        };
        let walk = walk(dir.path(), &opts).unwrap();
        assert!(rels(&walk).iter().all(|r| !r.starts_with("out")));
        assert_eq!(walk.skipped, [(PathBuf::from("out"), SkipReason::Excluded)]);
    }

    #[cfg(unix)]
    fn symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    /// Falls back to a junction (no privileges needed) when symlinks are not permitted.
    #[cfg(windows)]
    fn symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link).or_else(|_| {
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .current_dir(std::env::temp_dir())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()?;
            if status.success() {
                Ok(())
            } else {
                Err(io::Error::other("mklink /J failed"))
            }
        })
    }

    /// Creates `sub/loop -> <root>`, or returns `None` when symlinks need privileges we lack.
    fn tree_with_loop() -> Option<tempfile::TempDir> {
        let dir = tree();
        match symlink_dir(dir.path(), &dir.path().join("sub").join("loop")) {
            Ok(()) => Some(dir),
            Err(e) => {
                eprintln!("skipping symlink test: {e}");
                None
            }
        }
    }

    #[test]
    fn symlinks_are_not_followed_by_default() {
        let Some(dir) = tree_with_loop() else { return };
        let opts = WalkOptions { recursive: true, ..Default::default() };
        let walk = walk(dir.path(), &opts).unwrap();
        assert_eq!(rels(&walk).len(), 5);
        assert_eq!(walk.skipped, [(PathBuf::from("sub/loop"), SkipReason::Symlink)]);
    }

    #[test]
    fn followed_symlink_loops_are_cut() {
        let Some(dir) = tree_with_loop() else { return };
        let opts = WalkOptions { recursive: true, follow_links: true, ..Default::default() };
        let walk = walk(dir.path(), &opts).unwrap();
        assert_eq!(rels(&walk).len(), 5);
        assert_eq!(walk.skipped, [(PathBuf::from("sub/loop"), SkipReason::Loop)]);
    }

    #[test]
    fn followed_symlink_to_sibling_is_walked() {
        let dir = tree();
        let other = tempfile::tempdir().unwrap();
        touch(other.path(), "e.png");
        if let Err(e) = symlink_dir(other.path(), &dir.path().join("linked")) {
            eprintln!("skipping symlink test: {e}");
            return;
        }
        let opts = WalkOptions { recursive: true, follow_links: true, ..Default::default() };
        let walk = walk(dir.path(), &opts).unwrap();
        assert!(rels(&walk).contains(&"linked/e.png".to_string()));
    }
}
