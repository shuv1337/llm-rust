use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::fs;

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
        "Hi there",
    ])
    .env("LLM_PROMPT_STUB", "1")
    .env("LLM_USER_PATH", tmp.path());
    cmd.assert().success().stdout(predicate::str::contains(
        "llm-core stub response to: Hi there",
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
