//! The Data Sources pane: favourites, recent sources, folders, named connections and a library
//! of public sources. Rows add a source or open its browser through the existing commands.
//! Sources and connections are kept with the app's preferences, never with secrets.

use egui::{RichText, Sense, Ui, vec2};
use rustgis_engine::connections::{Connection, DATABASE_KINDS};
use rustgis_engine::sources::{Source, library};
use serde_json::{Value, json};

use crate::theme::{Tokens, semibold};
use crate::{RustGisApp, icons};

/// The preference the sources are kept under.
pub const PREF: &str = "data_sources";

/// Preference loading and generation-keyed snapshots for the pane.
#[derive(Default)]
pub struct State {
    loaded: bool,
    /// The sources as last shown, and their generation.
    shown: Option<(u64, std::sync::Arc<rustgis_engine::sources::Sources>)>,
    connections: Option<(u64, std::sync::Arc<Vec<Connection>>)>,
    /// The kinds of notice said of the stored sources, with the count each was said at: reading
    /// them again at a save says a kind again only when more is affected than was said.
    said: Vec<(&'static str, usize)>,
    /// Whether a save refused to write over unreadable stored sources has been said since the
    /// last save that happened.
    refusal_said: bool,
    /// What this session last wrote: a save that finds it stored has nothing to lay its changes
    /// over.
    last_written: Option<String>,
    /// When a data sources save that didn't happen (the preferences busy, unreadable or
    /// failing) is tried again.
    retry: Retry,
    /// Since when another copy of the app has kept the preferences busy.
    busy_since: Option<web_time::Instant>,
    /// When the other preferences left to save (`retry_prefs`) are tried again.
    prefs_retry: Retry,
}

/// When a save that didn't happen is tried again, so a frame doesn't retry it each time:
/// shared by the data sources and the app's other preferences.
#[derive(Default)]
struct Retry {
    at: Option<web_time::Instant>,
    /// Failed tries in a row.
    failures: u32,
}

impl Retry {
    fn due(&self, now: web_time::Instant) -> bool {
        self.at.is_none_or(|at| now >= at)
    }
    /// Tried again after `delay`, whatever failed before (another copy busy).
    fn after(&mut self, now: web_time::Instant, delay: std::time::Duration) {
        self.at = Some(now + delay);
    }
    /// One more failure in a row: tried again after `first`, doubling each time up to `most`.
    fn failed(&mut self, now: web_time::Instant, first: std::time::Duration, most: std::time::Duration) {
        self.failures = self.failures.saturating_add(1);
        self.at = Some(now + first.saturating_mul(1 << self.failures.saturating_sub(1).min(16)).min(most));
    }
    fn done(&mut self) {
        *self = Retry::default();
    }
    /// How long until it is due, if it waits.
    fn wait(&self, now: web_time::Instant) -> Option<std::time::Duration> {
        self.at.map(|at| at.saturating_duration_since(now))
    }
}

/// How soon a save is tried again while another copy of the app saves its preferences.
const BUSY_RETRY: std::time::Duration = std::time::Duration::from_millis(250);
/// How long another copy may keep them busy before that is said.
const BUSY_SAY: std::time::Duration = std::time::Duration::from_secs(10);
/// A failed data sources save is tried again after a second, doubling up to a minute.
const FAILED_RETRY: (std::time::Duration, std::time::Duration) = (std::time::Duration::from_secs(1), std::time::Duration::from_secs(60));

/// The sources to show this frame: the last snapshot unless they changed since.
fn snapshot(app: &mut RustGisApp) -> std::sync::Arc<rustgis_engine::sources::Sources> {
    let generation = app.session.sources.generation;
    match &app.ui.sources.shown {
        Some((g, s)) if *g == generation => s.clone(),
        _ => {
            let s = std::sync::Arc::new(app.session.sources.clone());
            app.ui.sources.shown = Some((generation, s.clone()));
            s
        }
    }
}

pub(crate) fn connections_snapshot(app: &mut RustGisApp) -> std::sync::Arc<Vec<Connection>> {
    let generation = app.session.connections.generation;
    match &app.ui.sources.connections {
        Some((g, list)) if *g == generation => list.clone(),
        _ => {
            let list = std::sync::Arc::new(app.session.connections.connections.clone());
            app.ui.sources.connections = Some((generation, list.clone()));
            list
        }
    }
}

/// Reads the sources kept with the preferences, once.
pub fn load(app: &mut RustGisApp) {
    if app.ui.sources.loaded {
        return;
    }
    app.ui.sources.loaded = true;
    // A change made before this first read (a source just added, which is what usually brings
    // it on) is still to be saved, with the stored lists.
    let changed = app.session.sources.generation != app.ui.sources_saved;
    if let Some(text) = app.services.load_pref.as_ref().and_then(|load| load(PREF)) {
        // What this session already has (added, saved or made favourite before the first read)
        // stays; the stored lists are added after it. What of them isn't used here is said.
        let stored = rustgis_engine::sources::Sources::from_json(&text);
        app.session.sources.merge(stored);
        // After the merge: sources it moved past a list's limit are said too.
        say_unread(app);
    }
    if !changed {
        app.ui.sources_saved = app.session.sources.generation;
    }
}

/// Says what of the stored sources isn't used here: each kind once, and again when more of it
/// is affected than was said (another window's newer sources, more rows unreadable).
fn say_unread(app: &mut RustGisApp) {
    for (kind, count, said) in app.session.sources.unread_by_kind() {
        say_more(app, kind, count, said);
    }
}

/// Says `said` unless its kind was said at `count` or more already.
fn say_more(app: &mut RustGisApp, kind: &'static str, count: usize, said: String) {
    let known = &mut app.ui.sources.said;
    match known.iter_mut().find(|(k, _)| *k == kind) {
        Some((_, at)) if *at >= count => return,
        Some((_, at)) => *at = count,
        None => known.push((kind, count)),
    }
    app.session.warn(said);
}

/// Keeps the sources with the preferences when they changed. Where another copy of the app
/// keeps them too (the desktop), what is stored is read again and this session's changes laid
/// over it ([`Sources::rebase`](rustgis_engine::sources::Sources::rebase)), so neither copy
/// writes over the other's. A save that doesn't happen (another copy saving, the stored
/// sources unreadable, the file failing) leaves the changes to save, tried again later
/// ([`retry_in`]); it never waits.
pub fn save(app: &mut RustGisApp) {
    if !app.ui.sources.loaded {
        load(app);
    }
    if app.session.sources.generation == app.ui.sources_saved {
        return;
    }
    if app.services.update_pref.is_none() {
        app.ui.sources_saved = app.session.sources.generation;
        if let Some(save) = &app.services.save_pref {
            save(PREF, &app.session.sources.to_json());
        }
        return;
    }
    if !app.ui.sources.retry.due(web_time::Instant::now()) {
        return;
    }
    attempt(app);
}

/// One try at saving the data sources through `update_pref`: what it came to.
fn attempt(app: &mut RustGisApp) -> Option<crate::PrefUpdate> {
    let update = app.services.update_pref.as_ref()?;
    let now = web_time::Instant::now();
    let (mut next, mut unreadable) = (None, None);
    let (sources, last) = (&app.session.sources, app.ui.sources.last_written.as_deref());
    let outcome = update(PREF, &mut |stored| {
        let mut rebased = sources.clone();
        match rebased.rebase(stored, last) {
            Ok(()) => {
                let text = rebased.to_json();
                next = Some((rebased, text.clone()));
                Some(text)
            }
            // Never written over: the changes wait until it can be read (or is removed).
            Err(why) => {
                unreadable = Some(why);
                None
            }
        }
    });
    let state = &mut app.ui.sources;
    match (outcome, next) {
        (crate::PrefUpdate::Saved, Some((sources, text))) => {
            state.retry.done();
            (state.busy_since, state.refusal_said, state.last_written) = (None, false, Some(text));
            state.said.retain(|(kind, _)| *kind != "busy");
            app.session.sources = sources;
            app.ui.sources_saved = app.session.sources.generation;
            say_unread(app);
        }
        (crate::PrefUpdate::Busy, _) => {
            let since = *state.busy_since.get_or_insert(now);
            state.retry.after(now, BUSY_RETRY);
            if now.duration_since(since) >= BUSY_SAY {
                let said =
                    crate::tl!("The data sources aren't saved yet: another copy of RustGIS keeps its preferences busy. Trying again.");
                say_more(app, "busy", 1, said.to_string());
            }
        }
        _ => {
            state.busy_since = None;
            state.retry.failed(now, FAILED_RETRY.0, FAILED_RETRY.1);
            // Said when a save refuses, though the same was said when the sources were read.
            if let Some(why) = unreadable
                && !state.refusal_said
            {
                state.refusal_said = true;
                app.session.warn(why);
            }
        }
    }
    Some(outcome)
}

/// As the app closes: data sources left to save are tried once more whatever the retry delay,
/// trying again for up to `wait` while another copy of the app keeps the preferences busy
/// (closing isn't the path that must answer at once). What still can't be saved is logged and
/// said in this session's messages; with the window closing, only the log keeps it for later
/// (the next start doesn't say it).
pub fn save_at_exit(app: &mut RustGisApp, wait: std::time::Duration) {
    if !app.ui.sources.loaded || app.session.sources.generation == app.ui.sources_saved || app.services.update_pref.is_none() {
        return;
    }
    let start = web_time::Instant::now();
    loop {
        let outcome = attempt(app);
        if app.session.sources.generation == app.ui.sources_saved {
            return;
        }
        let busy = outcome == Some(crate::PrefUpdate::Busy);
        if !busy || start.elapsed() >= wait {
            let why = if busy { "another copy of RustGIS kept its preferences busy" } else { "they couldn't be written" };
            log::warn!("the data sources changed in this session weren't saved: {why}");
            let said =
                if busy { crate::tl!("another copy of RustGIS kept its preferences busy") } else { crate::tl!("they couldn't be written") };
            app.session.warn(crate::i18n::fmt(
                "The data sources changed in this session weren't saved as RustGIS closed: {why}.",
                &[("why", said)],
            ));
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// How soon a data sources save left to do is tried again, if one is: the frame that tries it
/// is asked for.
pub fn retry_in(app: &RustGisApp) -> Option<std::time::Duration> {
    if app.session.sources.generation == app.ui.sources_saved {
        return None;
    }
    app.ui.sources.retry.wait(web_time::Instant::now())
}

/// Saves again the app's other preferences left to save while another copy saved its own
/// (`Services::retry_prefs`), when due: tried at once, then after a quarter second, doubling up
/// to five seconds while they stay left, so a long-held lock doesn't cost every frame. How soon
/// it is tried again, if anything is left.
pub fn retry_prefs(app: &mut RustGisApp) -> Option<std::time::Duration> {
    let retry = app.services.retry_prefs.as_ref()?;
    let now = web_time::Instant::now();
    let state = &mut app.ui.sources.prefs_retry;
    if !state.due(now) {
        return state.wait(now);
    }
    if retry() {
        state.failed(now, std::time::Duration::from_millis(250), std::time::Duration::from_secs(5));
        state.wait(now)
    } else {
        state.done();
        None
    }
}

/// Loads connections before commands can run, importing legacy saved endpoints only once.
pub fn load_connections(app: &mut RustGisApp) {
    use rustgis_engine::{connections::Connections, sources::Sources};
    let Some(load) = &app.services.load_pref else { return };
    if let Some(text) = load(rustgis_engine::connections::PREF) {
        app.session.load_connections(&text);
    } else {
        let imported =
            Connections::import(&Sources::from_json(&load(PREF).unwrap_or_default()), &load(crate::s3ui::SAVED).unwrap_or_default());
        // Even an empty import is saved, so removing everything cannot re-import old entries.
        if let Some(save) = &app.services.save_pref {
            save(rustgis_engine::connections::PREF, &imported.to_json());
        }
        app.session.connections = imported;
    }
    app.ui.connections_saved = app.session.connections.generation;
}

/// Keeps command and assistant changes with the app's preferences, never the project.
pub fn save_connections(app: &mut RustGisApp) {
    if app.session.connections.generation == app.ui.connections_saved {
        return;
    }
    app.ui.connections_saved = app.session.connections.generation;
    if let Some(save) = &app.services.save_pref {
        save(rustgis_engine::connections::PREF, &app.session.connections.to_json());
    }
}

fn icon_of(kind: &str) -> &'static str {
    match kind {
        "file" => "add_data",
        "url" => "web_map",
        "tiles" => "basemap",
        "service" => "globe_view",
        "database" => "table",
        "zarr" => "raster_layer",
        "stac" => "search",
        "sample" => "map",
        "s3" => "package",
        _ => "add_data",
    }
}

fn text<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// Adds a source, or opens the dialog that browses it (a service's layers, a database's tables,
/// a STAC API's collections), connected.
pub fn open(app: &mut RustGisApp, s: &Source) -> Result<(), String> {
    let x = &s.extra;
    match s.kind.as_str() {
        "file" => match text(x, "layer") {
            Some(l) => app.run("layer.addData", json!({"path": s.address, "layer": l})).map(|_| ()),
            // A file holding several layers asks which to add, as Add Data does (#934).
            None => crate::layerchooser::add_file(app, &s.address).map(|_| ()),
        },
        "url" => {
            let mut p = json!({"url": s.address, "name": s.name});
            if let Some(l) = text(x, "layer") {
                p["layer"] = json!(l);
            }
            app.run("ui.addUrl", p).map(|_| ())
        }
        "tiles" => {
            let mut p = json!({"url": s.address, "name": s.name});
            for k in ["kind", "attribution"] {
                if let Some(v) = text(x, k) {
                    p[k] = json!(v);
                }
            }
            if let Some(z) = text(x, "maxZoom").and_then(|z| z.parse::<u32>().ok()) {
                p["maxZoom"] = json!(z);
            }
            app.run("map.addTiles", p).map(|_| ())
        }
        "service" => {
            app.run("ui.dialog", json!({"name": "addService"}))?;
            app.run("ui.addService", json!({"url": s.address, "type": text(x, "type").unwrap_or("auto")})).map(|_| ())
        }
        "database" => {
            app.run("ui.dialog", json!({"name": "addDatabase"}))?;
            app.dialogs.a = s.address.clone();
            if let Some(l) = text(x, "layer") {
                app.dialogs.c = l.to_string();
            }
            Ok(())
        }
        "zarr" => app.run("ui.addZarr", json!({"address": s.address})).map(|_| ()),
        "stac" if text(x, "api").is_some() => {
            // The panel's own switch: it also drops a catalog walk still on its way from the last
            // API, which would otherwise land in this listing through this connection.
            crate::stacui::switch_api(&mut app.ui.stac, &s.address);
            app.ui.stac.preset = None;
            app.run("ui.dialog", json!({"name": "stac"})).map(|_| ())
        }
        "stac" => app.run("ui.stacLink", json!({"url": s.address})).map(|_| ()),
        "sample" => app.run("layer.addSample", json!({"name": s.address})).map(|_| ()),
        "s3" => {
            crate::s3ui::open_connection(app, &s.address);
            app.run("ui.dialog", json!({"name": "s3"})).map(|_| ())
        }
        other => Err(format!("unknown source kind '{other}'")),
    }
}

/// One source's row: its icon and name (a click adds or opens it), and a menu.
fn row(app: &mut RustGisApp, ui: &mut Ui, t: &Tokens, s: &Source, list: Option<&str>) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        icons::paint(ui.painter(), r, icon_of(&s.kind), t.icon, t.accent);
        let resp = ui.link(&s.name).on_hover_text(&s.address);
        if resp.clicked()
            && let Err(e) = open(app, s)
        {
            app.session.notify(e, true);
        }
        resp.context_menu(|ui| {
            let fav = app.session.sources.favourites.iter().any(|f| f.is(&s.kind, &s.address));
            let p = json!({"kind": s.kind, "address": s.address, "name": s.name, "extra": s.extra});
            if !fav && ui.button(tl!("Add to Favourites")).clicked() {
                let _ = app.run("sources.favourite", p.clone());
                ui.close();
            }
            if fav && ui.button(tl!("Remove from Favourites")).clicked() {
                let mut q = p.clone();
                q["on"] = json!(false);
                let _ = app.run("sources.favourite", q);
                ui.close();
            }
            if let Some(list) = list.filter(|l| *l != "favourites")
                && ui.button(tl!("Remove")).clicked()
            {
                let _ = app.run("sources.remove", json!({"list": list, "kind": s.kind, "address": s.address}));
                ui.close();
            }
            if ui.button(tl!("Copy Address")).clicked() {
                ui.ctx().copy_text(s.address.clone());
                ui.close();
            }
        });
    });
}

