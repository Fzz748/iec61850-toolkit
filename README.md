# IEC 61850 Toolkit

A Rust CLI for offline SCL inspection, ICD creation, controlled editing, reference checking, and semantic comparison. One Cargo package provides the `iec61850` executable and a reusable library.

One shared Agent Skill supports the CLI workflows in Codex and Claude Code. Online MMS/GOOSE operations, packet analysis, and code generation are not implemented.

## Build

Use Rust 1.86 or newer with the platform's native linker/build tools:

```sh
cargo build --locked
cargo run --locked -- --help
cargo run --locked -- capabilities --json
```

The executable is `target/debug/iec61850` (`iec61850.exe` on Windows). For an optimized executable, use `cargo build --release --locked`.

## Run the example

Run from the repository root; output paths must not already exist:

```sh
cargo run --locked -- icd create --spec examples/device.json -o demo.icd --references-only --json
cargo run --locked -- scl inspect demo.icd --ied DEMO --json
cargo run --locked -- scl edit demo.icd --patch examples/add-goose.json --dry-run --references-only --json
cargo run --locked -- scl edit demo.icd --patch examples/add-goose.json -o demo.updated.icd --references-only --json
cargo run --locked -- scl validate demo.updated.icd --references-only --json
cargo run --locked -- scl diff demo.icd demo.updated.icd --json
```

`--references-only` explicitly selects XML and the implemented reference checks. It does **not** establish XSD, data-model completeness, interoperability, or device support. The result includes `xsd: not_run` with the selected scope.

For XSD validation, provide `xmllint` and a matching local schema bundle:

```sh
cargo run --locked -- scl validate demo.updated.icd --xsd /path/to/SCL2007B4/SCL.xsd --xmllint /path/to/xmllint --json
```

Use the same options on `icd create` or `scl edit` to require XSD validation before saving. A missing validator/schema produces exit code `3` and `incomplete`; a failing required check prevents output. External validation has a configurable timeout (default 10 seconds).

## Supported scope

| Area | Current implementation |
|---|---|
| Input | UTF-8 SCL, optional BOM; 32 MiB and 128-level nesting limits |
| Creation | `minimal-ggio` template, LLN0/LPHD/GGIO, explicit SPS and MV signals |
| Editing | SCL `2007B4`; seven operations listed by `capabilities` |
| Preservation | Local XML changes; untouched text, comments, prefixes and vendor extensions retained |
| Reference checks | Type links/cycles, LN identities, explicit non-array FCDA paths, local control-block/DataSet links |
| Diff | Element/attribute changes, ordered FCDA members, raw vendor-extension changes |
| XSD | Local `xmllint` adapter; schema version/namespace checks; no automatic downloads |

Other SCL revisions can be inspected and compared; editing requires `version="2007" revision="B" release="4"`. The template is a structural starting point, not a declaration of firmware functionality. Full NSD/CDC rules, ExtRef bindings, communication addresses, initial-value editing, and arbitrary data-type editing are outside this version's scope.

See [CLI contracts and JSON inputs](docs/cli.md) for edit examples, validation behavior, exit codes, and limitations. See [architecture](docs/architecture.md) for module ownership.

## Agent Skill

The single [iec61850 Skill](skills/iec61850/SKILL.md) covers model creation/editing and read-only inspection, validation, and comparison. Detailed workflows live inside its `references/` directory so the copied folder is self-contained. Codex UI metadata is in `agents/openai.yaml`; the shared instructions use no host-specific tools or MCP dependencies.

Build/install the CLI separately and make its location available to the agent. Copy the entire `skills/iec61850` folder into **one** appropriate skills directory for each host/project where you want to use it:

| Host | Project location | User location |
|---|---|---|
| Codex | `.agents/skills/iec61850/` | `~/.agents/skills/iec61850/` |
| Claude Code | `.claude/skills/iec61850/` | `~/.claude/skills/iec61850/` |

For example, from this checkout in PowerShell:

```powershell
$skillDestination = '.agents/skills/iec61850' # Use .claude/skills/iec61850 for Claude Code
if (Test-Path -LiteralPath $skillDestination) { throw 'Skill destination already exists; review it before updating.' }
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $skillDestination) | Out-Null
Copy-Item -LiteralPath './skills/iec61850' -Destination $skillDestination -Recurse
```

Keep `skills/iec61850` as the maintained source rather than editing separate host copies. In Codex CLI, invoke `$iec61850`; in Claude Code, invoke `/iec61850`. The description also allows automatic selection for matching file-engineering tasks. Installation does not install the CLI or XSD tooling.

Example request: "Use the iec61850 skill to inspect this ICD, add the specified DataSet to a new file, and report the validation scope and changes." For full XSD checks, provide a matching schema and validator; the Skill reports missing checks instead of silently treating reference-only checks as full validation.

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

The regular suite is offline and includes library and real CLI process tests. `cargo test --locked --test skill_workflows` executes the Skill's bundled JSON examples from an isolated working folder and checks missing-XSD and stale-input failures. This verifies the examples and command contract, not either host's automatic Skill selection.

The real XSD integration test is opt-in because schemas and validators are not bundled. Set `IEC61850_TEST_XSD` and `IEC61850_TEST_XMLLINT` to absolute local paths, then run:

```sh
cargo test --locked --test cli real_xsd -- --ignored
```

That test validates generated and edited models and checks that a schema-only error is rejected. Schema resources and external binaries retain their own licenses and are not part of the source distribution. The local verification cache is ignored by Git.

## License

Project-authored source code, the Agent Skill, documentation, examples, and templates are licensed under [Apache License 2.0](LICENSE). See [NOTICE](NOTICE) for attribution. The standalone Skill folder includes copies of both files so they accompany the Skill when distributed separately.

Third-party dependencies, IEC XSD/NSD resources, and vendor files retain their own licenses and notices. This license does not relicense user-provided project files.
