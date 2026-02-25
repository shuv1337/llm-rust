# ADR-002: ID Migration Strategy (Integer to ULID)

**Status:** Accepted  
**Date:** 2026-02-24  
**Author:** llm-rust team

## Context

The current Rust implementation uses integer autoincrement IDs for `responses.id` in the logs database. Upstream `llm` uses ULID-style string IDs (e.g., `01HQJK5VXYZ123ABC456DEF78`). This mismatch causes:

1. **Incompatibility:** Rust-created logs are unreadable by upstream `llm logs list`.
2. **Filter breakage:** `--id-gt`/`--id-gte` filters have different semantics (numeric vs lexical comparison).
3. **Foreign key issues:** Conversation IDs, tool call references, etc. all depend on the ID format.

Migration is required before M1 storage compatibility can be achieved.

## Decision

### 1. ULID Format Adoption

All response and conversation IDs will use the ULID format:

```
 01AN4Z07BY      79KA1307SR9X4MV3
|----------|    |----------------|
 Timestamp          Randomness
  (48-bit)           (80-bit)
```

**Properties:**
- 26 characters, Crockford Base32 encoded
- Lexicographically sortable by creation time
- 1.21e+24 unique IDs per millisecond (collision-resistant)
- Compatible with upstream string ID columns

**Rust implementation:** Use the `ulid` crate for generation.

### 2. Deterministic Legacy ID Migration

When migrating an existing database with integer IDs, the algorithm must preserve:
- **Chronological ordering:** Older responses must have lexically smaller ULIDs.
- **Logical ordering:** Responses created in the same millisecond must maintain their original integer order.
- **Reproducibility:** Running migration twice on the same data produces identical ULIDs.

**Algorithm:**

```python
# Pseudocode for deterministic migration
def migrate_integer_ids(db):
    # 1. Read all responses ordered by (datetime_utc ASC, id ASC)
    rows = db.execute("""
        SELECT id, datetime_utc 
        FROM responses 
        ORDER BY datetime_utc ASC, id ASC
    """)
    
    # 2. Group by datetime (millisecond precision)
    grouped = group_by_datetime_ms(rows)
    
    # 3. Generate ULIDs with deterministic randomness
    migration_map = {}
    for datetime_ms, group in grouped:
        for seq, row in enumerate(group):
            # Use datetime for timestamp component
            # Use hash(row.id + migration_seed) for randomness component
            # This ensures reproducibility
            ulid = generate_deterministic_ulid(
                timestamp_ms=datetime_ms,
                randomness=hash_to_80bits(f"{MIGRATION_SEED}:{row.id}")
            )
            migration_map[row.id] = ulid
    
    # 4. Update all tables using migration_map
    update_responses(db, migration_map)
    update_foreign_keys(db, migration_map)
```

**Migration seed:** A fixed constant (`LLM_RUST_MIGRATION_SEED_V1`) embedded in the migration code ensures identical runs produce identical output.

### 3. Schema Changes

**Before migration:**
```sql
CREATE TABLE responses (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- ...
);
```

**After migration:**
```sql
CREATE TABLE responses (
    id TEXT PRIMARY KEY,  -- ULID format
    -- ...
);
```

**Foreign key updates:**
- `conversations.id` → TEXT
- `tool_calls.response_id` → TEXT
- `attachments.response_id` → TEXT
- All other FK references

### 4. CLI Filter Migration

**Current behavior (integer):**
```bash
llm logs list --id-gt 100  # numeric comparison
```

**New behavior (ULID):**
```bash
llm logs list --id-gt 01HQJK5VXYZ123ABC  # lexical comparison
```

**Compatibility:**
- Accept both formats during migration period
- If input looks like an integer, emit deprecation warning and convert to ULID using migration map
- After one release cycle, require ULID format only

### 5. Ordering Guarantees

| Scenario | Guarantee |
|----------|-----------|
| Responses from different times | Lexical order = chronological order |
| Responses from same millisecond | Lexical order = original integer order |
| New Rust-created responses | Standard ULID generation (timestamp + random) |
| Mixed legacy + new responses | Correct ordering maintained |

### 6. Migration Execution

**Trigger:** Migration runs automatically on first database open after upgrade.

**Backup:** Before any schema change, create timestamped backup:
```
logs.db → logs.db.backup.20260224T151234Z
```

**Preflight mode:** `llm logs migrate --dry-run` shows:
- Number of rows to migrate
- Sample ID conversions (first 5, last 5)
- Estimated time
- Backup location

**Rollback:** Keep `legacy_id` column populated for one release cycle to enable rollback if needed.

## Consequences

### Positive
- Full upstream compatibility for logs database.
- Lexical sorting works correctly for all queries.
- Deterministic migration enables testing and verification.
- ULID generation is fast (~50ns per ID).

### Negative
- Migration complexity for existing Rust users (though user base is small).
- Slightly larger storage (26 chars vs 8-byte integer).
- One-time migration cost on first upgrade.

### Neutral
- New responses use standard ULID generation (not deterministic).
- Third-party tools reading the database must handle string IDs.

## Testing Requirements

1. **Round-trip test:** Migrate Rust DB → read with upstream → write with upstream → read with Rust
2. **Ordering test:** Verify `logs list` order matches pre-migration order
3. **Filter test:** Verify `--id-gt`/`--id-gte` produces same result set
4. **Idempotency test:** Running migration twice produces identical results
5. **Fixture test:** Migrate real upstream-created database (with existing string IDs) without error

## References

- ULID spec: https://github.com/ulid/spec
- Upstream migrations.py: https://github.com/simonw/llm/blob/6b84a0d36b0df1341a9b64ef7001d56eee5e9185/llm/migrations.py
- Roadmap M1: Storage and config compatibility foundation