fn header(ui: &mut Ui, title: &str, open: bool, body: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(RichText::new(title).font(semibold(12.5))).id_salt(("sources", title)).default_open(open).show(ui, body);
}

fn empty(ui: &mut Ui, what: &str) {
    ui.label(RichText::new(what).small().weak());
}

fn connections(app: &mut RustGisApp, ui: &mut Ui, t: &Tokens, list: &[Connection]) {
    crate::connectiontree::prune(app);
    // A project's S3 sources waiting for the saved connection that reads them.
    if !app.session.s3_pending.is_empty()
        && ui
            .button(tl!("Choose S3 connection…"))
            .on_hover_text(tl!("Some of the project's S3 sources wait for you to choose the saved connection that reads them"))
            .clicked()
        && let Err(e) = app.run("ui.s3Connections", Value::Null)
    {
        app.session.notify(e, true);
    }
    ui.horizontal(|ui| {
        ui.menu_button(tl!("New Connection ▾"), |ui| {
            for (kind, label) in crate::connectionui::KINDS {
                if cfg!(target_arch = "wasm32") && DATABASE_KINDS.contains(&kind) {
                    continue;
                }
                if ui.button(tl!(label)).clicked() {
                    if let Err(e) = app.run("ui.connection", json!({"kind": kind})) {
                        app.session.notify(e, true);
                    }
                    ui.close();
                }
            }
        });
        if ui.small_button("⟳").on_hover_text(tl!("Refresh every connection's lists")).clicked() {
            for c in list.iter().filter(|c| DATABASE_KINDS.contains(&c.kind.as_str())) {
                app.session.database_logins.retry_saved(&c.id);
            }
            crate::connectiontree::refresh(app, None);
        }
    });
    if list.is_empty() {
        empty(ui, tl!("Connections you add are listed here"));
    }
    let mut groups = std::collections::BTreeMap::new();
    for c in list.iter().filter(|c| !c.group.is_empty()) {
        groups.entry(c.group.to_lowercase()).or_insert(c.group.as_str());
    }
    for (lower, group) in groups {
        egui::CollapsingHeader::new(group).id_salt(("connections", &lower)).default_open(true).show(ui, |ui| {
            for c in list.iter().filter(|c| c.group.to_lowercase() == lower) {
                connection_row(app, ui, t, c);
            }
        });
    }
    for c in list.iter().filter(|c| c.group.is_empty()) {
        connection_row(app, ui, t, c);
    }
}

