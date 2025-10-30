use assert_cmd::Command;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use predicates::prelude::*;
use serde_json::Value;
use std::{fs, thread, time::Duration as StdDuration};

#[test]
fn prompt_command_returns_stub_response() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.arg("Hello world")
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: Hello world",
    ));
}

#[test]
fn prompt_subcommand_accepts_prompt_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args([
        "prompt",
        "--model",
        "custom-model",
        "--temperature",
        "0.5",
        "--max-tokens",
        "42",
        "--retries",
        "3",
        "--retry-backoff-ms",
        "500",
        "--no-stream",
        "--key",
        "inline-secret",
        "Hi there",
    ])
    .env("LLM_PROMPT_STUB", "1")
    .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: Hi there",
    ));
}

#[test]
fn prompt_subcommand_accepts_system_prompt() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args([
        "prompt",
        "--system",
        "Behave excitedly",
        "--no-stream",
        "Test with system",
    ])
    .env("LLM_PROMPT_STUB", "1")
    .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: Test with system",
    ));
}

#[test]
fn top_level_prompt_accepts_prompt_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args([
        "--temperature",
        "0.25",
        "--max-tokens",
        "64",
        "--retries",
        "2",
        "--retry-backoff-ms",
        "300",
        "--no-stream",
        "Hello",
        "world",
    ])
    .env("LLM_PROMPT_STUB", "1")
    .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: Hello world",
    ));
}

#[test]
fn prompt_subcommand_streams_output() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["prompt", "Streaming test"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: Streaming test",
    ));
}

#[test]
fn top_level_prompt_streams_output() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["Hello", "stream"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: Hello stream",
    ));
}

#[test]
fn top_level_prompt_accepts_key_override() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["--key", "inline-secret", "--no-stream", "Hello", "override"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: Hello override",
    ));
}

#[test]
fn prompt_subcommand_accepts_key_alias_override() {
    let tmp = tempfile::tempdir().unwrap();
    let keys_path = tmp.path().join("keys.json");
    fs::write(&keys_path, r#"{"temp-key": "stored-secret"}"#).unwrap();

    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["prompt", "--key", "temp-key", "--no-stream", "Alias prompt"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: Alias prompt",
    ));
}

#[test]
fn prompt_subcommand_sets_conversation_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args([
        "prompt",
        "--conversation",
        "thread-123",
        "--conversation-name",
        "Bug triage",
        "--conversation-model",
        "anthropic/claude-3-haiku",
        "--no-stream",
        "Log this conversation",
    ])
    .env("LLM_PROMPT_STUB", "1")
    .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success();

    let mut logs = Command::cargo_bin("llm-cli").expect("binary exists");
    logs.args(["logs", "list", "--json", "--count", "1"])
        .env("LLM_USER_PATH", tmp.path());
    let output = logs.assert().success().get_output().stdout.clone();
    let entries: Value = serde_json::from_slice(&output).expect("valid json");
    let array = entries.as_array().expect("array");
    assert_eq!(array.len(), 1);
    let entry = array.first().expect("entry");
    assert_eq!(entry["conversation_id"], Value::String("thread-123".into()));
    assert_eq!(
        entry["conversation_name"],
        Value::String("Bug triage".into())
    );
    assert_eq!(
        entry["conversation_model"],
        Value::String("anthropic/claude-3-haiku".into())
    );

    let mut status = Command::cargo_bin("llm-cli").expect("binary exists");
    status
        .args(["logs", "status"])
        .env("LLM_USER_PATH", tmp.path());
    status.assert().success().stdout(predicate::str::contains(
        "Number of conversations logged:\t1",
    ));
}

#[test]
fn prompt_records_system_prompt_in_logs() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["--system", "Be terse", "Check logs"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success();

    let mut logs = Command::cargo_bin("llm-cli").expect("binary exists");
    logs.args(["logs", "list", "--json", "--count", "1"])
        .env("LLM_USER_PATH", tmp.path());
    let output = logs.assert().success().get_output().stdout.clone();
    let entries: Value = serde_json::from_slice(&output).expect("valid json");
    let array = entries.as_array().expect("array");
    assert_eq!(array.len(), 1);
    let entry = array.first().expect("entry");
    assert_eq!(entry["prompt"], Value::String("Check logs".into()));
    assert_eq!(entry["system"], Value::String("Be terse".into()));
}

#[test]
fn cmd_command_auto_accepts_stub() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["cmd", "undo", "last", "git", "commit"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_CMD_AUTO_ACCEPT", "1")
        .env("LLM_USER_PATH", tmp.path());
    let assertion = cmd.assert().success();
    let output = String::from_utf8(assertion.get_output().stdout.clone()).unwrap();
    assert!(output.contains("Auto-accepting generated command"));
    assert!(output.contains("Command failed with error"));
}

