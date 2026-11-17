# Architecture

The executable exposes `capabilities`, `scl`, `icd`, `sim`, `mms`, `goose`, and `runtime` commands. The library holds the implementation; command groups do not maintain separate authoritative XML models.

## Modules

- `cli`: clap arguments and command definitions.
- `app`: file input, execution, validation selection, report formatting, and saving new files.
- `scl::document`: complete source text and a borrowing XML tree. The source remains authoritative.
- `scl::model` / `query`: logical-node identities, FCDA values, inspection views.
- `scl::edit`: typed operations, semantic preconditions, local byte changes, reparsing after every operation.
- `scl::generate`: an embedded template and escaped, explicit device specifications.
- `scl::validate`: the implemented reference checks and source-located diagnostics.
- `scl::diff`: content snapshots keyed by object identity, with FCDA order retained.
- `adapters::xsd`: a bounded external `xmllint` invocation with a caller-supplied schema.
- `report`: versioned result, check status, and diagnostic structures.
- `simulation`: strict specification types and timeline/address checks.
- `scl::runtime`: pure projections of IED/AP, selected controls and ordered DataSet members.
- `app::simulation`: source snapshots, model preparation, session orchestration and reporting.
- `adapters::model_generator`: bounded genconfig execution and explicit warning/error interpretation.
- `adapters::runtime`: subprocess protocol, cancellation, deadlines and streamed event persistence.
- `native/runtime`: C/libiec61850 model loading, typed updates, MMS client/server and GOOSE pub/sub.

## Mutation flow

Read source/specification → prepare candidate in memory → parse candidate → perform explicitly requested reference/XSD checks → report changes → save if the operation and requested checks passed.

Preview and execution use the same candidate-building path. The editor never writes files. Application code persists a temporary file in the output directory with no-clobber semantics; an existing destination is an error. No original-file overwrite option is exposed.

A patch may pin the input SHA-256. Operations are sequential and reparsed so subsequent operations never reuse stale offsets. A failed operation or explicitly requested validation prevents persistence of the entire batch. DataSet replacement edits FCDA nodes while retaining its enclosing metadata and unrelated content. Matching members move intact; changed members retain unknown attributes.

Editing is selected by the SCL namespace and the requested operation's structure, not an edition whitelist. Whole-document validation is opt-in for mutations so unrelated existing errors do not prevent ordinary local editing. `scl validate` performs reference checks by default. The XSD adapter delegates schema/version constraints to xmllint rather than requiring schema metadata labels to equal document attributes.

## Extension points

Add new edit operations to the shared SCL engine with reference and preservation tests. GOOSE configuration commands should call this engine. Runtime preparation consumes SCL projections without adding network or process side effects to the SCL core. See [runtime contracts](runtime.md) for model conversion and native process ownership.

## Agent workflow

`skills/iec61850/SKILL.md` is the single shared entrypoint for Codex and Claude Code. It discovers CLI capabilities, selects a task reference, prepares explicit JSON inputs, and interprets structured results. Its references cover model authoring, review, and runtime sessions. Engineering rules remain in Rust; native protocol execution belongs to the libiec61850 adapter.

The Skill pins edits to the inspected source hash, distinguishes reference-only checks from XSD validation, and preserves the CLI's no-clobber output behavior. It carries no second SCL engine and no mandatory MCP or model API dependency. Runtime sessions use an explicitly selected, task-owned child process; there is no persistent background daemon.
