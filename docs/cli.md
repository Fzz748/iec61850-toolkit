# CLI contracts

`iec61850 capabilities --json` lists the implemented operations and limits. `--json` is accepted before or after subcommands. Business results and argument/input errors emit one JSON object on stdout; operational logging uses stderr. `--help` and `--version` retain normal CLI output.

## Results and exit codes

Results contain `schema_version: 1`, `tool_version`, `command`, `status`, `scope`, `data`, `checks`, and `diagnostics`. Reference diagnostics have stable rule codes, an object path, and one-based line/column. Execution errors may have no source location.

| Exit | Meaning |
|---|---|
| `0` | The requested operation/check scope completed successfully |
| `1` | An executed validation check failed |
| `2` | Invalid arguments/input, rejected edit preconditions, or execution/I/O failure |
| `3` | A required check could not run; no mutation is saved |

`scl diff` returns `0` for a successful comparison even when differences exist. Read `data.changes` to detect them.

For create, edit, and validate, choose `--references-only` or `--xsd PATH`. Omitting both returns `incomplete`. Only the explicitly selected checks determine success; `pass` never means IEC conformance certification. The XSD root must declare the SCL target namespace and a version matching the document, for example `2007B4`. Keep its includes/imports available locally.

Use `--xmllint PATH` for a validator outside PATH and `--xsd-timeout SECONDS` (1–60) to bound it. Missing/broken validators, incompatible schemas, and timeouts are `not_run`, not successful validation. Only xmllint's schema-invalid exit code is classified as a failed XSD check.

## Create an ICD

`icd create --spec DEVICE.json -o NEW.icd --references-only`

See [`examples/device.json`](../examples/device.json). The only template is `minimal-ggio`. Provide an IED name, AccessPoint, logical-device instance, and a nonempty list of explicit signals. SPS signals use `Ind1`, `Ind2`, etc.; MV signals use `AnIn1`, `AnIn2`, etc. They belong to GGIO1. The template includes LLN0 and LPHD1 and is compiled into the executable.

Generation does not infer business-to-IEC mappings, units, control behavior, or firmware services. Confirm these independently for a real device.

Generated IED names use 1–64 ASCII identifier characters starting with a letter; `None` is reserved. AccessPoint names use the same subset with a 32-character limit. Logical-device instances may start with a digit and are limited to 64 characters. Signal names are limited to 12 characters.

## Edit a document

`scl edit INPUT.icd --patch PATCH.json -o NEW.icd --references-only`

Both create and edit support `--dry-run` without `-o`. All saves require a new output path. There is no force/in-place option.

A patch contains `schema_version: 1`, optional `source_sha256`, and 1–256 operations. Unknown JSON fields are rejected. `source_sha256` comes from `scl inspect` and rejects application to a different input.

Logical-node targets always specify `ied`, `access_point`, `ld_inst`, and `ln_class`. `prefix` and `ln_inst` default to empty strings. LLN0 has empty `ln_inst`; GGIO1 uses `ln_class: "GGIO", ln_inst: "1"`. Missing or ambiguous targets fail.

New DataSet/control names are ASCII identifiers starting with a letter, at most 32 characters. A new LN has an optional prefix of at most 11 characters and a numeric instance of 1–12 digits. `go_id` accepts 1–129 printable ASCII characters.

| Operation (`op`) | Additional fields | Behavior |
|---|---|---|
| `set_description` | `target`, `description` | Set the LN's `desc`, preserving other content |
| `add_logical_node` | new `target`, `ln_type` | Add an LN referencing an existing LNodeType |
| `add_dataset` | `target`, `name`, `members` | Add a previously absent DataSet |
| `replace_dataset` | `target`, `name`, `members` | Replace ordered members; update referencing GOOSE revisions |
| `remove_dataset` | `target`, `name` | Remove a DataSet only when no local control references it |
| `add_goose` | `target`, `name`, `dataset`, `go_id` | Add GSEControl in LN0 with `confRev=1` |
| `set_goose_dataset` | `target`, `name`, `dataset` | Change a GSEControl's local DataSet and increment `confRev` |

Example description change:

```json
{
  "schema_version": 1,
  "operations": [{
    "op": "set_description",
    "target": {"ied": "DEMO", "access_point": "AP1", "ld_inst": "LD0", "ln_class": "GGIO", "ln_inst": "1"},
    "description": "Measured status signals"
  }]
}
```

Each `members` entry contains `ld_inst`, `ln_class`, `do_name`, `fc`, optional `prefix`/`ln_inst`, and optional `da_name`. Dotted paths traverse SDO/BDA structures, for example `mag.f`. Omitting `da_name` selects a whole data object with the specified FC. Explicit identities are required; wildcard/all-LN and array-indexed FCDA forms are outside this checker’s scope.

See [`examples/add-goose.json`](../examples/add-goose.json) for a complete batch. `go_id` becomes **GSEControl `appID`**, a textual identifier. It is not the hexadecimal Ethernet APPID under Communication. Network address configuration is not implemented.

Replacing identical members is a no-op. An actual replacement increments each referencing local GSEControl's `confRev`, including an ordering-only change. Overflow fails. Replacement is rejected when a non-GOOSE control references the DataSet or when replacement would discard descriptions, namespace declarations, comments, or extension content. Multiple operations form one all-or-nothing output transaction.

## Inspect and compare

`scl inspect FILE --ied NAME --json` filters model inspection to an IED. Inspection is not validation. Types are listed from the document's shared DataTypeTemplates.

`scl diff BEFORE AFTER --json` compares element attributes and text. Attribute ordering and ordinary comments/formatting are ignored; FCDA order is significant. Private and foreign-namespace subtrees are compared as raw XML so changes to uninterpreted content remain visible. `source_changed` also exposes byte changes that have no semantic diff. It does not perform an automatic rename match or determine runtime compatibility.
