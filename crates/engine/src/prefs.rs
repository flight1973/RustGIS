//! The desktop app's preferences, `prefs.json` in the settings folder: one JSON object of
//! string values. The desktop reads and writes it; the command line and Python read it for
//! `--use-saved-secrets` / `use_saved_secrets`.

use rustgis_data::secrets::{Reporting, SecretStore};
use serde_json::{Map, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const FILE: &str = "prefs.json";

static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
#[path = "tests/released_files.rs"]
mod released_files;

/// Reads the desktop preferences. A missing file is normal; unreadable or malformed files
/// return an error without including any preference values in the diagnostic.
pub fn read() -> Result<Map<String, Value>, String> {
    match rustgis_data::dirs::config_dir() {
        Some(dir) => read_at(&dir.join(FILE)),
        None => Ok(Map::new()),
    }
}

fn read_at(path: &Path) -> Result<Map<String, Value>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(error) => return Err(format!("could not read preferences: {error}")),
    };
    parse_preferences(&bytes)
}

/// Parses only bytes successfully read, keeping malformed content distinct from I/O failures.
fn parse_preferences(bytes: &[u8]) -> Result<Map<String, Value>, String> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|error| format!("invalid preferences JSON at line {}, column {}", error.line(), error.column()))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err("preferences must be a JSON object".into()),
    }
}

/// Updates one desktop preference, preserving all others. A malformed file is set aside (see
/// [`set_aside`]) and a new one started: returns where it was put. An unreadable one is never
/// overwritten. The new file is written under a name of its own beside the old one, synced to
/// disk, then renamed over it, so a crash or power cut leaves the old file or the new one, never
/// a partly written one, and two copies of RustGIS saving at once never share a temporary file.
///
/// It reads, changes and writes the file under the lock [`update`] takes, so it never puts back
/// what another copy of RustGIS wrote meanwhile. While another copy holds that lock it writes
/// nothing and returns [`Updated::Busy`]: the value is kept to save, the latest of each key, and
/// [`retry_pending`] writes it later (a later write of the key replaces it).
pub fn write(key: &str, value: &str) -> Result<Updated, String> {
    let dir = rustgis_data::dirs::config_dir().ok_or("the settings folder isn't available")?;
    write_in(&dir.join(FILE), key, value)
}

/// [`write`] of the file at `path` (a test folder in tests). The write and what is left to save
/// change together under [`WRITE_LOCK`], so a value left to save is never older than one written
/// since.
fn write_in(path: &Path, key: &str, value: &str) -> Result<Updated, String> {
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let done = update_file(path, key, |_| Some(value.to_string()));
    let mut pending = PENDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let id = (path.to_path_buf(), key.to_string());
    match done {
        // Left to save: the lock was busy, or what is stored is a newer RustGIS's (refused, and
        // tried again in case it changes).
        Ok(Updated::Busy | Updated::Newer) => {
            pending.insert(id, value.to_string());
        }
        _ => {
            pending.remove(&id);
        }
    }
    done
}

/// Values [`write`] couldn't save yet, by file and key: the latest of each.
static PENDING: Mutex<std::collections::BTreeMap<(PathBuf, String), String>> = Mutex::new(std::collections::BTreeMap::new());

/// The value of `key` left to save, if [`write`] left one: newer than what the file holds, so a
/// read of the setting gives it.
pub fn pending(key: &str) -> Option<String> {
    let dir = rustgis_data::dirs::config_dir()?;
    pending_in(&dir.join(FILE), key)
}

fn pending_in(path: &Path, key: &str) -> Option<String> {
    PENDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&(path.to_path_buf(), key.to_string())).cloned()
}

/// Writes again what [`write`] left to save. `wait` bounds how long it waits for another copy
/// to let go of the lock: zero on the UI thread (it never waits there), a second or two at exit.
/// Returns what each write tried came to (to report), and whether any is still left.
pub fn retry_pending(wait: std::time::Duration) -> (Vec<Result<Option<PathBuf>, String>>, bool) {
    retry_pending_in(None, wait)
}

