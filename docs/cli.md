# CLI contracts

`iec61850 capabilities --json` lists the implemented operations and limits. `--json` is accepted before or after subcommands. Business results and argument/input errors emit one JSON object on stdout; operational logging uses stderr. `--help` and `--version` retain normal CLI output.

Optional `sim`, `mms`, `goose`, and `runtime` commands use the separately built native backend.
See [runtime contracts and examples](runtime.md). Runtime event streaming uses a separate JSONL
file; it does not change `--json`. For subscriptions exit 1 also covers invalid observed traffic,
and exit 3 covers no observations or cancellation. Operational/backend failures remain exit 2.

## Results and exit codes

Results contain `schema_version: 1`, `tool_version`, `command`, `status`, `scope`, `data`, `checks`, and `diagnostics`. Reference diagnostics have stable rule codes, an object path, and one-based line/column. Execution errors may have no source location.

| Exit | Meaning |
|---|---|
| `0` | The requested operation/check scope completed successfully |
| `1` | An executed validation check failed |
| `2` | Invalid arguments/input, rejected edit preconditions, or execution/I/O failure |
| `3` | A required check could not run; no mutation is saved |

`scl diff` returns `0` for a successful comparison even when differences exist. Read `data.changes` to detect them.

Creation and editing default to XML parsing and operation-specific prerequisites. Add `--references-only` or `--xsd PATH` when validation must pass before saving. `scl validate` performs reference checks by default; `--xsd` adds schema validation. Results identify the actual scope. Supply the intended schema with its local includes/imports. Its declarations govern validation; descriptive labels such as `2007B.2014-01-22` are not an editing or validation whitelist.

Use `--xmllint PATH` for a validator outside PATH and `--xsd-timeout SECONDS` (1–60) to bound it. Missing/broken validators, incompatible schemas, and timeouts are `not_run`, not successful validation. Only xmllint's schema-invalid exit code is classified as a failed XSD check.

## Create an ICD

`icd create --spec DEVICE.json -o NEW.icd --references-only`

See [`examples/device.json`](../examples/device.json). The bundled template is `minimal-ggio`. Provide an IED name, AccessPoint, logical-device instance, and explicit signals with SPS or MV types and user-selected names. They belong to GGIO1. The template includes LLN0 and LPHD1 and is compiled into the executable. An empty signal list creates the base model.

Generation does not infer business-to-IEC mappings, units, control behavior, or firmware services. Confirm these independently for a real device.

Identifiers must be nonempty and signal names distinct from each other and existing template objects. Schema-specific lexical and length constraints are checked by XSD when requested.

## Edit a document

`scl edit INPUT.icd --patch PATCH.json -o NEW.icd --references-only`

Both create and edit support `--dry-run` without `-o`. All saves require a new output path. There is no force/in-place option.

A patch contains `schema_version: 1`, optional `source_sha256`, and an ordered operations array without a fixed count limit. An empty array is a no-op. Unknown JSON fields are rejected. `source_sha256` comes from `scl inspect` and rejects application to a different input. SCL version attributes are preserved; there is no editing version whitelist.

Logical-node targets always specify `ied`, `access_point`, `ld_inst`, and `ln_class`. `prefix` and `ln_inst` default to empty strings. LLN0 has empty `ln_inst`; GGIO1 uses `ln_class: "GGIO", ln_inst: "1"`. Missing or ambiguous targets fail.

Object names must be nonempty and unambiguous. A new LN must reference an existing LNodeType with the same class. Schema-specific naming and length restrictions are left to explicitly requested validation.

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

Each `members` entry contains `ld_inst`, `ln_class`, `do_name`, `fc`, optional `prefix`/`ln_inst`, and optional `da_name`. Paths traverse the actual DO/SDO/DA/BDA type tree, including SDO suffixes in `da_name` such as `phsA.cVal.mag.f`. Omitting `da_name` selects a whole data object with the specified FC. Explicit identities are required for reference checks; wildcard/all-LN and array-indexed forms are not yet checked.

See [`examples/add-goose.json`](../examples/add-goose.json) for a complete batch. `go_id` becomes **GSEControl `appID`**, a textual identifier. It is not the hexadecimal Ethernet APPID under Communication. Network address configuration is not implemented.

Replacing identical members is a no-op. An actual replacement increments `confRev` on referencing local GSEControl, ReportControl, and SampledValueControl elements, including an ordering-only change. Overflow fails. DataSet descriptions, namespaces, comments, Private elements, and unrelated content remain intact. Matching FCDA members move intact; changed members retain unknown attributes. Explicitly removed members are removed with their attached content. Multiple operations form one output transaction; whole-document validation is required only when requested by flags.

## Inspect and compare

`scl inspect FILE --ied NAME --json` filters model inspection to an IED. Inspection is not validation. Types are listed from the document's shared DataTypeTemplates.

`scl diff BEFORE AFTER --json` compares element attributes and text. Attribute ordering and ordinary comments/formatting are ignored; FCDA order is significant. Private and foreign-namespace subtrees are compared as raw XML so changes to uninterpreted content remain visible. `source_changed` also exposes byte changes that have no semantic diff. It does not perform an automatic rename match or determine runtime compatibility.
