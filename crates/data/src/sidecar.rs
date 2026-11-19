//! The files that go with a file, in whichever ASCII case the folder spells them. A whole
//! Shapefile is found once per read, publish or save from one listing of its folder
//! ([`Sidecars`]), so every reader and writer picks the same file and writes each part back
//! under the name it has; one file of one extension is found by trying its spellings
//! ([`find_one`]), without listing the folder.

use std::path::{Path, PathBuf};

/// The parts of a Shapefile besides its `.shp` that are read and written.
pub(crate) const SHAPEFILE_COMPANIONS: [&str; 4] = ["shx", "dbf", "prj", "cpg"];

/// The spatial indexes beside a Shapefile, which no longer match once it is written again.
pub(crate) const SHAPEFILE_INDEXES: [&str; 3] = ["qix", "sbn", "sbx"];

/// A Shapefile's files by their extensions in lower case: its shapes, its
/// [`SHAPEFILE_COMPANIONS`], then its [`SHAPEFILE_INDEXES`].
pub(crate) const SHAPEFILE: [&str; 8] = {
    let [shx, dbf, prj, cpg] = SHAPEFILE_COMPANIONS;
    let [qix, sbn, sbx] = SHAPEFILE_INDEXES;
    ["shp", shx, dbf, prj, cpg, qix, sbn, sbx]
};

/// Whether an extension is in upper case (`SHP`, not `shp` or `Shp`): a part written for the
/// first time beside such a `.shp` is named in upper case too.
pub(crate) fn upper_extension(ext: &[u8]) -> bool {
    !ext.is_empty() && !ext.iter().any(u8::is_ascii_lowercase)
}

/// Whether two paths reach one file ([`Identity`]); `false` when either reaches nothing.
pub(crate) fn same_file(a: &Path, b: &Path) -> bool {
    match (a.metadata(), b.metadata()) {
        (Ok(ma), Ok(mb)) => identity(a, &ma) == identity(b, &mb),
        _ => false,
    }
}

/// The most folder entries one listing reads; a larger folder has each spelling tried instead.
const LISTING_LIMIT: usize = 100_000;

/// Every ASCII case of `ext` (given in lower case): the lower-case one first, then the others in
/// byte order, the order a part's spellings are taken in.
pub(crate) fn case_spellings(ext: &str) -> Vec<String> {
    let letters = ext.len().min(8);
    let mut spellings: Vec<String> = (0u32..1 << letters)
        .map(|mask| ext.chars().enumerate().map(|(i, c)| if i < 8 && mask & (1 << i) != 0 { c.to_ascii_uppercase() } else { c }).collect())
        .collect();
    spellings.sort_by(|a, b| (a != ext, a).cmp(&(b != ext, b)));
    spellings.dedup();
    spellings
}

/// The file with extension `ext` (in lower case) beside `path`: the first of its
/// [`case_spellings`] that is a file (following a link), each tried with one look at the disk,
/// at most eight. Where the file system ignores case, the name returned is the one asked for.
pub(crate) fn find_one(path: &Path, ext: &str) -> Option<PathBuf> {
    case_spellings(ext).into_iter().map(|e| path.with_extension(e)).find(|p| p.is_file())
}

/// One extension's entries beside the file.
#[derive(Debug, Clone)]
struct Part {
    ext: &'static str,
    /// Its entries (any but folders, so a dangling link too), the lower-case spelling first and
    /// then the others in byte order, each with whether it is a file (following a link).
    found: Vec<(PathBuf, bool)>,
}

impl Part {
    /// The entry taken: the first that is a file.
    fn taken(&self) -> Option<&Path> {
        self.found.iter().find(|(_, file)| *file).map(|(p, _)| p.as_path())
    }
}

/// A Shapefile's files, each extension with every entry its folder has for it.
#[derive(Debug, Clone)]
pub(crate) struct Sidecars {
    /// The `.shp`, as its folder spells it (as given when it isn't there).
    file: PathBuf,
    /// Whether a part written for the first time is named in upper case: the `.shp`'s extension is.
    upper: bool,
    parts: Vec<Part>,
}

