-- SQL script to generate upstream-compatible test fixtures
-- This creates databases matching the upstream Python llm schema

-- ============================================================================
-- upstream_logs_integer_ids.db - Pre-ULID migration schema (integer IDs)
-- ============================================================================

-- Note: This will be executed by the test setup to create fixtures
-- The actual schema matches what upstream llm creates before ULID migration

-- Core tables
CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    name TEXT,
    model TEXT
);

-- Pre-migration responses table with INTEGER id
CREATE TABLE IF NOT EXISTS responses (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model TEXT NOT NULL,
    prompt TEXT,
    system TEXT,
    prompt_json TEXT,
    options_json TEXT,
    response TEXT,
    response_json TEXT,
    conversation_id TEXT REFERENCES conversations(id),
    duration_ms INTEGER,
    datetime_utc TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    token_details TEXT
);

CREATE INDEX IF NOT EXISTS idx_responses_datetime ON responses(datetime_utc);
CREATE INDEX IF NOT EXISTS idx_responses_conversation_id ON responses(conversation_id);
CREATE INDEX IF NOT EXISTS idx_responses_model ON responses(model);

-- FTS for search (integer rowid version)
CREATE VIRTUAL TABLE IF NOT EXISTS responses_fts USING fts5(
    prompt,
    response,
    content='responses',
    content_rowid='id'
);

-- Attachments
CREATE TABLE IF NOT EXISTS attachments (
    id TEXT PRIMARY KEY,
    type TEXT,
    path TEXT,
    url TEXT,
    content BLOB
);

CREATE TABLE IF NOT EXISTS prompt_attachments (
    response_id INTEGER NOT NULL,
    attachment_id TEXT NOT NULL,
    "order" INTEGER,
    PRIMARY KEY (response_id, attachment_id),
    FOREIGN KEY (response_id) REFERENCES responses(id),
    FOREIGN KEY (attachment_id) REFERENCES attachments(id)
);

-- Schemas
CREATE TABLE IF NOT EXISTS schemas (
    id TEXT PRIMARY KEY,
    content TEXT
);

-- Fragments
CREATE TABLE IF NOT EXISTS fragments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hash TEXT NOT NULL UNIQUE,
    content TEXT,
    datetime_utc TEXT,
    source TEXT
);
CREATE INDEX IF NOT EXISTS idx_fragments_hash ON fragments(hash);

CREATE TABLE IF NOT EXISTS fragment_aliases (
    alias TEXT PRIMARY KEY,
    fragment_id INTEGER NOT NULL,
    FOREIGN KEY (fragment_id) REFERENCES fragments(id)
);

CREATE TABLE IF NOT EXISTS prompt_fragments (
    response_id INTEGER NOT NULL,
    fragment_id INTEGER NOT NULL,
    "order" INTEGER,
    PRIMARY KEY (response_id, fragment_id, "order"),
    FOREIGN KEY (response_id) REFERENCES responses(id),
    FOREIGN KEY (fragment_id) REFERENCES fragments(id)
);

CREATE TABLE IF NOT EXISTS system_fragments (
    response_id INTEGER NOT NULL,
    fragment_id INTEGER NOT NULL,
    "order" INTEGER,
    PRIMARY KEY (response_id, fragment_id, "order"),
    FOREIGN KEY (response_id) REFERENCES responses(id),
    FOREIGN KEY (fragment_id) REFERENCES fragments(id)
);

-- Tools
CREATE TABLE IF NOT EXISTS tools (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hash TEXT NOT NULL UNIQUE,
    name TEXT,
    description TEXT,
    input_schema TEXT,
    plugin TEXT
);
CREATE INDEX IF NOT EXISTS idx_tools_hash ON tools(hash);

CREATE TABLE IF NOT EXISTS tool_responses (
    tool_id INTEGER NOT NULL,
    response_id INTEGER NOT NULL,
    PRIMARY KEY (tool_id, response_id),
    FOREIGN KEY (tool_id) REFERENCES tools(id),
    FOREIGN KEY (response_id) REFERENCES responses(id)
);

CREATE TABLE IF NOT EXISTS tool_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    response_id INTEGER NOT NULL,
    tool_id INTEGER,
    name TEXT,
    arguments TEXT,
    tool_call_id TEXT,
    FOREIGN KEY (response_id) REFERENCES responses(id),
    FOREIGN KEY (tool_id) REFERENCES tools(id)
);
CREATE INDEX IF NOT EXISTS idx_tool_calls_response ON tool_calls(response_id);

CREATE TABLE IF NOT EXISTS tool_instances (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    plugin TEXT,
    name TEXT,
    arguments TEXT
);

CREATE TABLE IF NOT EXISTS tool_results (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    response_id INTEGER NOT NULL,
    tool_id INTEGER,
    name TEXT,
    output TEXT,
    tool_call_id TEXT,
    instance_id INTEGER,
    exception TEXT,
    FOREIGN KEY (response_id) REFERENCES responses(id),
    FOREIGN KEY (tool_id) REFERENCES tools(id),
    FOREIGN KEY (instance_id) REFERENCES tool_instances(id)
);
CREATE INDEX IF NOT EXISTS idx_tool_results_response ON tool_results(response_id);

CREATE TABLE IF NOT EXISTS tool_results_attachments (
    tool_result_id INTEGER NOT NULL,
    attachment_id TEXT NOT NULL,
    "order" INTEGER,
    PRIMARY KEY (tool_result_id, attachment_id),
    FOREIGN KEY (tool_result_id) REFERENCES tool_results(id),
    FOREIGN KEY (attachment_id) REFERENCES attachments(id)
);

-- Sample data
INSERT INTO conversations (id, name, model) VALUES 
    ('conv-001', 'Test Conversation 1', 'openai/gpt-4o'),
    ('conv-002', 'Debug Session', 'anthropic/claude-3-opus');

INSERT INTO responses (id, model, prompt, system, response, conversation_id, datetime_utc, duration_ms, input_tokens, output_tokens) VALUES 
    (1, 'openai/gpt-4o', 'Hello world', 'You are helpful', 'Hello! How can I assist you today?', 'conv-001', '2024-01-15T10:30:00Z', 250, 10, 15),
    (2, 'openai/gpt-4o', 'Follow up question', 'You are helpful', 'Sure, I can help with that!', 'conv-001', '2024-01-15T10:31:00Z', 180, 8, 12),
    (3, 'anthropic/claude-3-opus', 'Debug this code', 'You are a code expert', 'I found the issue in line 42.', 'conv-002', '2024-01-15T11:00:00Z', 500, 100, 200);

-- Populate FTS
INSERT INTO responses_fts(rowid, prompt, response) SELECT id, prompt, response FROM responses;

-- FTS triggers
CREATE TRIGGER IF NOT EXISTS responses_ai AFTER INSERT ON responses BEGIN
    INSERT INTO responses_fts(rowid, prompt, response) VALUES (new.id, new.prompt, new.response);
END;

CREATE TRIGGER IF NOT EXISTS responses_ad AFTER DELETE ON responses BEGIN
    INSERT INTO responses_fts(responses_fts, rowid, prompt, response)
        VALUES ('delete', old.id, old.prompt, old.response);
END;

CREATE TRIGGER IF NOT EXISTS responses_au AFTER UPDATE ON responses BEGIN
    INSERT INTO responses_fts(responses_fts, rowid, prompt, response)
        VALUES ('delete', old.id, old.prompt, old.response);
    INSERT INTO responses_fts(rowid, prompt, response) VALUES (new.id, new.prompt, new.response);
END;