fn connection_row(app: &mut RustGisApp, ui: &mut Ui, t: &Tokens, c: &Connection) {
    let unavailable = DATABASE_KINDS.contains(&c.kind.as_str()) && cfg!(target_arch = "wasm32");
    if unavailable {
        ui.horizontal(|ui| {
            connection_header(app, ui, t, c, true);
        });
        return;
    }
    // A click on the name opens the row: the datasets the connection offers are listed under it.
    let state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), ui.make_persistent_id(("connection", &c.id)), false);
    let open = state.is_open();
    let mut clicked = false;
    let mut header = state.show_header(ui, |ui| clicked = connection_header(app, ui, t, c, false));
    if clicked {
        header.set_open(!open);
    }
    header.body(|ui| crate::connectiontree::show(app, ui, t, c));
}

/// A connection's icon, name, kind and menu. True when the name was clicked.
fn connection_header(app: &mut RustGisApp, ui: &mut Ui, t: &Tokens, c: &Connection, unavailable: bool) -> bool {
    let icon = if DATABASE_KINDS.contains(&c.kind.as_str()) {
        "table"
    } else {
        match c.kind.as_str() {
            "stac" => "search",
            "s3" => "package",
            _ => "globe_view",
        }
    };
    let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    icons::paint(ui.painter(), r, icon, t.icon, t.accent);
    let response = if unavailable {
        ui.link(RichText::new(&c.name).weak()).on_hover_text(tl!("Database connections are used by the desktop app"))
    } else {
        ui.link(&c.name).on_hover_text(if c.kind == "s3" && c.address.is_empty() { tl!("Amazon S3") } else { &c.address })
    };
    // The status precedes the kind: the narrow pane clips trailing row text.
    if DATABASE_KINDS.contains(&c.kind.as_str()) && c.remembered {
        ui.label(RichText::new(tl!("Saved password")).small().weak());
    }
    // The saved-keys marker (the store isn't asked while drawing).
    if c.kind == "s3" && c.saved_keys {
        if app.session.database_logins.has_store() {
            ui.label(RichText::new(tl!("Saved keys")).small().weak())
                .on_hover_text(tl!("This computer keeps this connection's keys: logging in reads them."));
        } else {
            ui.label(RichText::new(tl!("Saved keys unavailable")).small().weak())
                .on_hover_text(tl!("Keys were saved for this connection, but there is no credential store here to read them from."));
        }
    }
    if rustgis_engine::service_logins::is_service(c) && c.remembered {
        let label = if app.session.database_logins.has_store() { tl!("Saved login") } else { tl!("Saved login unavailable") };
        ui.label(RichText::new(label).small().weak());
    }
    ui.label(RichText::new(tl!(crate::connectionui::label(&c.kind))).small().weak());
    response.context_menu(|ui| {
        let mut items = vec![
            (tl!("Edit…"), "ui.connection", json!({"id":c.id})),
            (tl!("Test"), "ui.connection", json!({"id":c.id,"test":true})),
            (tl!("Duplicate"), "connection.duplicate", json!({"id":c.id})),
        ];
        if unavailable {
            items.remove(1);
        }
        for (label, command, params) in items {
            if ui.button(label).clicked() {
                if let Err(e) = app.run(command, params) {
                    app.session.notify(crate::connectionui::translated(&e), true);
                }
                ui.close();
            }
        }
        if !unavailable {
            let keys = c.kind == "s3" && c.options.get("credentials").is_some_and(|v| v == "keys");
            let service_login = rustgis_engine::service_logins::is_service(c) && !c.auth.is_empty();
            if (DATABASE_KINDS.contains(&c.kind.as_str()) || keys || service_login) && ui.button(tl!("Log In…")).clicked() {
                crate::connectionui::log_in(app, &c.id);
                ui.close();
            }
            if DATABASE_KINDS.contains(&c.kind.as_str()) && c.remembered && ui.button(tl!("Forget Saved Password")).clicked() {
                match app.run("connection.forgetPassword", json!({"id":c.id})) {
                    Ok(_) => crate::connectiontree::refresh(app, Some(&c.id)),
                    Err(e) => app.session.notify(crate::connectionui::translated(&e), true),
                }
                ui.close();
            }
            if service_login
                && c.remembered
                && ui.add_enabled(app.session.database_logins.has_store(), egui::Button::new(tl!("Forget Saved Login"))).clicked()
            {
                match app.run("connection.forgetLogin", json!({"id":c.id})) {
                    Ok(_) => {
                        crate::connectiontree::refresh(app, Some(&c.id));
                        app.session.notify(tl!("The saved login was forgotten; a login in this session keeps working.").to_string(), false);
                    }
                    Err(e) => app.session.notify(crate::connectionui::translated(&e), true),
                }
                ui.close();
            }
            if service_login && rustgis_engine::service_logins::logged_in(&app.session, c) && ui.button(tl!("Log Out")).clicked() {
                match app.run("connection.logout", json!({"id": c.id})) {
                    Ok(_) => {
                        crate::connectiontree::refresh(app, Some(&c.id));
                        app.session.notify(tl!("Logged out. Log in explicitly to use this connection again.").to_string(), false);
                    }
                    Err(e) => app.session.notify(crate::connectionui::translated(&e), true),
                }
                ui.close();
            }
            if ui.button(tl!("Refresh")).clicked() {
                if DATABASE_KINDS.contains(&c.kind.as_str()) {
                    app.session.database_logins.retry_saved(&c.id);
                }
                crate::connectiontree::refresh(app, Some(&c.id));
                ui.close();
            }
            // The kind's own add dialog, as a click on the connection opened it before.
            if ui.button(tl!("Add With Options…")).clicked() {
                if let Err(e) = crate::connectionui::open_connection(app, c) {
                    app.session.notify(crate::connectionui::translated(&e), true);
                }
                ui.close();
            }
            if c.kind == "s3" && c.saved_keys && ui.button(tl!("Forget Saved Keys")).clicked() {
                match crate::connectionui::forget_keys(app, &c.id) {
                    Ok(()) => app.session.notify(
                        tl!("The keys saved on this computer were forgotten; a login in this session keeps working.").to_string(),
                        false,
                    ),
                    Err(e) => app.session.notify(crate::connectionui::translated(&e), true),
                }
                ui.close();
            }
        }
        if !c.address.is_empty() && ui.button(tl!("Copy Address")).clicked() {
            ui.ctx().copy_text(c.address.clone());
            ui.close();
        }
        if ui.button(tl!("Remove")).clicked() {
            match app.run("connection.remove", json!({"id":c.id})) {
                Ok(_) => crate::connectiontree::refresh(app, Some(&c.id)),
                Err(e) => app.session.notify(crate::connectionui::translated(&e), true),
            }
            ui.close();
        }
    });
    !unavailable && response.clicked()
}