/// [`retry_pending`], of the file at `only` alone when given (a test folder in tests).
fn retry_pending_in(only: Option<&Path>, wait: std::time::Duration) -> (Vec<Result<Option<PathBuf>, String>>, bool) {
    let todo = || -> Vec<((PathBuf, String), String)> {
        let pending = PENDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        pending.iter().filter(|((path, _), _)| only.is_none_or(|o| o == path)).map(|(k, v)| (k.clone(), v.clone())).collect()
    };
    let mut said = Vec::new();
    // The browser keeps no preferences file: nothing is ever left to write there.
    #[cfg(not(target_arch = "wasm32"))]
    let start = std::time::Instant::now();
    loop {
        let mut busy = false;
        {
            // What is left is read and written under the same lock a write takes, so a value
            // written meanwhile is never put back with an older one.
            let _guard = WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            for ((path, key), value) in todo() {
                let done = update_file(&path, &key, |_| Some(value.clone()));
                match done {
                    Ok(Updated::Busy) => {
                        busy = true;
                        break;
                    }
                    Ok(Updated::Newer) => said.push(Err(newer_notice(&key))),
                    done => {
                        PENDING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&(path, key));
                        said.push(done.map(|d| match d {
                            Updated::Saved(set_aside) => set_aside,
                            _ => None,
                        }));
                    }
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        if busy && start.elapsed() < wait {
            std::thread::sleep(std::time::Duration::from_millis(50));
            continue;
        }
        #[cfg(target_arch = "wasm32")]
        let _ = wait;
        return (said, busy || !todo().is_empty());
    }
}

/// What [`update`] came to.
#[derive(Debug, PartialEq)]
pub enum Updated {
    /// Saved; where a malformed file was set aside, if one was.
    Saved(Option<PathBuf>),
    /// The value given back none: nothing was written.
    Unchanged,
    /// Another copy of RustGIS is saving its preferences: nothing was read or written. Try again
    /// later; this never waits for it.
    Busy,
    /// What is stored under the key isn't text: a newer RustGIS's setting this version can't
    /// read. Never written over (as AGENTS.md's rule for newer settings asks); [`newer_notice`]
    /// says so.
    Newer,
}

/// Says that `key` was saved by a newer RustGIS in a form this version can't read, so a change
/// to it here isn't saved.
pub fn newer_notice(key: &str) -> String {
    format!(
        "the setting \"{key}\" was saved by a newer RustGIS in a form this version can't read: it is kept as it is, and this change isn't saved"
    )
}

/// Updates one desktop preference from its stored value: `value` is given what is stored now
/// (none when nothing is, or the file is malformed) and returns what to save, or none to leave
/// it. Another copy of RustGIS saving meanwhile can't read or write the preferences until this
/// one is written, so a value merged with what is stored keeps that copy's changes; while one
/// is saving, this returns [`Updated::Busy`] at once. Otherwise as [`write`].
pub fn update(key: &str, value: impl FnOnce(Option<&str>) -> Option<String>) -> Result<Updated, String> {
    let dir = rustgis_data::dirs::config_dir().ok_or("the settings folder isn't available")?;
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    update_file(&dir.join(FILE), key, value)
}

/// [`update`] of the file at `path` (a test folder in tests), with its lock.
fn update_file(path: &Path, key: &str, value: impl FnOnce(Option<&str>) -> Option<String>) -> Result<Updated, String> {
    let _held = match lock(path)? {
        Locked::Busy => return Ok(Updated::Busy),
        Locked::Held(held) => Some(held),
        Locked::Unlockable => None,
    };
    write_with(path, key, value, std::fs::read(path), sync_directory)
}

/// [`write`] to the file at `path` (a test folder in tests): returns where a malformed file went.
#[cfg(test)]
fn write_at(path: &Path, key: &str, value: &str) -> Result<Option<PathBuf>, String> {
    write_at_with_sync(path, key, value, sync_directory)
}

/// A lock on the preferences shared by every copy of RustGIS on this computer: held while one
/// reads, changes and writes them, released when dropped (or when the process ends). It is an
/// empty file beside them, never removed, so two copies always lock the same one.
#[cfg(not(target_arch = "wasm32"))]
struct Lock(std::fs::File);

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// The browser keeps no preferences file (and has one copy of RustGIS a page): nothing to lock.
#[cfg(target_arch = "wasm32")]
struct Lock;

/// What trying the preferences lock came to.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Locked {
    Held(Lock),
    /// Another copy of RustGIS holds it.
    Busy,
    /// This folder can't be locked at all (some network folders): saved without it, as before.
    Unlockable,
}

/// Tries the lock once, never waiting: [`Locked::Busy`] while another copy holds it.
#[cfg(not(target_arch = "wasm32"))]
fn lock(path: &Path) -> Result<Locked, String> {
    let lock_path = path.with_extension("json.lock");
    let dir = path.parent().ok_or("the settings folder isn't available")?;
    // Never through a link (the open itself refuses one on Unix), only a regular file, its
    // owner's alone.
    let file = std::fs::create_dir_all(dir)
        .and_then(|_| rustgis_data::io::open_lock_file(&lock_path, true, 0o600))
        .map_err(|error| format!("could not save preferences: couldn't open {}: {error}", lock_path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Locked::Held(Lock(file))),
        Err(std::fs::TryLockError::WouldBlock) => Ok(Locked::Busy),
        Err(std::fs::TryLockError::Error(error)) => {
            log::warn!("preferences saved without a lock: couldn't lock {}: {error}", lock_path.display());
            Ok(Locked::Unlockable)
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn lock(_: &Path) -> Result<Locked, String> {
    Ok(Locked::Held(Lock))
}

/// Separates replacement from its final directory flush; an unconfirmed flush must not claim
/// that the already-visible file was left unchanged. The callback permits a post-rename fault test.
#[cfg(test)]
fn write_at_with_sync(
    path: &Path,
    key: &str,
    value: &str,
    sync: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<Option<PathBuf>, String> {
    write_at_with_bytes(path, key, value, std::fs::read(path), sync)
}

/// Uses one read result for a save. Only successfully read, malformed bytes permit recovery;
/// a transient read failure cannot replace a valid file with a new one containing a single key.
#[cfg(test)]
fn write_at_with_bytes(
    path: &Path,
    key: &str,
    value: &str,
    bytes: std::io::Result<Vec<u8>>,
    sync: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<Option<PathBuf>, String> {
    match write_with(path, key, |_| Some(value.to_string()), bytes, sync)? {
        Updated::Saved(set_aside) => Ok(set_aside),
        Updated::Unchanged | Updated::Busy => Ok(None),
        Updated::Newer => Err(newer_notice(key)),
    }
}

/// [`write_at_with_bytes`] with the value made from the one stored under `key` in `bytes` (none
/// to leave it as it is).
fn write_with(
    path: &Path,
    key: &str,
    value: impl FnOnce(Option<&str>) -> Option<String>,
    bytes: std::io::Result<Vec<u8>>,
    sync: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<Updated, String> {
    let (mut map, malformed) = match bytes {
        Ok(bytes) => match parse_preferences(&bytes) {
            Ok(map) => (map, None),
            Err(_) => (Map::new(), Some(bytes)),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Map::new(), None),
        Err(error) => return Err(format!("could not read preferences: {error}")),
    };
    // A newer RustGIS's setting this version can't read (not text) is never written over.
    let stored = match map.get(key) {
        Some(Value::String(text)) => Some(text.as_str()),
        Some(_) => return Ok(Updated::Newer),
        None => None,
    };
    let Some(value) = value(stored) else { return Ok(Updated::Unchanged) };
    let set_aside = match malformed {
        Some(bytes) => Some(set_aside(path, &bytes)?),
        None => None,
    };
    map.insert(key.to_string(), Value::String(value));
    let text = serde_json::to_vec(&map).map_err(|_| "could not serialize preferences")?;
    let dir = path.parent().ok_or("the settings folder isn't available")?;
    std::fs::create_dir_all(dir).and_then(|_| rustgis_data::io::replace_atomic(path, &text)).map_err(|error| match &set_aside {
        Some(backup) => format!("could not save preferences: {error}; malformed preferences were kept at {}", backup.display()),
        None => format!("could not save preferences: {error}"),
    })?;
    sync(path).map_err(|error| match &set_aside {
        Some(backup) => format!(
            "preferences were replaced, but their durability could not be confirmed: {error}; malformed preferences were kept at {}",
            backup.display()
        ),
        None => format!("preferences were replaced, but their durability could not be confirmed: {error}"),
    })?;
    Ok(Updated::Saved(set_aside))
}

/// How many malformed preference files are kept: `prefs.json.bad`, then `prefs.json.bad2` to
/// `prefs.json.bad9`. An identical existing copy is reused; when all contain different bytes,
/// saving refuses to replace any recovery copy or the malformed current file.
const KEPT_BAD: u32 = 9;

/// Copies the exact bytes that failed parsing to an exclusively created `.bad` file, synced before saving
/// replaces the original. Concurrent callers cannot overwrite each other's recovery files.
/// Failed copies remain reserved: another process may already have verified and reused one.
/// A copy or sync failure stops replacement and reports the possibly incomplete recovery path.
/// Retries reuse an identical regular recovery file, without spending another recovery slot.
fn set_aside(path: &Path, bytes: &[u8]) -> Result<PathBuf, String> {
    set_aside_with_sync(path, bytes, |backup, target| backup.sync_all().and_then(|_| sync_directory(target)))
}

/// Copies into an exclusive recovery name and confirms it before returning. A failing creator
/// never deletes the name, because another writer can independently sync and reuse its bytes.
fn set_aside_with_sync(path: &Path, bytes: &[u8], sync: impl Fn(&std::fs::File, &Path) -> std::io::Result<()>) -> Result<PathBuf, String> {
    let name = |n: u32| path.with_extension(if n == 1 { "json.bad".to_string() } else { format!("json.bad{n}") });
    let retained = |error: std::io::Error, target: &Path| {
        format!(
            "could not confirm recovery of malformed preferences: {error}; recovery copy retained at {} and may be incomplete",
            target.display()
        )
    };
    let confirm =
        |copy: &std::fs::File, target: &Path| sync(copy, target).map(|()| target.to_path_buf()).map_err(|error| retained(error, target));
    let targets: Vec<PathBuf> = (1..=KEPT_BAD).map(name).collect();
    for target in &targets {
        if let Ok(Some(copy)) = same_contents(bytes, target) {
            return confirm(&copy, target);
        }
    }
    for target in targets {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut backup = match options.open(&target) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // Another process may have completed this copy after the first scan.
                if let Ok(Some(copy)) = same_contents(bytes, &target) {
                    return confirm(&copy, &target);
                }
                continue;
            }
            Err(error) => return Err(format!("could not preserve malformed preferences: {error}")),
        };
        if let Err(error) = backup.write_all(bytes) {
            return Err(retained(error, &target));
        }
        return confirm(&backup, &target);
    }
    Err(format!(
        "all {KEPT_BAD} preference recovery files are occupied; repair or move one before saving settings; the malformed preferences were left unchanged at {}",
        path.display()
    ))
}

/// Makes a created recovery name or replaced preference name durable on Unix before success.
/// Other platforms keep the synced-file guarantee; opening directories is not portable there.
fn sync_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        std::fs::File::open(dir)?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Returns the open copy when its bytes match, so the caller can confirm its durability.
/// Symlink paths and read errors do not establish a match; sync failures remain distinct.
fn same_contents(source: &[u8], target: &Path) -> std::io::Result<Option<std::fs::File>> {
    if !std::fs::symlink_metadata(target)?.file_type().is_file() {
        return Ok(None);
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    // Linux can sync a read-only descriptor; retain the other platforms' flush access.
    #[cfg(not(target_os = "linux"))]
    options.write(true);
    let mut copy = options.open(target)?;
    if source.len() as u64 != copy.metadata()?.len() {
        return Ok(None);
    }
    let mut right = [0u8; 8192];
    for left in source.chunks(right.len()) {
        copy.read_exact(&mut right[..left.len()])?;
        if left != &right[..left.len()] {
            return Ok(None);
        }
    }
    if copy.read(&mut right[..1])? != 0 {
        return Ok(None);
    }
    Ok(Some(copy))
}

/// Opts a native caller into the desktop's saved connections and lazy credential store, which
/// also keeps the Earthdata Login token remembered on this computer.
/// Constructing the store and loading connection definitions never reads a credential.
/// Both preference failures and once-only store failures use the caller's diagnostic sink.
/// Returns the same preference snapshot for assistant provider/key selection.
pub fn opt_in(
    s: &mut crate::Session,
    store: impl SecretStore + 'static,
    warn: impl Fn(String) + Send + Sync + 'static,
) -> Map<String, Value> {
    let prefs = read().unwrap_or_else(|error| {
        warn(format!("{error}; going on without saved preferences"));
        Map::new()
    });
    let store = Reporting::new(store, move |error| {
        warn(format!("saved secrets can't be used ({error}); going on without them"));
    });
    let store: Arc<dyn SecretStore> = Arc::new(store);
    crate::render::rustgis_tiles::earthdata::set_secret_store(Some(store.clone()));
    crate::render::rustgis_tiles::copernicus::set_secret_store(Some(store.clone()));
    s.saved_secrets_opted_in = true;
    use_saved_secrets(s, &prefs, store);
    prefs
}

/// Gives `s` the desktop's saved connections and their keychain. Connection commands use the
/// desktop's matching rules (same id, kind and address or endpoint; typed passwords and keys
/// win), with a database client's environment password (`PGPASSWORD`…) before the saved one.
/// Changes to the connections aren't written back. Missing preferences never import legacy
/// connections.
pub fn use_saved_secrets(s: &mut crate::Session, prefs: &Map<String, Value>, store: Arc<dyn SecretStore>) {
    if let Some(text) = prefs.get(crate::connections::PREF).and_then(Value::as_str) {
        s.load_connections(text);
    }
    s.database_logins.set_store(Some(store));
    s.database_logins.prefer_environment_passwords();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two copies of RustGIS read the same saved data sources and save their own changes: each
    /// keeps the other's, and a source one forgot stays forgotten.
    #[test]
    fn two_copies_keep_each_others_data_sources() {
        use crate::sources::{Source, Sources};
        let dir = std::env::temp_dir().join(format!("rustgis-preferences-two-copies-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        let file = |address: &str| Source { kind: "file".into(), address: address.into(), ..Source::default() };
        let mut first = Sources::default();
        first.keep("favourites", file("/old.csv")).unwrap();
        first.keep("favourites", file("/gone.csv")).unwrap();
        write_at(&path, "data_sources", &first.to_json()).unwrap();
        // As the desktop saves: read again, this copy's changes laid over what is stored, written.
        let save = |s: &mut Sources| {
            let mut next = None;
            let done = update_file(&path, "data_sources", |stored| {
                let mut n = s.clone();
                n.rebase(stored, None).ok()?;
                let text = n.to_json();
                next = Some(n);
                Some(text)
            })
            .unwrap();
            assert!(matches!(done, Updated::Saved(_)), "{done:?}");
            *s = next.unwrap();
        };
        let stored = || Sources::from_json(read_at(&path).unwrap().get("data_sources").and_then(Value::as_str).unwrap());
        let favorites = |s: &Sources| s.favourites.iter().map(|f| f.address.clone()).collect::<Vec<_>>();
        let (mut a, mut b) = (stored(), stored());
        a.keep("favourites", file("/a.csv")).unwrap();
        save(&mut a);
        b.keep("favourites", file("/b.csv")).unwrap();
        save(&mut b);
        assert_eq!(favorites(&stored()), ["/old.csv", "/gone.csv", "/a.csv", "/b.csv"]);
        // The copy that saved last lists the other's too.
        assert_eq!(favorites(&b), favorites(&stored()));
        // One forgets a source and saves after the other did: it stays forgotten.
        assert!(a.forget("favourites", "file", "/gone.csv").unwrap());
        b.note(file("/b-recent.csv"));
        save(&mut b);
        save(&mut a);
        assert_eq!(favorites(&stored()), ["/old.csv", "/a.csv", "/b.csv"]);
        assert_eq!(stored().recent.iter().map(|r| r.address.as_str()).collect::<Vec<_>>(), ["/b-recent.csv"]);
        // The other, saving again with what it read before, doesn't bring it back.
        b.keep("saved", file("/b-saved.csv")).unwrap();
        save(&mut b);
        assert_eq!(favorites(&stored()), ["/old.csv", "/a.csv", "/b.csv"]);
        assert_eq!(stored().saved.len(), 1);
        // Forgotten and kept again here, a source is listed again.
        assert!(a.forget("favourites", "file", "/a.csv").unwrap());
        a.keep("favourites", file("/a.csv")).unwrap();
        save(&mut a);
        assert!(favorites(&stored()).contains(&"/a.csv".to_string()));
        // With nothing stored (the preferences removed, or reset), a save writes this copy's whole
        // list, not just what it changed since.
        for gone in [None, Some("")] {
            match gone {
                None => std::fs::remove_file(&path).unwrap(),
                Some(empty) => write_at(&path, "data_sources", empty).map(drop).unwrap(),
            }
            let all = favorites(&a);
            a.keep("favourites", file("/after.csv")).unwrap();
            save(&mut a);
            assert_eq!(favorites(&stored()), [all, vec!["/after.csv".to_string()]].concat());
            assert!(a.forget("favourites", "file", "/after.csv").unwrap());
            save(&mut a);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Every preference write goes through the lock: while another copy holds it, a write is
    /// kept (the latest of its key), writes nothing, and is written later over what that copy
    /// saved meanwhile, never putting its old value back; at exit the retry waits a little.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_write_while_another_copy_saves_is_kept_and_written_later() {
        let dir = std::env::temp_dir().join(format!("rustgis-preferences-pending-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        write_at(&path, "data_sources", "old").unwrap();
        let held = lock(&path).unwrap();
        assert_eq!(write_in(&path, "theme", "light").unwrap(), Updated::Busy);
        assert_eq!(write_in(&path, "theme", "dark").unwrap(), Updated::Busy);
        assert!(read_at(&path).unwrap().get("theme").is_none(), "nothing written while it is held");
        let (said, left) = retry_pending_in(Some(&path), std::time::Duration::ZERO);
        assert!(said.is_empty() && left);
        // The other copy saves its own while it holds the lock.
        write_at(&path, "data_sources", "theirs").unwrap();
        drop(held);
        let (said, left) = retry_pending_in(Some(&path), std::time::Duration::ZERO);
        assert_eq!((said.len(), left), (1, false), "{said:?}");
        let map = read_at(&path).unwrap();
        assert_eq!(map.get("theme").and_then(Value::as_str), Some("dark"), "the latest value");
        assert_eq!(map.get("data_sources").and_then(Value::as_str), Some("theirs"), "the other copy's save stays");
        // At exit the retry waits (a little) for the other copy to let go.
        let held = lock(&path).unwrap();
        assert_eq!(write_in(&path, "theme", "exit").unwrap(), Updated::Busy);
        let other = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200)); // test-wait: another copy holding the lock briefly
            drop(held);
        });
        let (said, left) = retry_pending_in(Some(&path), std::time::Duration::from_millis(1500));
        other.join().unwrap();
        assert_eq!((said.len(), left), (1, false), "{said:?}");
        assert_eq!(read_at(&path).unwrap().get("theme").and_then(Value::as_str), Some("exit"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A value left to save is what the setting reads as until it is written, never written
    /// over a newer one saved since; and a setting a newer RustGIS stored as something other than
    /// text is never written over, the change kept to save and said.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn pending_values_stay_newest_and_newer_settings_are_kept() {
        let dir = std::env::temp_dir().join(format!("rustgis-preferences-pending-order-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        let held = lock(&path).unwrap();
        assert_eq!(write_in(&path, "theme", "old").unwrap(), Updated::Busy);
        assert_eq!(pending_in(&path, "theme").as_deref(), Some("old"), "read as the setting until written");
        drop(held);
        // A newer value saved meanwhile: the old one is no longer left, so a retry never puts it back.
        assert_eq!(write_in(&path, "theme", "new").unwrap(), Updated::Saved(None));
        assert_eq!(pending_in(&path, "theme"), None);
        let (said, left) = retry_pending_in(Some(&path), std::time::Duration::ZERO);
        assert!(said.is_empty() && !left, "{said:?}");
        assert_eq!(read_at(&path).unwrap().get("theme").and_then(Value::as_str), Some("new"));

        // A newer RustGIS stored data_sources as an object: never written over.
        let newer = r#"{"data_sources":{"version":2,"lists":[]},"theme":"new"}"#;
        std::fs::write(&path, newer).unwrap();
        assert_eq!(update_file(&path, "data_sources", |_| Some("{}".into())).unwrap(), Updated::Newer);
        assert_eq!(write_in(&path, "data_sources", "{}").unwrap(), Updated::Newer);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);
        assert_eq!(pending_in(&path, "data_sources").as_deref(), Some("{}"), "kept to save");
        let (said, left) = retry_pending_in(Some(&path), std::time::Duration::ZERO);
        assert!(left && matches!(said.as_slice(), [Err(e)] if e.contains("newer RustGIS")), "{said:?}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), newer);
        // Other settings are still written beside it.
        assert_eq!(write_in(&path, "theme", "dark").unwrap(), Updated::Saved(None));
        assert_eq!(read_at(&path).unwrap().get("data_sources"), Some(&serde_json::json!({"version": 2, "lists": []})));
        PENDING.lock().unwrap().retain(|(p, _), _| p != &path);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The preferences lock is shared by every copy of RustGIS: while one holds it, another's
    /// update returns at once, reading and writing nothing, and saves once it is let go.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_held_preferences_lock_never_waits() {
        let dir = std::env::temp_dir().join(format!("rustgis-preferences-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        let held = lock(&path).unwrap();
        assert!(matches!(held, Locked::Held(_)));
        assert!(matches!(lock(&path).unwrap(), Locked::Busy));
        let started = std::time::Instant::now();
        let busy = update_file(&path, "k", |_| panic!("nothing is read while another copy saves")).unwrap();
        assert_eq!(busy, Updated::Busy);
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
        assert!(!path.exists());
        drop(held);
        assert_eq!(update_file(&path, "k", |_| Some("v".into())).unwrap(), Updated::Saved(None));
        assert_eq!(read_at(&path).unwrap().get("k").and_then(Value::as_str), Some("v"));
        // A value given back as none leaves the preference as it was.
        assert_eq!(update_file(&path, "k", |_| None).unwrap(), Updated::Unchanged);
        assert_eq!(read_at(&path).unwrap().get("k").and_then(Value::as_str), Some("v"));
        // A link in the lock's place is refused, never followed.
        #[cfg(unix)]
        {
            let lock_path = path.with_extension("json.lock");
            std::fs::remove_file(&lock_path).unwrap();
            std::os::unix::fs::symlink(dir.join("elsewhere"), &lock_path).unwrap();
            assert!(lock(&path).is_err());
            assert!(!dir.join("elsewhere").exists());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Recovery preserves the malformed snapshot, even if the path now contains another save.
    #[test]
    fn recovery_keeps_the_bytes_that_failed_parsing() {
        let dir = std::env::temp_dir().join(format!("rustgis-preferences-snapshot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        let malformed = b"malformed original";
        assert!(parse_preferences(malformed).is_err());
        let current = br#"{"theme":"dark","other":"kept"}"#;
        std::fs::write(&path, current).unwrap();
        let backup = set_aside(&path, malformed).unwrap();
        assert_eq!(std::fs::read(&backup).unwrap(), malformed);
        assert_eq!(std::fs::read(&path).unwrap(), current);
        assert_eq!(set_aside(&path, malformed).unwrap(), backup);
        assert!(!path.with_extension("json.bad2").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A failed read must not be reclassified as malformed when the file is readable again.
    #[test]
    fn transient_read_failure_preserves_valid_preferences() {
        let dir = std::env::temp_dir().join(format!("rustgis-preferences-read-retry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        let original = br#"{"theme":"dark","other":"kept"}"#;
        std::fs::write(&path, original).unwrap();
        let error = write_at_with_bytes(
            &path,
            "theme",
            "light",
            Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "transient read failure")),
            sync_directory,
        )
        .unwrap_err();
        assert!(error.contains("could not read preferences") && error.contains("transient read failure"), "{error}");
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(!path.with_extension("json.bad").exists());
        assert_eq!(write_at(&path, "theme", "light").unwrap(), None);
        let saved = read_at(&path).unwrap();
        assert_eq!(saved.get("theme").and_then(Value::as_str), Some("light"));
        assert_eq!(saved.get("other").and_then(Value::as_str), Some("kept"));
        assert!(!path.with_extension("json.bad").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A verified copy whose durability cannot be confirmed must not spend another recovery slot.
    #[test]
    fn repeated_recovery_sync_failures_keep_one_slot() {
        let dir = std::env::temp_dir().join(format!("rustgis-preferences-sync-retry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        std::fs::write(&path, "malformed original").unwrap();
        for _ in 0..2 {
            let error =
                set_aside_with_sync(&path, b"malformed original", |_, _| Err(std::io::Error::other("directory sync failed"))).unwrap_err();
            assert!(error.contains("directory sync failed") && error.contains("recovery copy retained"), "{error}");
            assert!(!path.with_extension("json.bad2").exists());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "malformed original");
        }
        assert_eq!(set_aside(&path, &std::fs::read(&path).unwrap()).unwrap(), path.with_extension("json.bad"));
        assert!(!path.with_extension("json.bad2").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A second writer can finish a save while the recovery creator is still confirming it.
    /// A later creator error must not remove the recovery copy that the successful writer used.
    #[test]
    fn a_creator_sync_error_keeps_a_recovery_copy_another_writer_reused() {
        let dir = std::env::temp_dir().join(format!("rustgis-preferences-reused-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        std::fs::write(&path, "malformed original").unwrap();
        let error = set_aside_with_sync(&path, b"malformed original", |_, target| {
            let reused = write_at(&path, "theme", "dark").map_err(std::io::Error::other)?;
            assert_eq!(reused.as_deref(), Some(target));
            Err(std::io::Error::other("creator sync failed"))
        })
        .unwrap_err();
        assert!(error.contains("recovery copy retained") && error.contains("creator sync failed"), "{error}");
        assert_eq!(read_at(&path).unwrap().get("theme").and_then(Value::as_str), Some("dark"));
        assert_eq!(std::fs::read_to_string(path.with_extension("json.bad")).unwrap(), "malformed original");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn full_recovery_slots_leave_the_malformed_file_and_every_backup_unchanged() {
        let dir = std::env::temp_dir().join(format!("rustgis-preferences-full-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        std::fs::write(&path, "broken preferences").unwrap();
        let mut kept = Vec::new();
        for n in 0..KEPT_BAD {
            let text = format!("broken preferences {n}");
            std::fs::write(&path, &text).unwrap();
            kept.push((set_aside(&path, &std::fs::read(&path).unwrap()).unwrap(), text));
        }
        std::fs::write(&path, "broken preferences").unwrap();
        assert!(write_at(&path, "theme", "dark").unwrap_err().contains("left unchanged"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken preferences");
        for (backup, text) in kept {
            assert_eq!(std::fs::read_to_string(backup).unwrap(), text);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn preference_updates_preserve_other_values_and_set_malformed_files_aside() {
        let dir = std::env::temp_dir().join(format!("rustgis-preferences-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        assert_eq!(read_at(&path).unwrap(), Map::new());
        assert_eq!(write_at(&path, "connections", "saved definitions").unwrap(), None);
        assert_eq!(write_at(&path, "theme", "dark").unwrap(), None);
        let map = read_at(&path).unwrap();
        assert_eq!(map.get("connections"), Some(&Value::String("saved definitions".into())));
        assert_eq!(map.get("theme"), Some(&Value::String("dark".into())));
        // A failed final flush leaves the replacement visible, with an honest durability error.
        let error = write_at_with_sync(&path, "theme", "light", |_| {
            Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "directory sync unavailable"))
        })
        .unwrap_err();
        assert!(error.contains("preferences were replaced") && error.contains("durability could not be confirmed"), "{error}");
        assert_eq!(read_at(&path).unwrap().get("theme"), Some(&Value::String("light".into())));
        let broken = r#"{"sensitive-pref":"prefs-sentinel", broken}"#;
        std::fs::write(&path, broken).unwrap();
        let error = read_at(&path).unwrap_err();
        assert!(!error.contains("prefs-sentinel"));
        assert_eq!(write_at(&path, "theme", "light").unwrap(), Some(dir.join("prefs.json.bad")));
        assert_eq!(std::fs::read_to_string(dir.join("prefs.json.bad")).unwrap(), broken);
        assert_eq!(read_at(&path).unwrap().get("theme"), Some(&Value::String("light".into())));
        // A file cut short (as a power cut can leave one) is set aside beside the first, not over it.
        std::fs::write(&path, "").unwrap();
        assert_eq!(write_at(&path, "theme", "dark").unwrap(), Some(dir.join("prefs.json.bad2")));
        assert_eq!(std::fs::read_to_string(dir.join("prefs.json.bad")).unwrap(), broken);
        assert_eq!(write_at(&path, "theme", "light").unwrap(), None);
        // Failed replacements leave the malformed original: repeated preservation reuses its copy.
        // Protecting the existing copy on Linux must not spend more recovery slots.
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.join("prefs.json.bad"), std::fs::Permissions::from_mode(0o444)).unwrap();
        }
        std::fs::write(&path, broken).unwrap();
        for _ in 0..=KEPT_BAD {
            assert_eq!(set_aside(&path, &std::fs::read(&path).unwrap()).unwrap(), dir.join("prefs.json.bad"));
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
        assert_eq!(write_at(&path, "theme", "light").unwrap(), Some(dir.join("prefs.json.bad")));
        // Invalid UTF-8 is malformed JSON too; recovery preserves the exact original bytes.
        let invalid = [0xff, 0xfe];
        std::fs::write(&path, invalid).unwrap();
        assert!(read_at(&path).is_err());
        assert_eq!(write_at(&path, "theme", "light").unwrap(), Some(dir.join("prefs.json.bad3")));
        assert_eq!(std::fs::read(dir.join("prefs.json.bad3")).unwrap(), invalid);
        // No temporary file is left behind.
        let mut names: Vec<String> =
            std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["prefs.json", "prefs.json.bad", "prefs.json.bad2", "prefs.json.bad3"]);
        std::fs::write(&path, r#"["prefs-sentinel"]"#).unwrap();
        assert!(!read_at(&path).unwrap_err().contains("prefs-sentinel"));
        assert!(read_at(&dir).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
