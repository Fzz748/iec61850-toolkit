# Model creation and controlled editing

Use these workflows after checking the CLI's capabilities. Commands below use `iec61850`; substitute the resolved executable/command prefix. The examples select reference-only validation. Replace that flag with the matching `--xsd PATH` options when required by the task.

## Create an ICD

Establish the IED name, AccessPoint, logical-device instance, and explicit signal-to-template mappings. The current `minimal-ggio` template creates LLN0, LPHD1, and GGIO1. SPS signals use `Ind1`, `Ind2`, etc.; MV signals use `AnIn1`, `AnIn2`, etc. These are structural examples, not an assertion that a device implements the model.

Save this complete example as `device.json`, adapting its explicit mappings to the user's requirements:

```json
{
  "schema_version": 1,
  "template": "minimal-ggio",
  "ied_name": "SKILL_DEMO",
  "access_point": "AP1",
  "ld_inst": "LD0",
  "description": "Explicit offline model example",
  "signals": [
    {"name": "Ind1", "kind": "sps", "description": "Example status"},
    {"name": "AnIn1", "kind": "mv", "description": "Example measurement"}
  ]
}
```

IED names start with an ASCII letter and are at most 64 characters; `None` is reserved. AccessPoint names use the same identifier subset with a 32-character limit. Logical-device instances allow an initial digit and are limited to 64 characters. Signal names are limited to 12 characters. Resolve unsupported model requests explicitly instead of silently converting them to this template.

```sh
iec61850 icd create --spec device.json --dry-run --references-only --json
iec61850 icd create --spec device.json -o device.icd --references-only --json
```

Inspect the preview result before saving. If the preview fails, correct the specification or report the missing prerequisite. A preview-only task stops before the second command. For a completed save, check `written`, the output path, and that `candidate_sha256` matches the preview for the same inputs/options.

## Edit an existing document

1. Run `iec61850 scl inspect device.icd --json`. Check `scl_version`, the actual logical-node target, and any relevant DataSet/control details. Editing currently requires SCL `2007B4`.
2. Prepare a patch from supported operations. Add **`source_sha256` from the inspection's `data.source_sha256`** at the patch's top level. The example below omits that input-dependent value; populate it from the actual file before execution.
3. Preview with `--dry-run`; inspect checks, diagnostics, and `data.changes`. Confirm that changed objects and DataSet member order match the request.
4. Unless the user requested a review gate or preview only, execute the same patch/options to a new output path. Check `written` and compare the returned candidate hash to the preview. A hash-precondition failure requires re-inspection and reconsideration of the patch; do not remove the precondition to force the old patch through.
5. Use the returned checks and changes in the final report. Run a separate `scl diff` only for an additional comparison requested by the user.

For the generated example, this patch creates a two-member DataSet and a GOOSE control block. Save it as `changes.json` and add the inspected source hash:

```json
{
  "schema_version": 1,
  "operations": [
    {
      "op": "add_dataset",
      "target": {"ied": "SKILL_DEMO", "access_point": "AP1", "ld_inst": "LD0", "ln_class": "LLN0"},
      "name": "dsStatus",
      "members": [
        {"ld_inst": "LD0", "ln_class": "GGIO", "ln_inst": "1", "do_name": "Ind1", "da_name": "stVal", "fc": "ST"},
        {"ld_inst": "LD0", "ln_class": "GGIO", "ln_inst": "1", "do_name": "AnIn1", "da_name": "mag.f", "fc": "MX"}
      ]
    },
    {
      "op": "add_goose",
      "target": {"ied": "SKILL_DEMO", "access_point": "AP1", "ld_inst": "LD0", "ln_class": "LLN0"},
      "name": "gcbStatus",
      "dataset": "dsStatus",
      "go_id": "SkillStatus"
    }
  ]
}
```

```sh
iec61850 scl edit device.icd --patch changes.json --dry-run --references-only --json
iec61850 scl edit device.icd --patch changes.json -o device.updated.icd --references-only --json
```

Logical-node targets include the AccessPoint as well as IED/LD. `prefix` and `ln_inst` default to empty strings; LLN0's instance is empty, while GGIO1 uses `ln_inst: "1"`. A new LN must reference an existing LNodeType.

## Supported patch operations

Every operation has `op` and `target`. The additional fields are:

| `op` | Fields | Effect |
|---|---|---|
| `set_description` | `description` | Set the target LN's description |
| `add_logical_node` | `ln_type` | Add the new target LN using an existing type |
| `add_dataset` | `name`, `members` | Add a new DataSet |
| `replace_dataset` | `name`, `members` | Replace ordered members |
| `remove_dataset` | `name` | Remove a DataSet with no local control references |
| `add_goose` | `name`, `dataset`, `go_id` | Add a GSEControl under LN0 |
| `set_goose_dataset` | `name`, `dataset` | Change a GSEControl's local DataSet |

Use 1–256 operations per patch. Unknown JSON fields fail. FCDA members require explicit `ld_inst`, `ln_class`, `do_name`, and `fc`; `prefix`/`ln_inst` default to empty and `da_name` is optional. Omitting `da_name` selects a whole data object with that FC. Dotted names traverse SDO/BDA structures, such as `mag.f`. Array-indexed and wildcard FCDA forms are outside the current checker’s scope.

DataSet/control names start with a letter and contain at most 32 ASCII identifier characters. New LN prefixes are at most 11 characters; instances contain 1–12 digits. `go_id` is 1–129 printable ASCII characters and maps to **GSEControl `appID`**, not the hexadecimal Ethernet APPID in Communication. MAC/VLAN/communication APPID and ExtRef binding edits are unavailable.

An actual DataSet replacement, including a reorder, increments each referencing local GSEControl's `confRev`; replacing identical members is a no-op. The CLI also increments `confRev` when changing the control's DataSet. Do not add a second manual increment. Overflow, unsupported non-GOOSE references, or replacement that would discard extensions/comments/descriptions causes rejection. Explain that restriction rather than stripping content to make the operation succeed.