#[test]
fn cmd_command_sets_conversation_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args([
        "cmd",
        "--conversation",
        "ops-thread",
        "--conversation-name",
        "Deploy sequence",
        "--conversation-model",
        "openai/gpt-4o",
        "rollout",
        "latest",
    ])
    .env("LLM_PROMPT_STUB", "1")
    .env("LLM_CMD_AUTO_ACCEPT", "1")
    .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success();

    let mut logs = Command::cargo_bin("llm-cli").expect("binary exists");
    logs.args(["logs", "list", "--json", "--count", "1"])
        .env("LLM_USER_PATH", tmp.path());
    let output = logs.assert().success().get_output().stdout.clone();
    let entries: Value = serde_json::from_slice(&output).expect("valid json");
    let array = entries.as_array().expect("array");
    assert_eq!(array.len(), 1);
    let entry = array.first().expect("entry");
    assert_eq!(entry["conversation_id"], Value::String("ops-thread".into()));
    assert_eq!(
        entry["conversation_name"],
        Value::String("Deploy sequence".into())
    );
    assert_eq!(
        entry["conversation_model"],
        Value::String("openai/gpt-4o".into())
    );

    let mut status = Command::cargo_bin("llm-cli").expect("binary exists");
    status
        .args(["logs", "status"])
        .env("LLM_USER_PATH", tmp.path());
    status.assert().success().stdout(predicate::str::contains(
        "Number of conversations logged:\t1",
    ));
}

#[test]
fn models_list_outputs_available_models() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["models", "list", "--json"])
        .env("LLM_USER_PATH", tmp.path());
    let output = cmd.assert().success().get_output().stdout.clone();
    let value: Value = serde_json::from_slice(&output).expect("valid json");
    let array = value.as_array().expect("array");
    assert!(array.iter().any(|m| m["name"] == "openai/gpt-4o-mini"));
    assert!(array
        .iter()
        .any(|m| m["name"] == "anthropic/claude-3-opus-latest"));
    let gpt4o = array
        .iter()
        .find(|m| m["name"] == "openai/gpt-4o")
        .expect("gpt-4o present");
    let aliases = gpt4o["aliases"].as_array().expect("aliases array");
    assert!(aliases.iter().any(|alias| alias.as_str() == Some("4o")));
}

#[test]
fn models_default_sets_and_shows_value() {
    let tmp = tempfile::tempdir().unwrap();
    let mut set_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    set_cmd
        .args(["models", "default", "openai:gpt-4.1-mini"])
        .env("LLM_USER_PATH", tmp.path());
    set_cmd.assert().success().stdout(predicate::str::contains(
        "Default model set to openai/gpt-4.1-mini.",
    ));

    let mut show_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    show_cmd
        .args(["models", "default"])
        .env("LLM_USER_PATH", tmp.path());
    show_cmd.assert().success().stdout(predicate::str::contains(
        "Current default model: openai/gpt-4.1-mini",
    ));
}

#[test]
fn models_default_accepts_alias() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["models", "default", "4o"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "Default model set to openai/gpt-4o.",
    ));
}

#[test]
fn prompt_command_errors_without_key() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.arg("Hello world").env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("OpenAI API key not configured"));
}

#[test]
fn plugins_command_outputs_json() {
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["plugins", "--json"]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(
        json,
        Value::Array(vec![Value::String("llm-default-plugin-stub".into())])
    );
}

#[test]
fn keys_path_uses_user_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").unwrap();
    cmd.args(["keys", "path"]).env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains(tmp.path().display().to_string()));
}