/// What tells two entries for one file apart from two files: device and inode on Unix, else the
/// path the system resolves the entry to (its name as the disk has it), or the entry's own path
/// when it resolves to nothing (a dangling link).
#[derive(PartialEq)]
enum Identity {
    #[cfg(unix)]
    Inode(u64, u64),
    #[cfg(not(unix))]
    Path(PathBuf),
}

fn identity(path: &Path, meta: &std::fs::Metadata) -> Identity {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        Identity::Inode(meta.dev(), meta.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        Identity::Path(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
    }
}

impl Sidecars {
    /// The parts of the Shapefile whose `.shp` is at `path` ([`SHAPEFILE`]), named its stem and
    /// an extension in any ASCII case, from one listing of its folder. Where the folder ignores
    /// case, a stem in another case (`roads.shp` for `ROADS.SHP`) names the same files and they
    /// keep the names they have; where it tells cases apart, only the stem as the file has it
    /// counts. When the folder can't be listed or holds more than [`LISTING_LIMIT`] entries,
    /// each spelling of each part is tried instead, keeping every one there but for spellings
    /// that reach a file already kept.
    pub(crate) fn shapefile(path: &Path) -> Sidecars {
        let upper_of = |p: &Path| p.extension().is_some_and(|e| upper_extension(e.as_encoded_bytes()));
        let mut found: Vec<(&'static str, Vec<PathBuf>)> = SHAPEFILE.iter().map(|&e| (e, Vec::new())).collect();
        let file = match (path.file_name(), list(path)) {
            (Some(name), Some(entries)) => {
                let name = name.as_encoded_bytes();
                let exact = entries.iter().any(|(n, _)| n.as_encoded_bytes() == name);
                let other = entries.iter().find(|(n, _)| n.as_encoded_bytes() != name && n.as_encoded_bytes().eq_ignore_ascii_case(name));
                // Whether the folder ignores case: the file is there under another spelling, or
                // its name in the opposite case, not listed, still reaches a file.
                let ignores_case = if exact {
                    let flipped: Vec<u8> =
                        name.iter().map(|b| if b.is_ascii_lowercase() { b.to_ascii_uppercase() } else { b.to_ascii_lowercase() }).collect();
                    flipped != name
                        && !entries.iter().any(|(n, _)| n.as_encoded_bytes() == flipped.as_slice())
                        && std::str::from_utf8(&flipped).is_ok_and(|f| path.with_file_name(f).symlink_metadata().is_ok())
                } else {
                    other.is_some() && path.symlink_metadata().is_ok()
                };
                let file = match other {
                    Some((n, _)) if !exact && ignores_case => path.with_file_name(n),
                    _ => path.to_path_buf(),
                };
                let stem = file.file_stem().map(|s| s.as_encoded_bytes().to_vec()).unwrap_or_default();
                for (n, is_dir) in &entries {
                    let bytes = n.as_encoded_bytes();
                    let Some(dot) = bytes.iter().rposition(|&b| b == b'.') else { continue };
                    let (base, ext) = (bytes.get(..dot).unwrap_or_default(), bytes.get(dot + 1..).unwrap_or_default());
                    if *is_dir || !(base == stem.as_slice() || (ignores_case && base.eq_ignore_ascii_case(&stem))) {
                        continue;
                    }
                    if let Some((_, part)) = found.iter_mut().find(|(e, _)| e.as_bytes().eq_ignore_ascii_case(ext)) {
                        part.push(path.with_file_name(n));
                    }
                }
                file
            }
            _ => {
                let upper = upper_of(path);
                for (ext, part) in &mut found {
                    let mut seen: Vec<Identity> = Vec::new();
                    // Spellings that reach the `.shp` itself are it, never another Shapefile.
                    if *ext == "shp"
                        && let Ok(meta) = path.symlink_metadata()
                    {
                        seen.push(identity(path, &meta));
                        part.push(path.to_path_buf());
                    }
                    // Where case is ignored every spelling reaches one file: it is named as the
                    // `.shp`'s case suggests, the guess nearest the name it has.
                    let mut spellings = case_spellings(ext);
                    if upper {
                        spellings.sort_by_key(|s| !upper_extension(s.as_bytes()));
                    }
                    for spelling in spellings {
                        let p = path.with_extension(&spelling);
                        let Ok(meta) = p.symlink_metadata() else { continue };
                        let id = identity(&p, &meta);
                        if meta.is_dir() || seen.contains(&id) {
                            continue;
                        }
                        seen.push(id);
                        part.push(p);
                    }
                }
                path.to_path_buf()
            }
        };
        let upper = upper_of(&file);
        // Beside another `.shp` of the name in another case, a part spelled in this one's case
        // (`ROADS.DBF` for `ROADS.SHP`, `ROADS.dbf` for `ROADS.shp`) is taken for this one's.
        let paired = found.iter().any(|(e, f)| *e == "shp" && f.iter().any(|p| *p != file));
        let parts = found
            .into_iter()
            .map(|(ext, mut entries)| {
                entries.sort_by_key(|p| {
                    let e = p.extension().map(|e| e.as_encoded_bytes().to_vec()).unwrap_or_default();
                    let unlike = paired && (if upper { !upper_extension(&e) } else { e != ext.as_bytes() });
                    (unlike, e != ext.as_bytes(), e.clone(), p.as_os_str().as_encoded_bytes().to_vec())
                });
                let found = entries
                    .into_iter()
                    .map(|p| {
                        let file = p.is_file();
                        (p, file)
                    })
                    .collect();
                Part { ext, found }
            })
            .collect();
        Sidecars { upper, file, parts }
    }

    fn part(&self, ext: &str) -> Option<&Part> {
        self.parts.iter().find(|p| p.ext == ext)
    }

    /// The `.shp`, as its folder spells it.
    pub(crate) fn file(&self) -> &Path {
        &self.file
    }

    /// Every entry for `ext`, in the order they are taken in (none when it isn't a Shapefile's).
    pub(crate) fn spellings(&self, ext: &str) -> Vec<&Path> {
        self.part(ext).map(|p| p.found.iter().map(|(p, _)| p.as_path()).collect()).unwrap_or_default()
    }

    /// The part `ext`: the first of its spellings that is a file.
    pub(crate) fn existing(&self, ext: &str) -> Option<&Path> {
        self.part(ext).and_then(Part::taken)
    }

    /// Where the part `ext` is read from and written to: the `.shp` itself; else the part as
    /// [`Sidecars::existing`] finds it, else the name a new one takes, in upper case beside a
    /// `.shp` whose extension is (`ROADS.SHP` gets `ROADS.PRJ`), as
    /// [`crate::io::shapefile_part_urls`] names them, else in lower case.
    pub(crate) fn path(&self, ext: &str) -> PathBuf {
        if ext == "shp" {
            return self.file.clone();
        }
        match self.existing(ext) {
            Some(p) => p.to_path_buf(),
            None => self.file.with_extension(if self.upper { ext.to_ascii_uppercase() } else { ext.to_string() }),
        }
    }

    /// The files of `ext` there besides the one taken: other spellings, on a file system that
    /// tells cases apart.
    pub(crate) fn others(&self, ext: &str) -> Vec<&Path> {
        let Some(part) = self.part(ext) else { return Vec::new() };
        let taken = part.taken();
        part.found.iter().filter(|(p, file)| *file && Some(p.as_path()) != taken).map(|(p, _)| p.as_path()).collect()
    }

    /// Another `.shp` of this name in another case (`roads.SHP` beside `roads.shp`, where case is
    /// told apart): a second Shapefile whose parts can't be told from this one's.
    pub(crate) fn other_shp(&self) -> Option<&Path> {
        self.spellings("shp").into_iter().find(|p| *p != self.file.as_path())
    }

    /// A note for reading beside another `.shp` of the name in another case ([`Sidecars::other_shp`]):
    /// the parts taken for this one's, those spelled in its case, can't be told apart for sure.
    pub(crate) fn other_shp_note(&self) -> Option<String> {
        let other = self.other_shp()?;
        let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let taken: Vec<String> = SHAPEFILE_COMPANIONS.iter().filter_map(|e| self.existing(e)).map(name).collect();
        Some(format!(
            "Shapefile: {}: {} is there too, a Shapefile of the same name in another case; RustGIS reads the parts spelled in this one's case ({}), and can't tell for sure which belong to which",
            self.file.display(),
            name(other),
            if taken.is_empty() { "none".to_string() } else { taken.join(", ") }
        ))
    }

    /// A note naming each part there under more than one spelling (`roads.dbf` and
    /// `roads.DBF`): which one is read and written, and that the others are left as they are.
    pub(crate) fn duplicates_note(&self) -> Option<String> {
        let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let twice: Vec<String> = SHAPEFILE_COMPANIONS
            .iter()
            .filter_map(|ext| {
                let others = self.others(ext);
                let taken = self.existing(ext).filter(|_| !others.is_empty())?;
                Some(format!("{} (not {})", name(taken), others.iter().map(|p| name(p)).collect::<Vec<_>>().join(", ")))
            })
            .collect();
        if twice.is_empty() {
            return None;
        }
        Some(format!(
            "Shapefile: {}: {} {} there under more than one spelling; RustGIS uses {}, and leaves the others as they are",
            self.file.display(),
            twice.len(),
            if twice.len() == 1 { "part is" } else { "parts are" },
            twice.join(", ")
        ))
    }
}

/// The names in `path`'s folder whose stem is `path`'s in any ASCII case, each with whether it
/// is a folder; `None` when the folder can't be listed or holds more than [`LISTING_LIMIT`]
/// entries, which it stops at.
fn list(path: &Path) -> Option<Vec<(std::ffi::OsString, bool)>> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let stem = path.file_stem()?.as_encoded_bytes();
    let mut entries = Vec::new();
    for (i, entry) in std::fs::read_dir(dir).ok()?.enumerate() {
        if i >= LISTING_LIMIT {
            return None;
        }
        let entry = entry.ok()?;
        let name = entry.file_name();
        let bytes = name.as_encoded_bytes();
        let base = bytes.iter().rposition(|&b| b == b'.').and_then(|dot| bytes.get(..dot)).unwrap_or(bytes);
        if base.eq_ignore_ascii_case(stem) {
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            entries.push((name, is_dir));
        }
    }
    Some(entries)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Whether the folder ignores case, as the default file systems of macOS and Windows do.
    pub(crate) fn ignores_case(dir: &Path) -> bool {
        std::fs::write(dir.join("probe.txt"), "").unwrap();
        let ignores = dir.join("PROBE.TXT").exists();
        std::fs::remove_file(dir.join("probe.txt")).unwrap();
        ignores
    }

    #[test]
    fn spellings_are_lower_case_first() {
        assert_eq!(case_spellings("cpg"), ["cpg", "CPG", "CPg", "CpG", "Cpg", "cPG", "cPg", "cpG"]);
        assert_eq!(case_spellings("7z"), ["7z", "7Z"]);
    }

    /// One extension's file is found by its spellings, in that order.
    #[test]
    fn one_file_is_found_by_its_spellings() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.PRJ"), "GEOGCS").unwrap();
        let found = find_one(&dir.path().join("a.tfw"), "prj").unwrap();
        assert_eq!(std::fs::read_to_string(found).unwrap(), "GEOGCS");
        assert_eq!(find_one(&dir.path().join("a.tfw"), "cpg"), None);
    }

    /// On a file system that ignores case, a project naming `roads.shp` finds `ROADS.*` and
    /// keeps their names.
    #[test]
    fn a_stem_in_another_case_finds_the_files_where_case_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        if !ignores_case(dir.path()) {
            return;
        }
        for ext in ["SHP", "SHX", "DBF", "prj"] {
            std::fs::write(dir.path().join(format!("ROADS.{ext}")), ext).unwrap();
        }
        let files = Sidecars::shapefile(&dir.path().join("roads.shp"));
        assert_eq!(files.file(), dir.path().join("ROADS.SHP"));
        assert_eq!(files.path("shp"), dir.path().join("ROADS.SHP"));
        assert_eq!(files.existing("shx"), Some(dir.path().join("ROADS.SHX").as_path()));
        assert_eq!(files.existing("prj"), Some(dir.path().join("ROADS.prj").as_path()));
        assert_eq!(files.path("cpg"), dir.path().join("ROADS.CPG"));
        assert_eq!(files.other_shp(), None);
        assert!(files.duplicates_note().is_none());
    }

    /// Where case is told apart, only the stem as the file has it counts, and another `.shp` of
    /// the name in another case is said.
    #[test]
    fn a_stem_in_another_case_is_another_file_where_case_is_told_apart() {
        let dir = tempfile::tempdir().unwrap();
        if ignores_case(dir.path()) {
            return;
        }
        for name in ["roads.shp", "ROADS.dbf", "roads.SHP"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        let files = Sidecars::shapefile(&dir.path().join("roads.shp"));
        assert_eq!(files.existing("dbf"), None);
        assert_eq!(files.path("dbf"), dir.path().join("roads.dbf"));
        assert_eq!(files.other_shp(), Some(dir.path().join("roads.SHP").as_path()));
    }

    /// Trying each spelling (a folder that can't be listed), every spelling of `ROADS.SHP` that
    /// reaches it (all of them, where case is ignored) is it, not another Shapefile.
    #[cfg(unix)]
    #[test]
    fn trying_each_spelling_never_takes_the_shp_for_another() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        for ext in ["SHP", "SHX", "DBF"] {
            std::fs::write(d.join(format!("ROADS.{ext}")), ext).unwrap();
        }
        let mut perms = std::fs::metadata(d).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o300);
        std::fs::set_permissions(d, perms.clone()).unwrap();
        let listed = std::fs::read_dir(d).is_ok();
        let files = Sidecars::shapefile(&d.join("ROADS.SHP"));
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o700);
        std::fs::set_permissions(d, perms).unwrap();
        if listed {
            return; // Running as a user who reads any folder (root).
        }
        assert_eq!(files.other_shp(), None);
        assert_eq!(files.spellings("shp"), [d.join("ROADS.SHP")]);
        assert_eq!(std::fs::read_to_string(files.existing("shx").unwrap()).unwrap(), "SHX");
        assert_eq!(files.path("dbf").file_name().unwrap().to_string_lossy().to_ascii_lowercase(), "roads.dbf");
    }

    /// Trying each spelling (a folder that can't be listed): a dangling `roads.prj` doesn't hide
    /// the real `roads.PRJ`, and a folder named like a part isn't one.
    #[cfg(unix)]
    #[test]
    fn trying_each_spelling_finds_every_one_there() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::write(d.join("roads.shp"), "").unwrap();
        std::fs::write(d.join("roads.PRJ"), "GEOGCS").unwrap();
        // A file system that ignores case can't hold both: the link replaces nothing there.
        let both = !ignores_case(d);
        if both {
            std::os::unix::fs::symlink(d.join("gone.prj"), d.join("roads.prj")).unwrap();
        }
        std::fs::create_dir(d.join("roads.dbf")).unwrap();
        // Unlisted: the folder can't be read, but its files can.
        let mut perms = std::fs::metadata(d).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o300);
        std::fs::set_permissions(d, perms.clone()).unwrap();
        let listed = std::fs::read_dir(d).is_ok();
        let files = Sidecars::shapefile(&d.join("roads.shp"));
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o700);
        std::fs::set_permissions(d, perms).unwrap();
        if listed {
            return; // Running as a user who reads any folder (root).
        }
        if both {
            assert_eq!(files.spellings("prj"), [d.join("roads.prj"), d.join("roads.PRJ")]);
            assert_eq!(files.existing("prj"), Some(d.join("roads.PRJ").as_path()));
        } else {
            // Without a listing, the spelling can't be told there: the file is found all the same.
            assert_eq!(files.spellings("prj").len(), 1);
            assert_eq!(std::fs::read_to_string(files.existing("prj").unwrap()).unwrap(), "GEOGCS");
        }
        assert_eq!(files.existing("dbf"), None);
        assert!(files.spellings("dbf").is_empty());
    }
}
