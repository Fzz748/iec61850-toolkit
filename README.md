# IEC 61850 Toolkit

**English** | [简体中文](README.zh-CN.md)

A command-line toolkit for **SCL engineering and MMS / GOOSE simulation**. Create, edit, validate, and compare ICD files with a Rust CLI, run virtual IEDs through a native libiec61850 backend, and use one shared Agent Skill in Codex or Claude Code.

**v0.4.0** · Rust 1.86+ · Windows / Linux build paths · [Project license](LICENSE)

[Quick start](#quick-start) · [MMS write example](#mms-write-example) · [Agent Skill](#agent-skill) · [Documentation](#documentation)

## Features

| Workflow | Commands and capabilities |
|---|---|
| Create ICD files | `icd create`: define an IED and SPS / MV signals in JSON using the built-in `minimal-ggio` template |
| Inspect models | `scl inspect`: inspect IEDs, logical nodes, types, and DataSets |
| Controlled editing | `scl edit`: change descriptions, add logical nodes, manage DataSets, and add or update GOOSE control blocks |
| Validate and compare | `scl validate` checks XML and supported references, with optional local XSD validation; `scl diff` compares models and ordered DataSet members |
| Simulate an IED | `sim check` checks the runtime model; `sim run` runs MMS, GOOSE, or both against a shared model with initial values and a timeline |
| MMS client | `mms browse`, `mms read`, and `mms write`: browse, read, and write scalar attributes |
| MMS server | Configure `mms.writable` to accept writes to selected attributes and update the shared model; unlisted attributes remain denied |
| GOOSE communication | Publish selected controls from the simulator; use `goose subscribe` to record values, state/sequence numbers, TTL, and timeout events |
| Automation | Query `capabilities --json`; commands return versioned JSON, while runtime events can stream to a separate JSONL file |

Edits preserve untouched XML, namespaces, comments, and vendor extensions. Preview and save use the same processing path. Changing DataSet member order updates the revisions of referencing local control blocks. Output paths must be new; failures leave original files intact.

## Quick start

Install Rust 1.86+ and the native linker tools for your platform. On Windows, you can use the MSVC toolchain with Visual Studio Build Tools. Build from source:

```sh
git clone https://github.com/Fzz748/iec61850-toolkit.git
cd iec61850-toolkit
cargo build --release --locked
cargo run --locked -- --help
cargo run --locked -- capabilities --json
```

The optimized CLI is `target/release/iec61850.exe` on Windows or `target/release/iec61850` on Linux. SCL file operations do not require the native communication backend.

### Create, edit, and compare ICD files

Run from the repository root. The output files below must not already exist:

```sh
cargo run --locked -- icd create --spec examples/device.json -o demo.icd --references-only --json
cargo run --locked -- scl inspect demo.icd --ied DEMO --json
cargo run --locked -- scl edit demo.icd --patch examples/add-goose.json --dry-run --references-only --json
cargo run --locked -- scl edit demo.icd --patch examples/add-goose.json -o demo.updated.icd --references-only --json
cargo run --locked -- scl validate demo.updated.icd --references-only --json
cargo run --locked -- scl diff demo.icd demo.updated.icd --json
```

This workflow generates an example model, previews and saves DataSet / GOOSE configuration, then reports the validation scope and model differences. See the [device specification](examples/device.json) and [edit patch](examples/add-goose.json) for the input formats.

> [!NOTE]
> Creation and editing check XML and operation prerequisites by default. Add `--references-only` to require reference checks before saving. XSD validation requires a matching local schema and `xmllint`, for example `--xsd /path/to/SCL.xsd --xmllint /path/to/xmllint`. Missing dependencies produce `not_run` / `incomplete`, not successful validation. The editor preserves existing SCL version attributes and has no editing version whitelist.

## MMS write example

This example demonstrates both a **writable simulated MMS server** and an **MMS client write**. The server listens only on `127.0.0.1:8102`.

First follow the [native backend build instructions](docs/runtime.md#build) to prepare CMake, a C/C++ toolchain, Java, and libiec61850 **v1.6.2.1**. The Windows backend also needs Npcap and its SDK. The commands below assume the backend is built at `target/native/iec61850-runtime.exe` and libiec61850 is checked out at `C:/deps/libiec61850`; adjust these paths for your environment.

Terminal 1 (PowerShell): check the model, then run the simulated server for 60 seconds.

```powershell
$runtime = 'target/native/iec61850-runtime.exe'
$genconfig = 'C:/deps/libiec61850/tools/model_generator/genconfig.jar'

target/release/iec61850.exe sim check --spec examples/mms-write.json --genconfig $genconfig --runtime $runtime --json
target/release/iec61850.exe sim run --spec examples/mms-write.json --genconfig $genconfig --runtime $runtime --duration 60 --events writes.jsonl --json
```

Terminal 2 (PowerShell): while the server is running, change the setpoint from `10.0` to `25.5`, then read it back.

```powershell
$runtime = 'target/native/iec61850-runtime.exe'
$reference = 'DEMOLD0/GGIO1.Setpoint.setMag.f'

target/release/iec61850.exe mms write --host 127.0.0.1 --port 8102 --reference $reference --fc SP --value 25.5 --runtime $runtime --json
target/release/iec61850.exe mms read --host 127.0.0.1 --port 8102 --reference $reference --fc SP --runtime $runtime --json
```

Run both terminals from the repository root and use a new `writes.jsonl` path. The write result should contain `data.acknowledged: true`, the read result's `data.value.value` should be `25.5`, and the server event file should record one `mms_write` event.

The [example configuration](examples/mms-write.json) uses the bundled [ICD](examples/mms-write.icd) and lists writable attributes in `mms.writable`:

```json
"writable": [
  { "reference": "DEMOLD0/GGIO1.Setpoint.setMag.f", "fc": "SP" }
]
```

This is a fragment inside the `mms` object. The server can allow scalar leaves with FC `SP/SV/CF/DC/BL`. The client validates values against the remote type. Use `--value-file` for a file containing one JSON scalar, especially strings and INT64 values. See the [write contract](docs/runtime.md#mms-writes).

> [!IMPORTANT]
> MMS writes modify the target model. An acknowledgement confirms only the service response; a timeout can leave the outcome unknown, and the CLI does not automatically retry. Simulation generates real network traffic and does not automatically set IEC simulation/test flags. GOOSE communication requires an explicitly confirmed test interface and network parameters.

### GOOSE communication

Use `runtime --runtime PATH --json` to discover interface IDs. Configure the interface, controls, and MAC / APPID / VLAN / MinTime / MaxTime in the simulation specification. `sim run` publishes and `goose subscribe` receives; both require an explicit duration. MMS writes and timeline updates use the same model, and changes to associated DataSets can trigger GOOSE updates.

See the [publisher example](docs/runtime.md#goose-publisher) and [subscriber example](docs/runtime.md#goose-subscriber) for complete configurations and commands. `sim check` opens no network interfaces and cannot establish actual sending or reception.

## Agent Skill

The repository maintains one [iec61850 Skill](skills/iec61850/SKILL.md). The Skill orchestrates CLI workflows; protocol and model rules live in code. Build the CLI, then copy the entire `skills/iec61850` folder to your chosen host's skills directory:

| Host | Project installation | User installation |
|---|---|---|
| Codex | `.agents/skills/iec61850/` | `~/.agents/skills/iec61850/` |
| Claude Code | `.claude/skills/iec61850/` | `~/.claude/skills/iec61850/` |

Choose one installation location and keep the `references/` directory, metadata, and license files together. Installing the Skill does not install the CLI, native backend, or XSD resources. Make the CLI location available when invoking it.

For example, provide the CLI path and input file, then ask:

> Use the iec61850 skill to inspect this ICD, preview adding the specified DataSet, save to a new file, and report the validation scope actually checked.

## Verification and current limits

Run the ordinary checks with:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

v0.4.0 has passed **40 ordinary tests, 9 native integration tests, and 1 native value test** on Windows. Coverage includes write acknowledgement and readback, type/range rejection, default write protection, process cleanup, and MMS-written values observed through GOOSE on VMnet1. Native network tests are ignored by default; configure their dependencies and test interface before running them separately as described in the [verification guide](docs/runtime.md#verification).

| Area | Current limits |
|---|---|
| Platforms | Windows has been tested locally. Linux build paths are retained; Linux execution and physical IED verification are `not_run` |
| MMS | Ordinary scalar reads/writes are supported. Whole-structure/array writes, SBO/Operate controls, setting-group workflows, and Report subscription clients are not implemented |
| GOOSE | Layer 2 publishing and subscription are supported; capture-file analysis is not implemented. Windows / VMnet1 passed with MinTime 100 ms and MaxTime 1000 ms; a 4 ms MinTime previously produced TTL expiry |
| SCL | Built-in checks cover supported references. Full NSD/CDC and ExtRef validation, network-address editing, and general data-type editing are not implemented |
| Code generation | C code generation is not implemented; Java `genconfig` is used only to prepare the simulator's runtime model |

`--json` emits one final result object; `--events` writes events to a separate JSONL file. Exit code `0` means the requested scope passed, `1` indicates validation or observation failure, `2` indicates an argument, execution, or I/O error, and `3` means incomplete. See the [CLI contracts](docs/cli.md) for details.

## Documentation

| Resource | Contents |
|---|---|
| [CLI contracts](docs/cli.md) | JSON inputs/outputs, edit operations, validation scope, and exit codes |
| [MMS / GOOSE runtime guide](docs/runtime.md) | Windows / Linux builds, dependencies, simulation specifications, network verification, and lifecycle |
| [Architecture](docs/architecture.md) | Rust modules, the native backend, and the file-editing workflow |
| [Examples](examples/) | Device specifications, edit patches, timelines, and a writable MMS model |
| [Shared Skill](skills/iec61850/SKILL.md) | Model engineering, file review, and communication simulation workflows |

Project-authored code, documentation, and the Skill use [Apache-2.0](LICENSE); see [NOTICE](NOTICE) for attribution. The native backend links libiec61850 under GPL-3.0-or-later, with a commercial licensing option. cJSON uses MIT; Npcap has its own license and is not bundled. Account for these dependency licenses when distributing the backend; see [dependency details](docs/runtime.md#dependencies-and-memory).
