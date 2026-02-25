use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn golden_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/golden")
        .join(name)
}

#[test]
fn logs_list_entries_fixture_is_valid_json() {
    let path = golden_fixture("logs_list_entries.json");
    assert!(path.exists(), "missing fixture at {}", path.display());

    let raw = fs::read_to_string(&path).expect("read fixture file");
    let value: Value = serde_json::from_str(&raw).expect("parse fixture JSON");

    let entries = value.as_array().expect("fixture should be a JSON array");
    assert!(!entries.is_empty(), "fixture array should not be empty");

    for entry in entries {
        let obj = entry
            .as_object()
            .expect("fixture entry should be an object");
        assert!(obj.contains_key("id"), "entry missing id field");
        assert!(obj.contains_key("model"), "entry missing model field");
        assert!(obj.contains_key("prompt"), "entry missing prompt field");
        assert!(obj.contains_key("response"), "entry missing response field");
    }
}
