use assert_cmd::Command;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use predicates::prelude::*;
use serde_json::Value;
use std::{fs, thread, time::Duration as StdDuration};

const TINY_PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\xa6\x00\x00\x01\x1a\x02\x03\x00\x00\x00\xe6\x99\xc4^\x00\x00\x00\tPLTE\xff\xff\xff\x00\xff\x00\xfe\x01\x00\x12t\x01J\x00\x00\x00GIDATx\xda\xed\xd81\x11\x000\x08\xc0\xc0.]\xea\xaf&Q\x89\x04V\xe0>\xf3+\xc8\x91Z\xf4\xa2\x08EQ\x14EQ\x14EQ\x14EQ\xd4B\x91$I3\xbb\xbf\x08EQ\x14EQ\x14EQ\x14E\xd1\xa5\xd4\x17\x91\xc6\x95\x05\x15\x0f\x9f\xc5\t\x9f\xa4\x00\x00\x00\x00IEND\xaeB`\x82";

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
fn prompt_command_accepts_file_attachment() {
    let tmp = tempfile::tempdir().unwrap();
    let attachment_path = tmp.path().join("image.png");
    fs::write(&attachment_path, TINY_PNG).unwrap();

    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args([
        "prompt",
        "--no-stream",
        "-a",
        attachment_path.to_str().unwrap(),
        "describe file",
    ])
    .env("LLM_PROMPT_STUB", "1")
    .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: describe file",
    ));
}

#[test]
fn prompt_command_accepts_stdin_attachment() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["prompt", "--no-stream", "-a", "-", "describe piped content"])
        .write_stdin(TINY_PNG)
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: describe piped content",
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
        .any(|m| m["name"] == "anthropic/claude-opus-4-0"));
    assert!(array
        .iter()
        .any(|m| m["name"] == "openai/gpt-5.2-2025-12-11"));
    assert!(array.iter().any(|m| m["name"] == "openai/gpt-5"));
    assert!(array
        .iter()
        .any(|m| m["name"] == "anthropic/claude-sonnet-4-6"));
    assert!(array
        .iter()
        .any(|m| m["name"] == "anthropic/claude-opus-4-6"));
    assert!(array.iter().any(|m| m["name"] == "markov"));

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

    let array = json.as_array().expect("plugins json array");
    assert!(!array.is_empty(), "expected at least one compiled plugin");

    let markov = array
        .iter()
        .find(|entry| entry.get("id") == Some(&Value::String("llm-markov".into())))
        .expect("llm-markov plugin present");

    assert_eq!(markov.get("version"), Some(&Value::String("0.1.0".into())));
    assert_eq!(
        markov.get("min_host_version"),
        Some(&Value::String("1.0.0".into()))
    );
}

#[test]
fn prompt_uses_markov_plugin_model_without_api_key() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args([
        "prompt",
        "--model",
        "markov",
        "--no-stream",
        "the quick brown fox jumps over the lazy dog",
    ])
    .env("LLM_USER_PATH", tmp.path());

    let output = cmd.assert().success().get_output().stdout.clone();
    let text = String::from_utf8_lossy(&output);
    assert!(
        text.starts_with("quick brown fox jumps over the lazy dog"),
        "unexpected markov output: {text}"
    );
    assert!(!text.contains("llm-core stub response"));
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
    // IDs are now ULID strings, lexicographically sortable
    let mut ids: Vec<String> = all_entries
        .iter()
        .map(|entry| entry["id"].as_str().expect("id to be string").to_string())
        .collect();
    ids.sort();

    let smallest = &ids[0];
    let largest = ids.last().unwrap();

    let mut gt_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    gt_cmd
        .args([
            "logs", "list", "--json", "--count", "0", "--id-gt", smallest,
        ])
        .env("LLM_USER_PATH", tmp.path());
    let gt_output = gt_cmd.assert().success().get_output().stdout.clone();
    let gt_entries: Vec<Value> = serde_json::from_slice(&gt_output).expect("valid json");
    assert_eq!(gt_entries.len(), all_entries.len() - 1);
    assert!(gt_entries
        .iter()
        .all(|entry| entry["id"].as_str().unwrap() > smallest.as_str()));

    let mut gte_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    gte_cmd
        .args([
            "logs", "list", "--json", "--count", "0", "--id-gte", largest,
        ])
        .env("LLM_USER_PATH", tmp.path());
    let gte_output = gte_cmd.assert().success().get_output().stdout.clone();
    let gte_entries: Vec<Value> = serde_json::from_slice(&gte_output).expect("valid json");
    assert_eq!(gte_entries.len(), 1);
    assert_eq!(gte_entries[0]["id"].as_str().unwrap(), largest.as_str());
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

#[test]
fn prompt_query_selects_shortest_matching_model() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["--query", "gpt-4o", "--no-stream", "test query"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: test query",
    ));
}

#[test]
fn prompt_database_option_overrides_logs_path() {
    let tmp = tempfile::tempdir().unwrap();
    let custom_db = tmp.path().join("custom_logs.db");

    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args([
        "--database",
        custom_db.to_str().unwrap(),
        "--no-stream",
        "database test",
    ])
    .env("LLM_PROMPT_STUB", "1")
    .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success();

    // Verify the custom database was created
    assert!(custom_db.exists(), "Custom database should be created");
}

#[test]
fn prompt_usage_flag_shows_usage_note() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["--usage", "--no-stream", "usage test"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("usage"));
}

#[test]
fn prompt_stdin_merges_with_positional() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["--no-stream", "additional", "prompt"])
        .write_stdin("stdin content")
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    // Stdin content should come first per upstream semantics
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: stdin content\nadditional prompt",
    ));
}

#[test]
fn prompt_stdin_only() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["--no-stream"])
        .write_stdin("just stdin")
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: just stdin",
    ));
}

#[test]
fn prompt_stdin_not_used_when_attachment() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    // When -a - is used, stdin is consumed as attachment, not prompt
    cmd.args(["--no-stream", "-a", "-", "prompt with attachment"])
        .write_stdin(TINY_PNG)
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: prompt with attachment",
    ));
}

// ==================== Continuation Flag Migration Tests ====================

#[test]
fn continuation_flag_c_alone_is_boolean() {
    // -c alone (without a value) should be accepted as --continue boolean flag
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["--no-stream", "-c", "hello"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    // "hello" should be treated as the prompt, not the conversation ID
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("llm-core stub response to: hello"));
}

#[test]
fn continuation_flag_continue_long_form() {
    // --continue should be accepted as boolean flag
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["--no-stream", "--continue", "test prompt"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: test prompt",
    ));
}

#[test]
fn continuation_cid_explicit_id() {
    // --cid <id> should set conversation ID
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["--no-stream", "--cid", "my-conv-123", "hello"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success();

    // Verify the conversation ID was used by checking logs
    let mut list_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    list_cmd
        .args(["logs", "list", "--json", "-n", "1"])
        .env("LLM_USER_PATH", tmp.path());
    let output = list_cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let entries: Vec<Value> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["conversation_id"], "my-conv-123");
}

#[test]
fn continuation_conversation_alias_for_cid() {
    // --conversation should work as alias for --cid
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["--no-stream", "--conversation", "conv-alias-test", "hello"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success();

    // Verify the conversation ID was used
    let mut list_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    list_cmd
        .args(["logs", "list", "--json", "-n", "1"])
        .env("LLM_USER_PATH", tmp.path());
    let output = list_cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let entries: Vec<Value> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(entries[0]["conversation_id"], "conv-alias-test");
}

#[test]
fn continuation_legacy_c_id_rewrite_warning() {
    // Legacy `-c <id>` should be rewritten to `--cid <id>` with deprecation warning
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["--no-stream", "-c", "legacy-conv-id", "hello"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());

    // Should succeed and emit deprecation warning
    cmd.assert()
        .success()
        .stderr(predicate::str::contains("deprecated"))
        .stderr(predicate::str::contains("--cid"));

    // Verify the conversation ID was used (rewritten from -c to --cid)
    let mut list_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    list_cmd
        .args(["logs", "list", "--json", "-n", "1"])
        .env("LLM_USER_PATH", tmp.path());
    let output = list_cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let entries: Vec<Value> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(entries[0]["conversation_id"], "legacy-conv-id");
}

#[test]
fn continuation_legacy_c_equals_id_rewrite() {
    // Legacy `-c=<id>` form should also be rewritten
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["--no-stream", "-c=legacy-equals-id", "hello"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());

    cmd.assert()
        .success()
        .stderr(predicate::str::contains("deprecated"));

    // Verify the conversation ID was used
    let mut list_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    list_cmd
        .args(["logs", "list", "--json", "-n", "1"])
        .env("LLM_USER_PATH", tmp.path());
    let output = list_cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let entries: Vec<Value> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(entries[0]["conversation_id"], "legacy-equals-id");
}

#[test]
fn continuation_continue_loads_latest_conversation() {
    let tmp = tempfile::tempdir().unwrap();

    // First, create a conversation
    let mut cmd1 = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd1.args(["--no-stream", "--cid", "test-conv", "first message"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd1.assert().success();

    // Small delay to ensure different ULID timestamps
    thread::sleep(StdDuration::from_millis(10));

    // Now continue with -c (should resolve to latest conversation)
    let mut cmd2 = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd2.args(["--no-stream", "-c", "second message"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd2.assert().success();

    // Both should be in the same conversation
    let mut list_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    list_cmd
        .args([
            "logs",
            "list",
            "--json",
            "-n",
            "2",
            "--conversation",
            "test-conv",
        ])
        .env("LLM_USER_PATH", tmp.path());
    let output = list_cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let entries: Vec<Value> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(entries.len(), 2);
}

#[test]
fn cmd_continuation_cid_flag() {
    // cmd subcommand should also support --cid
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["cmd", "--cid", "cmd-conv-test", "list files"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_CMD_AUTO_ACCEPT", "1")
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success();

    // Verify conversation ID was recorded
    let mut list_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    list_cmd
        .args(["logs", "list", "--json", "-n", "1"])
        .env("LLM_USER_PATH", tmp.path());
    let output = list_cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let entries: Vec<Value> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(entries[0]["conversation_id"], "cmd-conv-test");
}

// ============================================================================
// Aliases command tests
// ============================================================================

#[test]
fn aliases_path_outputs_path() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["aliases", "path"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("aliases.json"));
}

#[test]
fn aliases_list_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["aliases", "list"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("No aliases defined"));
}

#[test]
fn aliases_list_empty_json() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["aliases", "list", "--json"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("{}"));
}

#[test]
fn aliases_set_and_list() {
    let tmp = tempfile::tempdir().unwrap();

    // Set an alias
    let mut set_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    set_cmd
        .args(["aliases", "set", "fast", "openai/gpt-4o-mini"])
        .env("LLM_USER_PATH", tmp.path());
    set_cmd.assert().success().stdout(predicate::str::contains(
        "Alias 'fast' now points to 'openai/gpt-4o-mini'",
    ));

    // List should show the alias
    let mut list_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    list_cmd
        .args(["aliases", "list"])
        .env("LLM_USER_PATH", tmp.path());
    list_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("fast: openai/gpt-4o-mini"));
}

#[test]
fn aliases_set_and_list_json() {
    let tmp = tempfile::tempdir().unwrap();

    // Set multiple aliases
    let mut set1 = Command::cargo_bin("llm-cli").expect("binary exists");
    set1.args(["aliases", "set", "smart", "anthropic/claude-3-opus"])
        .env("LLM_USER_PATH", tmp.path());
    set1.assert().success();

    let mut set2 = Command::cargo_bin("llm-cli").expect("binary exists");
    set2.args(["aliases", "set", "cheap", "openai/gpt-4o-mini"])
        .env("LLM_USER_PATH", tmp.path());
    set2.assert().success();

    // List as JSON
    let mut list_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    list_cmd
        .args(["aliases", "list", "--json"])
        .env("LLM_USER_PATH", tmp.path());
    let output = list_cmd.assert().success().get_output().stdout.clone();
    let aliases: Value = serde_json::from_slice(&output).expect("valid json");

    assert_eq!(aliases["smart"], "anthropic/claude-3-opus");
    assert_eq!(aliases["cheap"], "openai/gpt-4o-mini");
}

#[test]
fn aliases_set_overwrites_existing() {
    let tmp = tempfile::tempdir().unwrap();

    // Set initial alias
    let mut set1 = Command::cargo_bin("llm-cli").expect("binary exists");
    set1.args(["aliases", "set", "default", "openai/gpt-3.5-turbo"])
        .env("LLM_USER_PATH", tmp.path());
    set1.assert().success();

    // Overwrite with new value
    let mut set2 = Command::cargo_bin("llm-cli").expect("binary exists");
    set2.args(["aliases", "set", "default", "openai/gpt-4o"])
        .env("LLM_USER_PATH", tmp.path());
    set2.assert().success();

    // Verify new value
    let mut list_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    list_cmd
        .args(["aliases", "list", "--json"])
        .env("LLM_USER_PATH", tmp.path());
    let output = list_cmd.assert().success().get_output().stdout.clone();
    let aliases: Value = serde_json::from_slice(&output).expect("valid json");

    assert_eq!(aliases["default"], "openai/gpt-4o");
}

#[test]
fn aliases_remove_existing() {
    let tmp = tempfile::tempdir().unwrap();

    // Set an alias
    let mut set_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    set_cmd
        .args(["aliases", "set", "temp", "openai/gpt-4o-mini"])
        .env("LLM_USER_PATH", tmp.path());
    set_cmd.assert().success();

    // Remove it
    let mut remove_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    remove_cmd
        .args(["aliases", "remove", "temp"])
        .env("LLM_USER_PATH", tmp.path());
    remove_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("Alias 'temp' removed"));

    // Verify it's gone
    let mut list_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    list_cmd
        .args(["aliases", "list"])
        .env("LLM_USER_PATH", tmp.path());
    list_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("No aliases defined"));
}

#[test]
fn aliases_remove_nonexistent_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["aliases", "remove", "nonexistent"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("No alias 'nonexistent' found"));
}

#[test]
fn aliases_persists_to_file() {
    let tmp = tempfile::tempdir().unwrap();

    // Set an alias
    let mut set_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    set_cmd
        .args(["aliases", "set", "mymodel", "anthropic/claude-3-sonnet"])
        .env("LLM_USER_PATH", tmp.path());
    set_cmd.assert().success();

    // Verify the file exists and has correct content
    let aliases_path = tmp.path().join("aliases.json");
    assert!(aliases_path.exists());

    let content = fs::read_to_string(&aliases_path).expect("read aliases.json");
    let aliases: Value = serde_json::from_str(&content).expect("valid json");
    assert_eq!(aliases["mymodel"], "anthropic/claude-3-sonnet");
}

#[test]
fn aliases_default_subcommand_is_list() {
    let tmp = tempfile::tempdir().unwrap();

    // Set an alias first
    let mut set_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    set_cmd
        .args(["aliases", "set", "test", "openai/gpt-4o"])
        .env("LLM_USER_PATH", tmp.path());
    set_cmd.assert().success();

    // Running `aliases` without subcommand should list
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["aliases"]).env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("test: openai/gpt-4o"));
}

// ============================================================================
// Chat Command Tests
// ============================================================================

#[test]
fn chat_command_shows_help() {
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["chat", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("interactive chat session"))
        .stdout(predicate::str::contains("--model"))
        .stdout(predicate::str::contains("--continue"))
        .stdout(predicate::str::contains("--cid"));
}

#[test]
fn chat_command_accepts_model_option() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    // Chat is interactive, so we can't really run it without input
    // Just verify the args are accepted by checking help
    cmd.args(["chat", "--model", "gpt-4o", "--help"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success();
}

#[test]
fn chat_command_accepts_continuation_flags() {
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    // Verify --continue and --cid flags are documented
    cmd.args(["chat", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("-c, --continue"))
        .stdout(predicate::str::contains("--cid"));
}

// =============================================================================
// Templates command tests
// =============================================================================

#[test]
fn templates_path_returns_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["templates", "path"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("templates"));
}

#[test]
fn templates_list_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["templates", "list"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("No templates found"));
}

#[test]
fn templates_list_json_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["templates", "list", "--json"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("[]"));
}

#[test]
fn templates_edit_creates_template() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args([
        "templates",
        "edit",
        "greeting",
        "--content",
        "Hello, {{ name }}!",
    ])
    .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Template 'greeting' saved"));

    // Verify the template file was created
    let template_path = tmp.path().join("templates").join("greeting.txt");
    assert!(template_path.exists());
    let content = fs::read_to_string(&template_path).expect("read template");
    assert_eq!(content, "Hello, {{ name }}!");
}

#[test]
fn templates_show_displays_content() {
    let tmp = tempfile::tempdir().unwrap();

    // First create a template
    let mut create_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    create_cmd
        .args([
            "templates",
            "edit",
            "test",
            "--content",
            "Test content here",
        ])
        .env("LLM_USER_PATH", tmp.path());
    create_cmd.assert().success();

    // Now show it
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["templates", "show", "test"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Test content here"));
}

#[test]
fn templates_show_nonexistent_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["templates", "show", "nonexistent"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn templates_list_shows_created_templates() {
    let tmp = tempfile::tempdir().unwrap();

    // Create two templates
    for name in ["alpha", "beta"] {
        let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
        cmd.args([
            "templates",
            "edit",
            name,
            "--content",
            &format!("{} content", name),
        ])
        .env("LLM_USER_PATH", tmp.path());
        cmd.assert().success();
    }

    // List them
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["templates", "list"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("alpha"))
        .stdout(predicate::str::contains("beta"));
}

#[test]
fn templates_list_json_shows_array() {
    let tmp = tempfile::tempdir().unwrap();

    // Create a template
    let mut create_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    create_cmd
        .args(["templates", "edit", "mytemplate", "--content", "content"])
        .env("LLM_USER_PATH", tmp.path());
    create_cmd.assert().success();

    // List as JSON
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["templates", "list", "--json"])
        .env("LLM_USER_PATH", tmp.path());
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let templates: Vec<String> = serde_json::from_str(&stdout).expect("valid json");
    assert!(templates.contains(&"mytemplate".to_string()));
}

#[test]
fn templates_loaders_shows_filesystem() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["templates", "loaders"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("filesystem"));
}

#[test]
fn templates_loaders_json() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["templates", "loaders", "--json"])
        .env("LLM_USER_PATH", tmp.path());
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let loaders: Vec<Value> = serde_json::from_str(&stdout).expect("valid json");
    assert!(!loaders.is_empty());
    assert_eq!(loaders[0]["name"], "filesystem");
}

#[test]
fn templates_default_subcommand_is_list() {
    let tmp = tempfile::tempdir().unwrap();

    // Create a template first
    let mut create_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    create_cmd
        .args(["templates", "edit", "test", "--content", "content"])
        .env("LLM_USER_PATH", tmp.path());
    create_cmd.assert().success();

    // Running `templates` without subcommand should list
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["templates"]).env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("test"));
}

#[test]
fn templates_edit_overwrites_existing() {
    let tmp = tempfile::tempdir().unwrap();

    // Create initial template
    let mut cmd1 = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd1.args(["templates", "edit", "overwrite", "--content", "original"])
        .env("LLM_USER_PATH", tmp.path());
    cmd1.assert().success();

    // Overwrite it
    let mut cmd2 = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd2.args(["templates", "edit", "overwrite", "--content", "updated"])
        .env("LLM_USER_PATH", tmp.path());
    cmd2.assert().success();

    // Verify content was updated
    let mut show_cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    show_cmd
        .args(["templates", "show", "overwrite"])
        .env("LLM_USER_PATH", tmp.path());
    show_cmd
        .assert()
        .success()
        .stdout(predicate::str::contains("updated"))
        .stdout(predicate::str::contains("original").not());
}

// ============================================================================
// Logs list extended options tests
// ============================================================================

#[test]
fn logs_list_response_only_flag() {
    let tmp = tempfile::tempdir().unwrap();

    // Create a log entry
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .arg("test response only")
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    // List with --response flag
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["logs", "list", "--response", "-n", "1"])
        .env("LLM_USER_PATH", tmp.path());
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    // Should only show response, not prompt or metadata
    assert!(stdout.contains("llm-core stub response"));
    assert!(!stdout.contains("Model:"));
    assert!(!stdout.contains("Prompt:"));
}

#[test]
fn logs_list_short_format() {
    let tmp = tempfile::tempdir().unwrap();

    // Create a log entry
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .arg("test short format")
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    // List with --short flag
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["logs", "list", "--short", "-n", "1"])
        .env("LLM_USER_PATH", tmp.path());
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    // Should show prompt and response on single lines
    assert!(stdout.contains("Prompt:"));
    assert!(stdout.contains("Response:"));
    // Should NOT show full metadata
    assert!(!stdout.contains("Model:"));
    assert!(!stdout.contains("Duration:"));
}

#[test]
fn logs_list_truncate_long_text() {
    let tmp = tempfile::tempdir().unwrap();

    // Create a log entry with long prompt
    let long_prompt = "x".repeat(200);
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .arg(&long_prompt)
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    // List with --truncate flag
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["logs", "list", "--truncate", "-n", "1"])
        .env("LLM_USER_PATH", tmp.path());
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    // Should be truncated (ends with ...)
    assert!(stdout.contains("..."));
    // Should not contain the full 200 character string
    assert!(!stdout.contains(&long_prompt));
}

#[test]
fn logs_list_latest_returns_one_entry() {
    let tmp = tempfile::tempdir().unwrap();

    // Create multiple log entries
    for text in ["first entry", "second entry", "third entry"] {
        Command::cargo_bin("llm-cli")
            .expect("binary exists")
            .arg(text)
            .env("LLM_PROMPT_STUB", "1")
            .env("LLM_USER_PATH", tmp.path())
            .assert()
            .success();
        thread::sleep(StdDuration::from_millis(5));
    }

    // List with --latest flag
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["logs", "list", "--json", "--latest"])
        .env("LLM_USER_PATH", tmp.path());
    let output = cmd.assert().success().get_output().stdout.clone();
    let entries: Vec<Value> = serde_json::from_slice(&output).expect("valid json");

    // Should return only one entry
    assert_eq!(entries.len(), 1);
    // And it should be the most recent one
    assert_eq!(entries[0]["prompt"], "third entry");
}

#[test]
fn logs_list_current_filters_by_latest_conversation() {
    let tmp = tempfile::tempdir().unwrap();

    // Create entries in different conversations
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .args(["--cid", "conv-old", "old conversation"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    thread::sleep(StdDuration::from_millis(10));

    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .args(["--cid", "conv-current", "current conversation 1"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .args(["--cid", "conv-current", "current conversation 2"])
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    // List with --current flag should show only the latest conversation's entries
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["logs", "list", "--json", "--current", "-n", "0"])
        .env("LLM_USER_PATH", tmp.path());
    let output = cmd.assert().success().get_output().stdout.clone();
    let entries: Vec<Value> = serde_json::from_slice(&output).expect("valid json");

    // Should return only entries from conv-current
    assert_eq!(entries.len(), 2);
    for entry in &entries {
        assert_eq!(entry["conversation_id"], "conv-current");
    }
}

#[test]
fn logs_list_extract_code_blocks() {
    let tmp = tempfile::tempdir().unwrap();

    // We can't easily create a log entry with code blocks in the response using stub mode
    // So we'll test that the flag is accepted and works with regular content
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .arg("test extract")
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    // The --extract flag should be accepted
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["logs", "list", "--extract", "-n", "1"])
        .env("LLM_USER_PATH", tmp.path());
    // Should succeed (even if no code blocks are found, it just outputs nothing)
    cmd.assert().success();
}

#[test]
fn logs_list_extract_last_code_block() {
    let tmp = tempfile::tempdir().unwrap();

    // Create a log entry
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .arg("test extract last")
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    // The --extract-last flag should be accepted
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["logs", "list", "--extract-last", "-n", "1"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success();
}

#[test]
fn logs_list_tools_filter() {
    let tmp = tempfile::tempdir().unwrap();

    // Create a log entry (without tool calls, since stub doesn't support them)
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .arg("test tools filter")
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    // The --tools flag should be accepted and filter to entries with tool calls
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["logs", "list", "--json", "--tools", "-n", "0"])
        .env("LLM_USER_PATH", tmp.path());
    let output = cmd.assert().success().get_output().stdout.clone();
    let entries: Vec<Value> = serde_json::from_slice(&output).expect("valid json");

    // Should return no entries since stub mode doesn't create tool calls
    assert!(entries.is_empty());
}

#[test]
fn logs_list_extract_conflicts_with_extract_last() {
    let tmp = tempfile::tempdir().unwrap();

    // --extract and --extract-last should conflict
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["logs", "list", "--extract", "--extract-last"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn logs_list_latest_conflicts_with_current() {
    let tmp = tempfile::tempdir().unwrap();

    // --latest and --current should conflict
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["logs", "list", "--latest", "--current"])
        .env("LLM_USER_PATH", tmp.path());
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn logs_list_response_with_truncate() {
    let tmp = tempfile::tempdir().unwrap();

    // Create a log entry with long text
    let long_prompt = "y".repeat(200);
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .arg(&long_prompt)
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    // List with both --response and --truncate
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["logs", "list", "--response", "--truncate", "-n", "1"])
        .env("LLM_USER_PATH", tmp.path());
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    // Should be truncated
    assert!(stdout.contains("..."));
}

#[test]
fn logs_list_short_with_truncate() {
    let tmp = tempfile::tempdir().unwrap();

    // Create a log entry with long text
    let long_prompt = "z".repeat(200);
    Command::cargo_bin("llm-cli")
        .expect("binary exists")
        .arg(&long_prompt)
        .env("LLM_PROMPT_STUB", "1")
        .env("LLM_USER_PATH", tmp.path())
        .assert()
        .success();

    // List with both --short and --truncate
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["logs", "list", "--short", "--truncate", "-n", "1"])
        .env("LLM_USER_PATH", tmp.path());
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    // Should show truncated prompt and response
    assert!(stdout.contains("Prompt:"));
    assert!(stdout.contains("Response:"));
    assert!(stdout.contains("..."));
}

// ============================================================================
// Embeddings CLI Tests
// ============================================================================

#[test]
fn embed_models_list_outputs_models() {
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["embed-models", "list"]);
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    assert!(stdout.contains("text-embedding-3-small"));
    assert!(stdout.contains("text-embedding-3-large"));
    assert!(stdout.contains("text-embedding-ada-002"));
}

#[test]
fn embed_models_list_json_format() {
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["embed-models", "list", "--json"]);
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    // Should be valid JSON array
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(parsed.is_array());
    let arr = parsed.as_array().unwrap();
    assert!(arr.len() >= 3);

    // Check structure
    let first = &arr[0];
    assert!(first.get("model_id").is_some());
    assert!(first.get("provider").is_some());
}

#[test]
fn embed_models_default_subcommand_is_list() {
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["embed-models"]);
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    // Should show available models by default
    assert!(stdout.contains("embedding models"));
}

#[test]
fn collections_path_outputs_path() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["collections", "path"])
        .env("LLM_USER_PATH", tmp.path());
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    assert!(stdout.contains("embeddings.db"));
}

#[test]
fn collections_list_empty_database() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["collections", "list"])
        .env("LLM_USER_PATH", tmp.path());
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    // Should indicate no database found or empty
    assert!(stdout.contains("No embeddings database") || stdout.contains("No collections"));
}

#[test]
fn collections_list_empty_json() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["collections", "list", "--json"])
        .env("LLM_USER_PATH", tmp.path());
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    // Should be empty JSON array
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(parsed.is_array());
    assert!(parsed.as_array().unwrap().is_empty());
}

#[test]
fn collections_default_subcommand_is_list() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["collections"]).env("LLM_USER_PATH", tmp.path());
    cmd.assert().success();
}

#[test]
fn collections_delete_nonexistent_fails() {
    let tmp = tempfile::tempdir().unwrap();
    // Create an empty database first
    std::fs::write(tmp.path().join("embeddings.db"), "").unwrap();

    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["collections", "delete", "nonexistent"])
        .env("LLM_USER_PATH", tmp.path());
    // Will fail because it's an invalid/empty database
    cmd.assert().failure();
}

#[test]
fn embed_requires_content_or_stdin() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["embed"]).env("LLM_USER_PATH", tmp.path());

    // Should fail without content
    cmd.assert().failure();
}

#[test]
fn embed_accepts_model_option() {
    // Note: This will fail without API key, but we can check the error message
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["embed", "--model", "3-small", "hello world"])
        .env("LLM_USER_PATH", tmp.path())
        .env_remove("OPENAI_API_KEY")
        .env_remove("LLM_OPENAI_API_KEY");

    // Should fail with API key error, not model error
    let output = cmd.assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("API key") || stderr.contains("api_key") || stderr.contains("OPENAI"));
}

#[test]
fn similar_requires_collection() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["similar", "query text"])
        .env("LLM_USER_PATH", tmp.path());

    // Should fail because --collection is required
    cmd.assert().failure();
}

#[test]
fn similar_shows_help() {
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["similar", "--help"]);
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    assert!(stdout.contains("--collection"));
    assert!(stdout.contains("--number"));
    assert!(stdout.contains("--id"));
}

#[test]
fn embed_multi_requires_collection() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["embed-multi"]).env("LLM_USER_PATH", tmp.path());

    // Should fail because collection argument is required
    cmd.assert().failure();
}

#[test]
fn embed_multi_requires_files_or_sql() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["embed-multi", "test-collection"])
        .env("LLM_USER_PATH", tmp.path())
        .env_remove("OPENAI_API_KEY")
        .env_remove("LLM_OPENAI_API_KEY");

    // Should fail because neither --files nor --sql provided
    let output = cmd.assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("files") || stderr.contains("sql"));
}

#[test]
fn embed_multi_shows_help() {
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["embed-multi", "--help"]);
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    assert!(stdout.contains("--files"));
    assert!(stdout.contains("--sql"));
    assert!(stdout.contains("--batch-size"));
    assert!(stdout.contains("--model"));
}

#[test]
fn embed_shows_help() {
    let mut cmd = Command::cargo_bin("llm-cli").expect("binary exists");
    cmd.args(["embed", "--help"]);
    let output = cmd.assert().success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);

    assert!(stdout.contains("--model"));
    assert!(stdout.contains("--store"));
    assert!(stdout.contains("--metadata"));
    assert!(stdout.contains("--raw"));
    assert!(stdout.contains("--json"));
}
