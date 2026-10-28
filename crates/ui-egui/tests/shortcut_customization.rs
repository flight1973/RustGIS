//! Interaction tests for customizable shortcuts.
use egui_kittest::kittest::Queryable;
use rustgis_engine::{Project, Session};
use rustgis_ui_egui::{RustGisApp, Services};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

fn harness(prefs: Arc<Mutex<HashMap<String, String>>>) -> egui_kittest::Harness<'static, RustGisApp> {
    let reads = prefs.clone();
    let writes = prefs;
    let services = Services {
        load_pref: Some(Box::new(move |key| reads.lock().unwrap().get(key).cloned())),
        save_pref: Some(Box::new(move |key, value| {
            writes.lock().unwrap().insert(key.to_string(), value.to_string());
        })),
        ..Services::default()
    };
    let app = RustGisApp::new(Session::new(Project::default())).with_services(services);
    egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_max_steps(64).build_ui_state(
        |ui, app: &mut RustGisApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    )
}

#[test]
fn assigns_a_key_confirms_conflicts_and_restores_saved_preferences() {
    let prefs = Arc::new(Mutex::new(HashMap::new()));
    let mut h = harness(prefs.clone());
    h.state_mut().run("ui.customizeShortcuts", Value::Null).unwrap();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Search commands").focus();
    h.event(egui::Event::Text("ui.theme".into()));
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Add…").click();
    h.step();
    h.key_press(egui::Key::T);
    h.run_steps(3);
    assert!(prefs.lock().unwrap().get("shortcut_changes").is_some_and(|p| p.contains("ui.theme")));

    h.get_by_role_and_label(egui::accesskit::Role::Button, "Add…").click();
    h.step();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::T);
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Cancel").click();
    h.run_steps(3);
    let before = prefs.lock().unwrap().get("shortcut_changes").cloned().unwrap();
    assert!(!before.contains("Mod+T"));

    let restored = harness(prefs);
    assert!(restored.state().ui.keymap.bindings.iter().any(|b| b.command == "ui.theme" && b.chord.key == egui::Key::T));
    assert!(
        restored.state().ui.keymap.bindings.iter().any(|b| b.command == "table.open" && b.chord.key == egui::Key::T && b.chord.command)
    );
}

#[test]
fn tab_and_arrow_keys_can_be_captured() {
    let mut h = harness(Arc::new(Mutex::new(HashMap::new())));
    h.state_mut().run("ui.customizeShortcuts", Value::Null).unwrap();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Search commands").focus();
    h.event(egui::Event::Text("ui.theme".into()));
    h.run_steps(3);
    for key in [egui::Key::Tab, egui::Key::ArrowLeft] {
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Add…").click();
        h.step();
        h.key_press(key);
        h.run_steps(3);
        assert!(h.state().ui.keymap.bindings.iter().any(|b| b.command == "ui.theme" && b.chord.key == key), "{key:?} was not captured");
    }
}

#[test]
fn escape_cancels_capture_but_can_be_assigned_explicitly_and_reset() {
    let prefs = Arc::new(Mutex::new(HashMap::new()));
    let mut h = harness(prefs.clone());
    h.state_mut().run("ui.customizeShortcuts", Value::Null).unwrap();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Search commands").focus();
    h.event(egui::Event::Text("ui.theme".into()));
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Add…").focus();
    h.run_steps(3);
    h.key_press(egui::Key::Enter);
    h.step();
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.query_by_label("Assign Escape").is_none());
    assert!(prefs.lock().unwrap().get("shortcut_changes").is_none());
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Add…").click();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Assign Escape").click();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Replace").click();
    h.run_steps(3);
    assert!(h.state().ui.keymap.bindings.iter().any(|b| b.command == "ui.theme" && b.chord.key == egui::Key::Escape));
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Reset All").click();
    h.run_steps(3);
    assert!(h.state().ui.keymap.bindings.iter().any(|b| b.command == "tool.set" && b.chord.key == egui::Key::Escape));
    assert!(!h.state().ui.keymap.bindings.iter().any(|b| b.command == "ui.theme"));
    assert_eq!(prefs.lock().unwrap().get("shortcut_changes").unwrap(), "[]");
}

#[test]
fn command_search_context_menu_opens_matching_editor_row() {
    let mut h = harness(Arc::new(Mutex::new(HashMap::new())));
    h.state_mut().run("ui.commandSearch", Value::Null).unwrap();
    h.run_steps(3);
    h.event(egui::Event::Text("Toggle Dark Mode".into()));
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Toggle Dark Mode View").click_secondary();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Keyboard Shortcuts").click();
    h.run_steps(3);
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Add…").click();
    h.step();
    h.key_press(egui::Key::T);
    h.run_steps(3);
    assert!(h.state().ui.keymap.bindings.iter().any(|b| b.command == "ui.theme" && b.chord.key == egui::Key::T));
}

#[test]
fn unreadable_preferences_survive_edits_until_an_explicit_reset() {
    let too_many = serde_json::to_string(&vec![serde_json::json!({"keys":"D","command":"select.clear"}); 1_001]).unwrap();
    for original in ["not json".to_string(), " ".repeat(100_001), too_many] {
        let prefs = Arc::new(Mutex::new(HashMap::from([("shortcut_changes".into(), original.clone())])));
        let mut h = harness(prefs.clone());
        h.state_mut().run("ui.customizeShortcuts", Value::Null).unwrap();
        h.run_steps(3);
        assert!(h.query_by_label("Saved shortcuts are unreadable or exceed the limit. Changes apply only to this session; Reset All replaces the saved shortcuts.").is_some());
        h.get_by_role_and_label(egui::accesskit::Role::TextInput, "Search commands").focus();
        h.event(egui::Event::Text("ui.theme".into()));
        h.run_steps(3);
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Add…").click();
        h.step();
        h.key_press(egui::Key::T);
        h.run_steps(3);
        assert!(h.state().ui.keymap.bindings.iter().any(|b| b.command == "ui.theme"));
        assert_eq!(prefs.lock().unwrap().get("shortcut_changes").unwrap(), &original);
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Reset All").click();
        h.run_steps(3);
        assert_eq!(prefs.lock().unwrap().get("shortcut_changes").unwrap(), "[]");
        h.get_by_role_and_label(egui::accesskit::Role::Button, "Add…").click();
        h.step();
        h.key_press(egui::Key::T);
        h.run_steps(3);
        let reloaded = harness(prefs);
        assert!(reloaded.state().ui.keymap.bindings.iter().any(|b| b.command == "ui.theme" && b.chord.key == egui::Key::T));
    }
}
