# CLI Parity Matrix (Draft)

Status legend: `[ ]` not started, `[~]` in progress, `[x]` complete.

## Global Options
- [~] `--version`, `-h/--help` – confirm behavior and output parity.

## Command Reference

### `prompt`
- Status `[~]` – options captured, needs behavioral notes/tests mapping.
- Options: `-s/--system`, `-m/--model`, `-d/--database`, `-q/--query`, `-a/--attachment`, `--at/--attachment-type`, `-T/--tool`, `--functions`, `--td/--tools-debug`, `--ta/--tools-approve`, `--cl/--chain-limit`, `-o/--option`, `--schema`, `--schema-multi`, `-f/--fragment`, `--sf/--system-fragment`, `-t/--template`, `-p/--param`, `--no-stream`, `-n/--no-log`, `--log`, `-c/--continue`, `--cid/--conversation`, `--key`, `--save`, `--async`, `-u/--usage`, `-x/--extract`, `--xl/--extract-last`, `-h/--help`.
- Notes: Supports multimodal attachments, structured extraction, tool execution controls, logging toggles.

### `chat`
- Status `[~]`.
- Options: `-s/--system`, `-m/--model`, `-c/--continue`, `--cid/--conversation`, `-f/--fragment`, `--sf/--system-fragment`, `-t/--template`, `-p/--param`, `-o/--option`, `-d/--database`, `--no-stream`, `--key`, `-T/--tool`, `--functions`, `--td/--tools-debug`, `--ta/--tools-approve`, `--cl/--chain-limit`, `-h/--help`.
- Notes: Interactive session UI, shares tooling with `prompt`.

### `cmd`
- Status `[~]`.
- Options: `-m/--model`, `-s/--system`, `--key`, `-h/--help`.
- Notes: Executes shell commands suggested by model.

### `aliases`
- Status `[~]`.
- Subcommands: `list`, `set`, `remove`, `path`.
- Key options: `list --json`; `set -q/--query`; others standard help.
- Notes: Maintains alias mapping in `aliases.json`.

### `collections`
- Status `[~]`.
- Subcommands: `list`, `delete`, `path`.
- Options: `list -d/--database --json`; `delete -d/--database`; `path`.
- Notes: Operates on embeddings DB.

### `embed`
- Status `[~]`.
- Options: `-i/--input`, `-m/--model`, `--store`, `-d/--database`, `-c/--content`, `--binary`, `--metadata`, `-f/--format`, `-h/--help`.
- Notes: Inserts or returns embeddings.

### `embed-models`
- Status `[~]`.
- Subcommands: `list`, `default`.
- Options: `list -q/--query`; `default --remove-default`.

### `embed-multi`
- Status `[~]`.
- Options: `--format`, `--files <dir pattern>`, `--encoding`, `--binary`, `--sql`, `--attach <alias file>`, `--batch-size`, `--prefix`, `-m/--model`, `--prepend`, `--store`, `-d/--database`, `-h/--help`.
- Notes: Handles CSV/TSV/JSON/SQL/files ingestion workflows.

### `fragments`
- Status `[~]`.
- Subcommands: `list`, `set`, `remove`, `show`, `loaders`.
- Options: `list -q/--query --aliases --json`; others standard.
- Notes: Fragment storage for prompt reuse.

### `templates`
- Status `[~]`.
- Subcommands: `list`, `show`, `edit`, `path`, `loaders`.
- Notes: Template filesystem directory management.

### `schemas`
- Status `[~]`.
- Subcommands: `list`, `show`, `dsl`.
- Options: `list -d/--database -q/--query --full --json --nl`; `show -d/--database`; `dsl --multi`.
- Hidden options: `--path` override for `list` and `show`.
- Notes: JSON schema registry and DSL translator.

### `similar`
- Status `[~]`.
- Options: `-i/--input`, `-c/--content`, `--binary`, `-n/--number`, `-p/--plain`, `-d/--database`, `--prefix`, `-h/--help`.
- Notes: Cosine similarity search in collections.

### `tools`
- Status `[~]`.
- Subcommands: `list`.
- Options: `list --json --functions`.
- Notes: Lists registered tools, supports ad-hoc function registration.

### `keys`
- Status `[~]`.
- Subcommands: `list`, `get`, `set`, `path`.
- Options: `set --value`; others standard.
- Notes: Manages `keys.json`.

### `logs`
- Status `[~]`.
- Subcommands: `list`, `backup`, `on`, `off`, `status`, `path`.
- Key options: `list` includes filters `-n`, `-d`, `-m`, `-q`, `-f`, `-T`, `--tools`, `--schema`, `--schema-multi`, `-l`, data extraction flags, `-t`, `-s`, `-u`, `-r`, `-x`, `--xl`, `-c`, `--cid`, `--id-gt`, `--id-gte`, `--json`, `-e`.
- Hidden options: `logs list --path` to target custom DB file.
- Notes: Central log exploration CLI.

### `models`
- Status `[~]`.
- Subcommands: `list`, `default`, `options` (with `list`, `show`, `set`, `clear`).
- Key options: `list --options --async --schemas --tools -q/--query -m/--model`; others recorded above.

### `plugins`
- Status `[~]`.
- Options: `--all`, `--hook`, `-h/--help`.
- Notes: Surfaces plugin metadata.

### `install`
- Status `[~]`.
- Options: `-U/--upgrade`, `-e/--editable`, `--force-reinstall`, `--no-cache-dir`, `--pre`, `-h/--help`.
- Notes: Wrapper around pip for shared environment.

### `uninstall`
- Status `[~]`.
- Options: `-y/--yes`, `-h/--help`.

### `jq`
- Status `[~]`.
- Options: `-m/--model`, `-l/--length`, `-o/--output`, `-s/--silent`, `-v/--verbose`, `-h/--help`.

### Plugin-Provided Commands
- Status `[~]` – snapshot of currently installed plugin CLIs captured; deeper behavior analysis pending.
- `anyscale-endpoints` – subcommand `refresh`; options standard help.
- `gemini` – subcommands `models (--key, --method)`, `files (--key)`.
- `grok` – subcommand `models`.
- `mistral` – subcommands `models`, `refresh`.
- `openai` – subcommand `models (--json, --key)`.
- `openrouter` – subcommands `models (--free, --json)`, `key (--key)`.
- `cmd` – originates via plugin; documented above.
- `jq` – provided by plugin; documented above.
- Next step: inspect plugin packages for additional hidden commands and configuration flags.

## Notes
- Populate behavioral notes, environment variables, and test references per command.
- Identify hidden commands/flags by inspecting source (`llm/cli.py`) once parity matrix skeleton is ready.
