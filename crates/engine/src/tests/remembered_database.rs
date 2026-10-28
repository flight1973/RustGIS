use super::*;
use rustgis_data::secrets::{MemorySecrets, Secret, SecretError, SecretStore};
use std::sync::Arc;

fn session(store: Option<Arc<dyn SecretStore>>) -> Session {
    let mut s = Session::new(Project::default());
    s.database_logins.set_store(store);
    s
}
fn add(s: &mut Session, name: &str) -> String {
    s.run(
        "connection.new",
        &json!({"kind":"postgres","name":name,"address":"postgres:host=127.0.0.1 port=1 user=remembered_lifecycle dbname=gis"}),
    )
    .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn remembered_database_opt_in_lifetime_and_identity() {
    let store = Arc::new(MemorySecrets::new());
    let mut s = session(Some(store.clone()));
    let id = add(&mut s, "First");
    s.run("connection.login", &json!({"id":id,"password":"sentinel-one"})).unwrap();
    assert_eq!(store.load(&format!("connection/{id}")).unwrap(), None);
    let v = s.run("connection.login", &json!({"id":id,"password":"sentinel-one","remember":true})).unwrap();
    assert_eq!(v["passwordSaved"], true);
    let envelope: Value = serde_json::from_str(store.load(&format!("connection/{id}")).unwrap().unwrap().expose()).unwrap();
    assert_eq!(envelope, json!({"kind":"postgres","address":s.connections.find(&id).unwrap().address,"password":"sentinel-one"}));
    let other = add(&mut s, "Second");
    assert_eq!(s.run("connection.list", &Value::Null).unwrap()["connections"][1]["loggedIn"], false);
    s.run("connection.login", &json!({"id":other,"password":"sentinel-two","remember":true})).unwrap();
    s.run("connection.edit", &json!({"id":id,"name":"Renamed","group":"Work"})).unwrap();
    assert!(s.connections.find(&id).unwrap().remembered);
    let duplicate = s.run("connection.duplicate", &json!({"id":id})).unwrap();
    assert_eq!(duplicate["remembered"], false);
    assert_eq!(duplicate["loggedIn"], false);
    assert!(!s.connections.to_json().contains("sentinel"));
    assert!(!s.project.saved_text(None).unwrap().0.contains("sentinel"));
    s.run("connection.forgetPassword", &json!({"id":id})).unwrap();
    assert_eq!(store.load(&format!("connection/{id}")).unwrap(), None);
    assert!(s.database_logins.has_session_password(&id));
    s.run("connection.forgetPassword", &json!({"id":id})).unwrap();
    s.run("connection.edit", &json!({"id":other,"address":"postgres:host=127.0.0.1 port=1 user=another dbname=gis"})).unwrap();
    assert!(!s.connections.find(&other).unwrap().remembered);
    assert!(!s.database_logins.has_session_password(&other));
    assert_eq!(store.load(&format!("connection/{other}")).unwrap(), None);
}

#[test]
fn remembered_database_store_failures_preserve_session() {
    for store in [
        None,
        Some(Arc::new(MemorySecrets::failing(SecretError::Denied)) as Arc<dyn SecretStore>),
        Some(Arc::new(MemorySecrets::failing(SecretError::Unavailable("private backend payload".into()))) as Arc<dyn SecretStore>),
    ] {
        let mut s = session(store);
        let id = add(&mut s, "Database");
        let v = s.run("connection.login", &json!({"id":id,"password":"session-sentinel","remember":true})).unwrap();
        assert_eq!(v["passwordSaved"], false);
        assert_eq!(v["remembered"], false);
        assert_eq!(v["loggedIn"], true);
        assert_eq!(v["warning"], json!("The password is available for this session, but wasn't remembered on this computer."));
        assert!(!v.to_string().contains("private backend payload"));
        s.run("connection.login", &json!({"id":id})).unwrap();
    }
}

#[test]
fn remembered_database_invalid_envelope_and_reused_id() {
    let store = Arc::new(MemorySecrets::new());
    let mut s = session(Some(store.clone()));
    let id = add(&mut s, "Database");
    for raw in [
        "broken-secret-sentinel".to_string(),
        json!({"kind":"postgres","address":"postgres:host=elsewhere","password":"stale-sentinel"}).to_string(),
        json!({"kind":"postgres","address":s.connections.find(&id).unwrap().address,"password":"old-sentinel","extra":true}).to_string(),
    ] {
        store.save(&format!("connection/{id}"), &Secret::new(raw)).unwrap();
        s.connections.set_remembered(&id, true);
        assert!(s.run("connection.login", &json!({"id":id})).is_err());
        assert!(!s.connections.find(&id).unwrap().remembered);
        assert!(!s.notices.iter().any(|n| n.text.contains("sentinel")));
    }
    for remember in [json!("true"), json!(1), Value::Null, json!({})] {
        assert!(s.run("connection.login", &json!({"id":id,"password":"typed","remember":remember})).is_err());
    }
}

#[test]
fn remembered_database_delete_failure_is_transactional() {
    let store = Arc::new(MemorySecrets::new());
    let mut s = session(Some(store.clone()));
    let id = add(&mut s, "Database");
    s.run("connection.login", &json!({"id":id,"password":"original","remember":true})).unwrap();
    let old = s.connections.to_json();
    s.database_logins.set_store(Some(Arc::new(MemorySecrets::failing(SecretError::Denied))));
    assert!(s.run("connection.edit", &json!({"id":id,"address":"postgres:host=changed user=other"})).is_err());
    assert_eq!(s.connections.to_json(), old);
    assert!(s.run("connection.remove", &json!({"id":id})).is_err());
    assert_eq!(s.connections.to_json(), old);
    assert!(s.run("connection.forgetPassword", &json!({"id":id})).is_err());
    assert_eq!(s.connections.to_json(), old);
    s.database_logins.set_store(Some(store.clone()));
    let second = add(&mut s, "Occupied");
    assert!(s.run("connection.edit", &json!({"id":id,"name":"Occupied","address":"postgres:host=changed"})).is_err());
    assert!(store.load(&format!("connection/{id}")).unwrap().is_some());
    s.run("connection.remove", &json!({"id":id})).unwrap();
    assert_eq!(store.load(&format!("connection/{id}")).unwrap(), None);
    assert!(s.connections.find(&second).is_some());
}

#[test]
fn remembered_database_passwordless_login_reports_cached_credentials_per_id() {
    let store = Arc::new(MemorySecrets::new());
    let mut seed = session(Some(store.clone()));
    let id = add(&mut seed, "Remembered");
    let other = add(&mut seed, "Other");
    seed.run("connection.login", &json!({"id":id,"password":"status-sentinel","remember":true})).unwrap();
    let mut fresh = session(Some(store));
    fresh.connections = crate::connections::Connections::from_json(&seed.connections.to_json());
    assert_eq!(fresh.run("connection.list", &Value::Null).unwrap()["connections"][0]["loggedIn"], false);
    assert_eq!(fresh.run("connection.login", &json!({"id":id})).unwrap()["loggedIn"], true);
    let listing = fresh.run("connection.list", &Value::Null).unwrap();
    assert_eq!(listing["connections"][0]["loggedIn"], true);
    assert_eq!(listing["connections"][1]["id"], other);
    assert_eq!(listing["connections"][1]["loggedIn"], false);
}

#[test]
fn remembered_database_used_by_a_session_given_the_desktop_prefs() {
    let store = Arc::new(MemorySecrets::new());
    let mut seed = session(Some(store.clone()));
    let id = add(&mut seed, "Remembered");
    seed.run("connection.login", &json!({"id":id,"password":"prefs-sentinel","remember":true})).unwrap();
    let prefs = serde_json::Map::from_iter([(crate::connections::PREF.into(), Value::String(seed.connections.to_json()))]);
    let mut fresh = Session::new(Project::default());
    crate::prefs::use_saved_secrets(&mut fresh, &prefs, store.clone());
    assert_eq!(fresh.run("connection.login", &json!({"id":id})).unwrap()["loggedIn"], true);
    let mut empty = Session::new(Project::default());
    crate::prefs::use_saved_secrets(&mut empty, &serde_json::Map::new(), store);
    assert_eq!(empty.run("connection.list", &Value::Null).unwrap()["connections"], json!([]));
    assert!(!fresh.notices.iter().any(|notice| notice.text.contains("prefs-sentinel")));
}
