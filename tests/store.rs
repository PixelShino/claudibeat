use claudibeat::store::{Store, safe_key};
use std::fs;
use std::path::{Path, PathBuf};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/status-band")
}

/// A fresh copy of the fixture in the target dir, so a test can damage it.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&fixture(), &dir);
    dir
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn safe_key_replaces_unsafe() {
    assert_eq!(safe_key("local_ab:c/d"), "local_ab_c_d");
    assert_eq!(safe_key("cli-1.2"), "cli-1.2");
}

#[test]
fn reads_fixture() {
    let s = Store::new(fixture()).read();
    assert_eq!(s.index.len(), 3);
    assert_eq!(s.files.len(), 3);
    assert!(s.files.iter().all(|f| !f.key.is_empty()));
    let account = s.account.expect("account.json");
    assert_eq!(account.limits.len(), 2);
    assert_eq!(s.acks["local_bbb"].ack_at, 1_794_565_500_000);
}

#[test]
fn skips_torn_and_wrong_version() {
    let s = Store::new(fixture()).read();
    assert!(!s.acks.contains_key("zz"));
    assert!(s.files.iter().all(|f| f.key != "torn"));
}

#[test]
fn missing_dir_is_empty_not_error() {
    let s = Store::new(PathBuf::from("Z:/claudibeat-nope")).read();
    assert!(s.index.is_empty() && s.files.is_empty() && s.acks.is_empty() && s.account.is_none());
}

#[test]
fn keeps_last_good_on_torn_rewrite() {
    let dir = scratch("keeps_last_good");
    let mut store = Store::new(dir.clone());
    assert_eq!(store.read().index.len(), 3);
    fs::write(dir.join("index.json"), "{\"v\":1,\"sessions\":[").unwrap();
    assert_eq!(store.read().index.len(), 3);
}

#[test]
fn writes_ack_atomically() {
    let dir = scratch("writes_ack");
    Store::new(dir.clone()).write_ack("local_1", 5).unwrap();
    let text = fs::read_to_string(dir.join("acks/local_1.json")).unwrap();
    assert_eq!(text, r#"{"v":1,"ackAt":5,"by":"claudibeat"}"#);
    let leftovers = fs::read_dir(dir.join("acks")).unwrap().filter(|e| {
        e.as_ref().unwrap().path().extension().is_some_and(|x| x == "tmp")
    });
    assert_eq!(leftovers.count(), 0);
}

#[test]
fn writes_tray() {
    let dir = scratch("writes_tray");
    Store::new(dir.clone()).write_tray(7).unwrap();
    assert_eq!(fs::read_to_string(dir.join("tray.json")).unwrap(), r#"{"v":1,"at":7}"#);
}