#[test]
fn keys_get_returns_stored_value() {
    let tmp = tempfile::tempdir().unwrap();
    let keys_path = tmp.path().join("keys.json");
    fs::write(&keys_path, r#"{"openai": "stored-secret"}"#).unwrap();

    let mut cmd = Command::cargo_bin("llm-cli").unwrap();
    cmd.args(["keys", "get", "openai"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("stored-secret"));
}

#[test]
fn logs_list_returns_recent_prompt() {
    let tmp = tempfile::tempdir().unwrap();
    let mut prompt = Command::cargo_bin("llm-cli").expect("binary exists");
    prompt
        .arg("Hello logging")
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    prompt.assert().success();

    let mut logs = Command::cargo_bin("llm-cli").expect("binary exists");
    logs.args(["logs", "list", "--json"])
        .env("LLM_USER_PATH", tmp.path());
    let output = logs.assert().success().get_output().stdout.clone();
    let entries: Vec<Value> = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(entries.len(), 1);
    let entry = entries.first().unwrap();
    assert_eq!(entry["prompt"], Value::String("Hello logging".into()));
    assert_eq!(
        entry["response"],
        Value::String("llm-core stub response to: Hello logging".into())
    );
}

#[test]
fn logs_off_prevents_new_entries() {
    let tmp = tempfile::tempdir().unwrap();
    // First prompt should be logged
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .arg("First prompt")
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    // Disable logging
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .args(["logs", "off"])
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    // Second prompt should not be logged
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .arg("Second prompt")
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    let mut logs = Command::cargo_bin("llm-cli").expect("binary exists");
    logs.args(["logs", "list", "--json"])
        .env("LLM_USER_PATH", tmp.path());
    let output = logs.assert().success().get_output().stdout.clone();
    let entries: Vec<Value> = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(entries.len(), 1);
    let entry = entries.first().unwrap();
    assert_eq!(entry["prompt"], Value::String("First prompt".into()));
}

#[test]
fn no_log_flag_skips_persisting_entry() {
    let tmp = tempfile::tempdir().unwrap();
    // Baseline run creates the database.
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .arg("Baseline prompt")
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    // --no-log should avoid writing a second entry.
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .args(["--no-log", "Skip logging"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    let mut logs = Command::cargo_bin("llm-cli").expect("binary exists");
    logs.args(["logs", "list", "--json", "--count", "0"])
        .env("LLM_USER_PATH", tmp.path());
    let output = logs.assert().success().get_output().stdout.clone();
    let entries: Vec<Value> = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0]["prompt"],
        Value::String("Baseline prompt".into())
    );
}

#[test]
fn log_flag_forces_logging_when_disabled() {
    let tmp = tempfile::tempdir().unwrap();
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .args(["logs", "off"])
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .args(["--log", "Force logging"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    assert!(
        tmp.path().join("logs-off").exists(),
        "logs-off sentinel should remain after --log override"
    );

    let mut logs = Command::cargo_bin("llm-cli").expect("binary exists");
    logs.args(["logs", "list", "--json", "--count", "0"])
        .env("LLM_USER_PATH", tmp.path());
    let output = logs.assert().success().get_output().stdout.clone();
    let entries: Vec<Value> = serde_json::from_slice(&output).expect("valid json");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["prompt"], Value::String("Force logging".into()));
}

#[test]
fn logs_list_filters_by_id_thresholds() {
    let tmp = tempfile::tempdir().unwrap();
    for text in ["First id test", "Second id test", "Third id test"] {
        Command::cargo_bin("llm-cli")
            .expect("binary exists")
            .arg(text)
            .env("LLM_PROMPT_STUB", "1")
            .env("LLM_USER_PATH", tmp.path())
            .assert()
            .success();
        thread::sleep(StdDuration::from_millis(5));
    }

    let mut all_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    all_cmd
        .args(["logs", "list", "--json", "--count", "0"])
        .env("LLM_USER_PATH", tmp.path());
    let all_output = all_cmd.assert().success().get_output().stdout.clone();
    let all_entries: Vec<Value> = serde_json::from_slice(&all_output).expect("valid json");
    assert!(
        all_entries.len() >= 3,
        "expected at least three log entries"
    );
    let mut ids: Vec<i64> = all_entries
        .iter()
        .map(|entry| entry["id"].as_i64().expect("id to be integer"))
        .collect();
    ids.sort();

    let smallest = ids[0];
    let largest = *ids.last().unwrap();

    let mut gt_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    gt_cmd
        .args([
            "logs",
            "list",
            "--json",
            "--count",
            "0",
            "--id-gt",
            &smallest.to_string(),
        ])
        .env("LLM_USER_PATH", tmp.path());
    let gt_output = gt_cmd.assert().success().get_output().stdout.clone();
    let gt_entries: Vec<Value> = serde_json::from_slice(&gt_output).expect("valid json");
    assert_eq!(gt_entries.len(), all_entries.len() - 1);
    assert!(gt_entries
        .iter()
        .all(|entry| entry["id"].as_i64().unwrap() > smallest));

    let mut gte_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    gte_cmd
        .args([
            "logs",
            "list",
            "--json",
            "--count",
            "0",
            "--id-gte",
            &largest.to_string(),
        ])
        .env("LLM_USER_PATH", tmp.path());
    let gte_output = gte_cmd.assert().success().get_output().stdout.clone();
    let gte_entries: Vec<Value> = serde_json::from_slice(&gte_output).expect("valid json");
    assert_eq!(gte_entries.len(), 1);
    assert_eq!(gte_entries[0]["id"].as_i64().unwrap(), largest);
}

#[test]
fn logs_list_since_excludes_earlier_entries() {
    let tmp = tempfile::tempdir().unwrap();
    for text in ["Old entry", "Newer entry", "Newest entry"] {
        Command::cargo_bin("llm-cli")
            .expect("binary exists")
            .arg(text)
            .env("LLM_PROMPT_STUB", "1")
            .env("LLM_USER_PATH", tmp.path())
            .assert()
            .success();
        thread::sleep(StdDuration::from_millis(5));
    }

    let mut all_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    all_cmd
        .args(["logs", "list", "--json", "--count", "0"])
        .env("LLM_USER_PATH", tmp.path());
    let all_output = all_cmd.assert().success().get_output().stdout.clone();
    let all_entries: Vec<Value> = serde_json::from_slice(&all_output).expect("valid json");
    assert!(
        all_entries.len() >= 3,
        "expected at least three log entries"
    );
    let oldest_ts = all_entries
        .iter()
        .map(|entry| {
            DateTime::parse_from_rfc3339(entry["datetime_utc"].as_str().expect("timestamp present"))
                .unwrap()
                .with_timezone(&Utc)
        })
        .min()
        .unwrap();
    let threshold = oldest_ts + ChronoDuration::milliseconds(1);

    let mut since_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    since_cmd
        .args([
            "logs",
            "list",
            "--json",
            "--count",
            "0",
            "--since",
            &threshold.to_rfc3339(),
        ])
        .env("LLM_USER_PATH", tmp.path());
    let since_output = since_cmd.assert().success().get_output().stdout.clone();
    let since_entries: Vec<Value> = serde_json::from_slice(&since_output).expect("valid json");
    assert!(
        since_entries.len() < all_entries.len(),
        "since filter should exclude at least one entry"
    );
    assert!(since_entries.iter().all(|entry| {
        let ts = DateTime::parse_from_rfc3339(
            entry["datetime_utc"].as_str().expect("timestamp present"),
        )
        .unwrap()
        .with_timezone(&Utc);
        ts >= threshold
    }));
}

#[test]
fn logs_backup_writes_database_copy() {
    let tmp = tempfile::tempdir().unwrap();
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .arg("Backup entry")
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    let backup_path = tmp.path().join("backup").join("logs.db");
    let backup_str = backup_path.to_string_lossy().to_string();
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .args(["logs", "backup", &backup_str])
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Backed up"));

    let metadata = fs::metadata(&backup_path).expect("backup file to exist");
    assert!(metadata.len() > 0);
}

#[test]
fn keys_get_errors_when_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").unwrap();
    cmd.args(["keys", "get", "missing"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("No key found"));
}

#[test]
fn keys_resolve_checks_env() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").unwrap();
    cmd.args(["keys", "resolve", "--env", "OPENAI_API_KEY"])
        .env("LLM_USER_PATH", tmp.path())
        .env("OPENAI_API_KEY", "env-secret");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("env-secret"));
}

#[test]
fn logs_path_uses_user_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").unwrap();
    cmd.args(["logs", "path"]).env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains(tmp.path().display().to_string()));
}

#[test]
fn keys_list_handles_empty_and_json() {
    let tmp = tempfile::tempdir().unwrap();

    let mut empty_cmd = Command::cargo_bin("llm-cli").unwrap();
    empty_cmd
        .args(["keys", "list"])
        .env("LLM_USER_PATH", tmp.path());
    empty_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("No keys found"));

    let keys_path = tmp.path().join("keys.json");
    fs::write(
        &keys_path,
        r#"{"// Note": "warn","openai": "a","anthropic": "b"}"#,
    )
    .unwrap();
    let mut json_cmd = Command::cargo_bin("llm-cli").unwrap();
    json_cmd
        .args(["keys", "list", "--json"])
        .env("LLM_USER_PATH", tmp.path());
    let output = json_cmd.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        json,
        Value::Array(vec![
            Value::String("anthropic".into()),
            Value::String("openai".into())
        ])
    );
}

#[test]
fn keys_set_writes_value() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").unwrap();
    cmd.args(["keys", "set", "openai", "--value", "new-secret"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Saved key 'openai'."));

    let contents = std::fs::read_to_string(tmp.path().join("keys.json")).unwrap();
    let map: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(
        map.get("openai").and_then(|v| v.as_str()),
        Some("new-secret")
    );
}
