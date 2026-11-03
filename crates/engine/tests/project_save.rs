use rustgis_engine::{Project, Session, cmd::design::EditCounts};
use serde_json::{Value, json};

/// Failed Save As preserves session state and history, and a later save can still succeed.
#[test]
fn failed_save_as_preserves_project_state_and_a_later_save_succeeds() -> Result<(), String> {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    let root = std::env::temp_dir().join(format!("rustgis-save-failure-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&root).map_err(|e| e.to_string())?;
    let original = root.join("original.rgis");
    let occupied = root.join("occupied.rgis");
    std::fs::create_dir(&occupied).map_err(|e| e.to_string())?;
    let marker = occupied.join("keep.txt");
    std::fs::write(&marker, b"keep destination contents").map_err(|e| e.to_string())?;

    for saved_before in [false, true] {
        for dirty in [false, true] {
            let mut s = Session::new(Project::new("original"));
            if saved_before {
                s.run("project.saveAs", &json!({"path": original}))?;
            }
            // Keep a real redo step, as well as undo history when the project is dirty.
            s.run("project.rename", &json!({"name": "pending"}))?;
            s.run("project.rename", &json!({"name": "redo name"}))?;
            s.run("edit.undo", &Value::Null)?;
            if dirty {
                s.mark_unsaved()
            } else {
                s.mark_saved()
            }
            s.edit_counts.insert(1, EditCounts { added: 2, modified: 3, deleted: 1 });
            let before = serde_json::to_value(&s.project).map_err(|e| e.to_string())?;
            let path = s.path.clone();
            let depth = s.undo_depth();
            let counts = s.edit_counts.clone();
            let history = (s.undo_label(), s.redo_label());
            assert!(s.can_undo() && s.can_redo());
            for target in [root.join("missing/changed.rgis"), occupied.clone()] {
                assert!(s.run("project.saveAs", &json!({"path": target})).is_err());
                assert_eq!(serde_json::to_value(&s.project).map_err(|e| e.to_string())?, before);
                assert_eq!(s.path, path);
                assert_eq!(s.dirty(), dirty);
                assert_eq!(s.undo_depth(), depth);
                assert_eq!(s.edit_counts, counts);
                assert_eq!((s.undo_label(), s.redo_label()), history);
                assert!(s.can_undo() && s.can_redo());
                assert_eq!(std::fs::read(&marker).map_err(|e| e.to_string())?, b"keep destination contents");
                if saved_before {
                    assert_eq!(Project::load(&original)?.name, "original");
                }
            }
            s.run("edit.redo", &Value::Null)?;
            assert_eq!(s.project.name, "redo name");
            s.run("edit.undo", &Value::Null)?;
            assert_eq!(serde_json::to_value(&s.project).map_err(|e| e.to_string())?, before);

            s.edit_counts = counts;
            let saved = root.join("renamed");
            let expected = saved.with_extension("rgis");
            s.run("project.saveAs", &json!({"path": saved}))?;
            assert_eq!(s.project.name, "renamed");
            assert_eq!(Project::load(&expected)?.name, "renamed");
            assert_eq!(s.path.as_ref(), Some(&expected));
            assert!(!s.dirty());
            assert!(s.edit_counts.is_empty());
            assert_eq!(s.undo_depth(), depth);
            assert!(s.can_redo());
        }
    }
    std::fs::remove_dir_all(root).map_err(|e| e.to_string())
}

#[test]
fn project_from_newer_rustgis_reports_update_message() {
    let json = serde_json::json!({
        "format": 9999,
        "name": "future project"
    })
    .to_string();
    let err = Project::from_json(&json).unwrap_err();
    assert!(err.contains("saved by a newer RustGIS"));
    assert!(err.contains("Update RustGIS to open it"));
}

#[test]
fn project_with_unknown_variant_suggests_newer_version() {
    let json = serde_json::json!({
        "format": 1,
        "name": "project with unknown enum variant",
        "maps": [{
            "name": "Map",
            "projection": "FutureProjectionVariant"
        }]
    })
    .to_string();
    let err = Project::from_json(&json).unwrap_err();
    assert!(err.contains("may have been saved by a newer RustGIS") || err.contains("Update RustGIS to open it"));
}
