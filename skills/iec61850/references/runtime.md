# Runtime workflow

Use for virtual IED simulation, MMS browse/read/write or GOOSE subscription. Offline model authoring
does not require native tools.

1. Inspect `capabilities --json`. Resolve `iec61850-runtime`; for simulations also resolve Java
   and the matching libiec61850 `genconfig.jar`. Run `runtime --runtime PATH --json` to identify
   interfaces and library version. Missing dependencies remain errors.
2. Establish the model, IED/AP and actual target. MMS simulation needs a listen IP/port; GOOSE
   needs an exact interface ID and control/address selection. Reuse existing authorization.
   If a target network's scope is unknown, prepare and check the specification before resolving
   authorization for traffic. An offline-file task does not authorize transmission.
3. Prepare UTF-8 JSON and run `sim check`. `scl` paths are relative to the specification file.
   Optional `source_sha256` pins the model. Inspect warnings and value checks; successful checking
   does not open interfaces or establish communication.
4. Run with explicit `--duration SECONDS` and a new `--events FILE.jsonl`. Keep the process owned
   by the task. Wait for `ready` before dependent client operations. Stop or await the bounded
   session and collect its result and receiver evidence.

Command shapes:

```sh
iec61850 sim check --spec simulation.json --genconfig /path/to/genconfig.jar --runtime /path/to/iec61850-runtime --json
iec61850 sim run --spec simulation.json --genconfig /path/to/genconfig.jar --runtime /path/to/iec61850-runtime --duration 30 --events session.jsonl --json
iec61850 mms browse --host 127.0.0.1 --port 8102 --runtime /path/to/iec61850-runtime --json
iec61850 mms read --host 127.0.0.1 --port 8102 --reference DEMOLD0/GGIO1.AnIn1.mag.f --fc MX --runtime /path/to/iec61850-runtime --json
iec61850 goose subscribe --spec subscription.json --duration 30 --events received.jsonl --runtime /path/to/iec61850-runtime --json
```

Minimal MMS specification for a model with the named FLOAT32 attribute:

```json
{
  "schema_version": 1,
  "scl": "device.icd",
  "ied": "DEMO",
  "access_point": "AP1",
  "mms": { "listen": "127.0.0.1", "port": 8102 },
  "initial": [{ "reference": "DEMOLD0/GGIO1.AnIn1.mag.f", "value": 10.0 }],
  "timeline": [{ "at_ms": 1000, "values": [
    { "reference": "DEMOLD0/GGIO1.AnIn1.mag.f", "value": 25.5 }
  ] }]
}
```

GOOSE adds `goose: {interface, controls: [{ld_inst, name, address?}]}`. Without `address`, the exact
Communication/GSE entry must provide the parameters. An override has `mac`, numeric `appid`,
`vlan_id`, `vlan_priority`, `vlan_tagged`, `min_time_ms`, `max_time_ms`. Ethernet APPID differs
from textual GSEControl appID/GoID. Missing addresses are not guessed.

A subscription has `schema_version: 1`, `interface`, `gocb_ref`, `mac`, numeric `appid`, `dataset`,
and `conf_rev`. Unknown fields fail. Windows interface IDs come from `runtime`; Linux uses interface
names. Simulated publishers send real Ethernet frames.

Updates address scalar leaves. INT64 uses decimal strings; Timestamp uses epoch milliseconds and
bit strings/Quality use integer masks. Quality and time are not invented automatically. Unsupported
values fail before startup. MMS control/SBO/Operate and Report subscription clients are not exposed.

For ordinary MMS writes, use `mms write --host HOST --port PORT --reference REF --fc FC --value JSON`
or `--value-file PATH` for one JSON scalar. Query/validate the actual writable point and reuse existing
authorization for that target and action; implementing/testing a client is not permission to write to
an unspecified physical device. The command queries the remote type and sends one write with no retry.
`data.acknowledged` confirms the protocol response, not readback or physical behavior. Lost responses
or timeouts can leave the write outcome unknown; reconcile before retrying.

To receive writes in a simulator, configure `mms.writable` as exact `{reference, fc}` entries with
`SP/SV/CF/DC/BL`. Unlisted attributes remain denied. `sim check` validates references and leaf types.
`mms_write` records the actual applied value; timeline updates can subsequently overwrite it.
Use an explicit readback and, when relevant, receiver evidence to verify the intended change.
The 0.4 CLI needs native protocol 2; rebuild/select the matching backend on a protocol mismatch.

`--json` remains one final object; events go to JSONL. Exit 2 covers native, driver and execution
failures. No matching frames is `incomplete`/exit 3; invalid frames or TTL expiry is `fail`/exit 1.
Cancellation is incomplete. Publisher startup is not receiver evidence. Report actual observed
values, metadata and tested window; omitted network, Linux or board checks remain `not_run`.