pub fn show(app: &mut RustGisApp, ui: &mut Ui, t: &Tokens) {
    load(app);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::NONE.inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
            ui.label(
                RichText::new(tl!("Click a source to add it, or to open its browser connected. Right-click for favourites."))
                    .small()
                    .weak(),
            );
            // A snapshot shared across frames, taken again only when the sources change.
            let src = snapshot(app);
            header(ui, tl!("Favourites"), true, |ui| {
                if src.favourites.is_empty() {
                    empty(ui, tl!("Right-click a source to add it here"));
                }
                for s in &src.favourites {
                    row(app, ui, t, s, Some("favourites"));
                }
            });
            header(ui, tl!("Recent"), true, |ui| {
                if src.recent.is_empty() {
                    empty(ui, tl!("Data you add is listed here"));
                }
                for s in &src.recent {
                    row(app, ui, t, s, Some("recent"));
                }
            });
            // A web page can't reach local folders.
            if !cfg!(target_arch = "wasm32") {
                header(ui, tl!("Folders"), false, |ui| {
                    let folders = app.session.project.folders.clone();
                    for f in folders {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                            icons::paint(ui.painter(), r, "add_folder", t.icon, t.accent);
                            if ui.link(&f).on_hover_text(tl!("Browse it in the Browser")).clicked() {
                                app.table.folder = Some(std::path::PathBuf::from(&f));
                                app.ui.left = Some(crate::LeftPane::Catalog);
                            }
                        });
                    }
                    if ui.small_button(tl!("Add Folder…")).clicked() {
                        let _ = app.run("ui.addFolder", Value::Null);
                    }
                });
            }
            let list = connections_snapshot(app);
            header(ui, tl!("Connections"), true, |ui| connections(app, ui, t, &list));
            header(ui, tl!("Library"), false, |ui| {
                let mut groups: Vec<&str> = Vec::new();
                let lib = library();
                for (g, _) in lib {
                    if !groups.contains(g) {
                        groups.push(g);
                    }
                }
                for g in groups {
                    egui::CollapsingHeader::new(tl!(g)).id_salt(("library", g)).default_open(false).show(ui, |ui| {
                        for (_, s) in lib.iter().filter(|(x, _)| x == &g) {
                            row(app, ui, t, s, None);
                        }
                    });
                }
            });
        });
    });
}
