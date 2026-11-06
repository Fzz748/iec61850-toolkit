# Architecture

The executable exposes `capabilities`, `scl`, and `icd` command groups. The library holds the implementation; command groups do not maintain separate XML models.

## Modules

- `cli`: clap arguments and command definitions.
- `app`: bounded file input, execution, validation selection, report formatting, and saving new files.
- `scl::document`: complete source text and a borrowing XML tree. The source remains authoritative.
- `scl::model` / `query`: logical-node identities, FCDA values, inspection views.
- `scl::edit`: typed operations, semantic preconditions, local byte changes, reparsing after every operation.
- `scl::generate`: an embedded template and escaped, explicit device specifications.
- `scl::validate`: the implemented reference checks and source-located diagnostics.
- `scl::diff`: content snapshots keyed by object identity, with FCDA order retained.
- `adapters::xsd`: a bounded external `xmllint` invocation with a caller-supplied schema.
- `report`: versioned result, check status, and diagnostic structures.

## Mutation flow

Read source/specification → prepare candidate in memory → parse candidate → check references → optionally validate XSD → report changes → save if all requested checks passed.

Preview and execution use the same candidate-building path. The editor never writes files. Application code persists a temporary file in the output directory with no-clobber semantics; an existing destination is an error. No original-file overwrite option is exposed.

A patch may pin the input SHA-256. Operations are sequential and reparsed so subsequent operations never reuse stale offsets. A failed operation or validation prevents persistence of the entire batch. Replacement operations reject content they cannot preserve.

## Extension points

Add new edit operations to the shared SCL engine with reference and preservation tests. Future GOOSE configuration commands should call this engine. Packet analysis, MMS, and code generators can consume SCL queries and reports without adding network or process side effects to the SCL core.

## Agent workflow

`skills/iec61850/SKILL.md` is the single shared entrypoint for Codex and Claude Code. It discovers CLI capabilities, selects a task reference, prepares explicit JSON inputs, and interprets structured results. Its two bundled references cover model authoring and review. Protocol rules and file operations remain in Rust.

The Skill pins edits to the inspected source hash, distinguishes reference-only checks from XSD validation, and preserves the CLI's no-clobber output behavior. It carries no scripts that implement a second SCL engine and no mandatory MCP or model API dependency. Host-specific installation copies the same source folder; there is no runtime plugin system or background daemon.
