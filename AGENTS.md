# Repository Guidelines

## Scope & Architecture

Build an open-source IEC 61850 toolkit: one Rust CLI and one shared `iec61850` Skill for Codex and Claude Code. This repository currently contains a discussion export; the structure below is planned, not implemented.

Start with one Cargo package containing a library and binary. Prioritize ICD creation, controlled editing, validation, and semantic diff. Add GOOSE capture analysis, MMS, and code generation later.

## Project Structure & Module Organization

- `src/cli/`: arguments, subcommands, terminal output.
- `src/app/`: file I/O, workflow execution, validated result persistence.
- `src/scl/`: original XML, semantic indexes, queries, generation, editing, validation, diff. ICD and GOOSE configuration commands share this core.
- `src/report.rs`: structured results and diagnostics; `src/adapters/`: external-tool integration.
- `profiles/`, `templates/`, `examples/`: version/project rules, verified models, reproducible examples.
- `skills/iec61850/SKILL.md`: the single workflow entrypoint; `references/` beneath it holds task guidance.
- `tests/fixtures/`, `tests/cli/`: document samples and CLI tests; `docs/`: project documentation.

## Build, Test, and Development Commands

After introducing `Cargo.toml`, use:

- `cargo build`: compile.
- `cargo run -- --help`: inspect implemented commands.
- `cargo fmt --all -- --check`: verify formatting.
- `cargo clippy --all-targets -- -D warnings`: lint.
- `cargo test`: run unit and integration tests.

Until scaffolding exists, report these checks as `not_run`.

## Coding Style & Editing Contracts

Use rustfmt, four-space indentation, `snake_case` functions/modules, and `UpperCamelCase` types. Keep protocol rules in Rust; the Skill orchestrates commands.

Preserve original XML, namespaces, comments, and vendor extensions; derive semantic indexes from it. Prepare edits in memory, reparse, validate, and diff before saving a new file. Preview and execution share this path. Failures must leave originals intact.

Keep core operations independent of file/process I/O. Provide versioned JSON, stable diagnostic identifiers, explicit exit codes, and logs on stderr.

## Testing Guidelines

Use Rust unit and integration tests with descriptive names, such as `rejects_missing_dataset_reference`. Cover preservation, reference failures, DataSet ordering, deterministic output, and failure-without-write behavior. Register CLI integration suites in Cargo.

Distinguish `pass`, `fail`, and `not_run`; missing schemas cannot imply complete validation. Record external-tool versions and timeouts.

## Commit & Pull Request Guidelines

No Git history exists yet. Prefer scoped imperative messages, such as `feat(scl): add semantic diff`. PRs should explain behavior, link applicable issues, include focused verification, and disclose compatibility limitations. Use public or sanitized fixtures and record third-party resource licenses.
