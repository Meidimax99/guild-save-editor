//! Virtual file system over the game's `Resources/*.bin` ZIP archives.
//!
//! Mirrors the original `vfs_*` layer (`FUN_00429760` open, `FUN_00428be0` lookup):
//!
//! * every `Resources/<stem>.bin[N]` archive is mounted at `<stem>/`; numbered
//!   variants (`scripts.bin1`) overlay the base archive,
//! * paths are case-insensitive, `\` and `/` are equivalent, leading slashes ignored,
//! * a final path component starting with `*` (e.g. `scenes/*ob_kirche.ed3`) is a
//!   recursive search for that file name anywhere below the directory,
//! * loose files under the game root (e.g. `Resources/gamedata/...`) are reachable
//!   by their path relative to the game root.

use crate::error::{Error, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

enum Source {
    Zip { archive: usize, index: usize },
    Loose(PathBuf),
}

pub struct Vfs {
    archives: Vec<Mutex<zip::ZipArchive<File>>>,
    files: HashMap<String, Source>,
    /// file name → full normalized paths, for `*name` searches.
    by_name: HashMap<String, Vec<String>>,
    root: PathBuf,
}

pub fn normalize(path: &str) -> String {
    let p = path.replace('\\', "/").to_lowercase();
    let mut out = String::with_capacity(p.len());
    for comp in p.split('/').filter(|c| !c.is_empty() && *c != ".") {
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(comp);
    }
    out
}

impl Vfs {
    /// Mount a game installation (the directory containing `Resources/`).
    pub fn open_game_dir(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let res = find_ci(&root, "resources").ok_or_else(|| Error::NotFound(root.join("Resources")))?;
        let mut vfs = Vfs { archives: Vec::new(), files: HashMap::new(), by_name: HashMap::new(), root: root.clone() };

        let mut bins: Vec<(String, u32, PathBuf)> = std::fs::read_dir(&res)?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_lowercase();
                let (stem, ext) = name.rsplit_once('.')?;
                let layer = ext.strip_prefix("bin")?;
                let layer = if layer.is_empty() { 0 } else { layer.parse::<u32>().ok()? + 1 };
                Some((stem.to_string(), layer, e.path()))
            })
            .collect();
        // Lower layers first so overlays replace their entries.
        bins.sort_by(|a, b| (a.1, &a.0).cmp(&(b.1, &b.0)));
        for (stem, _, path) in bins {
            vfs.mount_zip(&stem, &path)?;
        }
        vfs.mount_loose(&root, &root)?;
        vfs.rebuild_name_index();
        Ok(vfs)
    }

    /// Mount one ZIP archive at `prefix/`.
    pub fn mount_zip(&mut self, prefix: &str, path: &Path) -> Result<()> {
        let mut zip = zip::ZipArchive::new(File::open(path)?)?;
        let archive = self.archives.len();
        for index in 0..zip.len() {
            let f = zip.by_index(index)?;
            if f.is_dir() {
                continue;
            }
            let key = normalize(&format!("{prefix}/{}", f.name()));
            self.files.insert(key, Source::Zip { archive, index });
        }
        self.archives.push(Mutex::new(zip));
        Ok(())
    }

    fn mount_loose(&mut self, root: &Path, dir: &Path) -> Result<()> {
        for e in std::fs::read_dir(dir)?.flatten() {
            let p = e.path();
            let ft = e.file_type()?;
            if ft.is_dir() {
                self.mount_loose(root, &p)?;
            } else if ft.is_file() {
                let rel = p.strip_prefix(root).unwrap().to_string_lossy();
                let key = normalize(&rel);
                self.files.entry(key).or_insert(Source::Loose(p.clone()));
            }
        }
        Ok(())
    }

    fn rebuild_name_index(&mut self) {
        self.by_name.clear();
        for k in self.files.keys() {
            let name = k.rsplit('/').next().unwrap().to_string();
            self.by_name.entry(name).or_default().push(k.clone());
        }
        for v in self.by_name.values_mut() {
            v.sort();
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a game path (possibly containing a `*name` search) to its canonical key.
    pub fn resolve(&self, path: &str) -> Option<String> {
        let norm = normalize(path);
        let (dir, name) = match norm.rsplit_once('/') {
            Some((d, n)) => (d, n),
            None => ("", norm.as_str()),
        };
        if let Some(name) = name.strip_prefix('*') {
            let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
            return self.by_name.get(name)?.iter().find(|k| k.starts_with(&prefix)).cloned();
        }
        self.files.contains_key(&norm).then_some(norm)
    }

    pub fn exists(&self, path: &str) -> bool {
        self.resolve(path).is_some()
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let key = self.resolve(path).ok_or_else(|| Error::NotFound(path.into()))?;
        match &self.files[&key] {
            Source::Loose(p) => Ok(std::fs::read(p)?),
            Source::Zip { archive, index } => {
                let mut zip = self.archives[*archive].lock().unwrap();
                let mut f = zip.by_index(*index)?;
                let mut buf = Vec::with_capacity(f.size() as usize);
                f.read_to_end(&mut buf)?;
                Ok(buf)
            }
        }
    }

    /// All canonical paths, sorted.
    pub fn list(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.files.keys().map(|s| s.as_str()).collect();
        v.sort();
        v
    }

    /// Canonical paths below `dir` with the given extension (case-insensitive).
    pub fn glob(&self, dir: &str, ext: &str) -> Vec<&str> {
        let dir = normalize(dir);
        let ext = format!(".{}", ext.to_lowercase());
        let mut v: Vec<&str> = self
            .files
            .keys()
            .filter(|k| (dir.is_empty() || k.starts_with(&format!("{dir}/"))) && k.ends_with(&ext))
            .map(|s| s.as_str())
            .collect();
        v.sort();
        v
    }
}

/// Case-insensitive child lookup (the game was authored on Windows).
pub fn find_ci(dir: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
        .map(|e| e.path())
}

/// Locate a game installation when none is given: the current directory, its
/// ancestors, and their immediate subdirectories are searched for a folder that
/// contains `Resources/scripts.bin`.
pub fn find_game_dir() -> Option<PathBuf> {
    find_game_dir_from(&std::env::current_dir().ok()?)
}

/// Like [`find_game_dir`], starting at `start` instead of the current directory.
pub fn find_game_dir_from(start: &Path) -> Option<PathBuf> {
    let is_game = |d: &Path| find_ci(d, "resources").is_some_and(|r| find_ci(&r, "scripts.bin").is_some());
    let start = std::fs::canonicalize(start).ok()?;
    for dir in start.ancestors() {
        if is_game(dir) {
            return Some(dir.to_path_buf());
        }
        let mut subs: Vec<PathBuf> =
            std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        subs.sort();
        if let Some(p) = subs.into_iter().find(|p| is_game(p)) {
            return Some(p);
        }
    }
    None
}
