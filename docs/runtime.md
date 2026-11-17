# MMS and GOOSE runtime

The Rust CLI prepares a model and owns a separate `iec61850-runtime` process. The native process
uses libiec61850 for MMS, GOOSE encoding/decoding and integrated retransmission. Starting a simulation
creates real network traffic; it does not automatically set IEC simulation/test flags in packets.

## Build

Native prerequisites: CMake, a C/C++ toolchain, Java, and the official libiec61850 **v1.6.2.1**
source (commit `6e7a603c771d6124cd3cafea7ec38b7e0f2e7aaf`). The CLI uses its
`tools/model_generator/genconfig.jar`. Dependencies are not automatically downloaded.
Offline Cargo commands remain independent of this backend.

The build generates patched upstream compilation units in its build directory: successful
enable-all GOOSE operations dispatch the public GoCB notification, as the upstream MMS write path
already does. This allows startup checks to observe the actual enable result. The supplied source
checkout is unchanged. `native/runtime/goose_events.cmake` records the exact patch and rejects an
unmatched upstream revision. A second Windows-only build-tree patch uses `SO_EXCLUSIVEADDRUSE`
instead of upstream `SO_REUSEADDR` so an occupied MMS endpoint fails rather than sharing another
process's listener. GOOSE and occupied-port integration tests cover these paths; the runtime reports
the integration patches.

On Windows, install Npcap and obtain its SDK. In a dedicated libiec61850 checkout, copy SDK headers
to `third_party/winpcap/Include` and the matching **x64** `wpcap.lib`/`Packet.lib` to
`third_party/winpcap/Lib` (flat directory, as required by upstream CMake). Do not use the x86 libraries.
With MinGW-w64 compiler and `mingw32-make` on PATH:

```powershell
cmake -S native/runtime -B target/native -G "MinGW Makefiles" -DLIBIEC61850_SOURCE_DIR=C:/deps/libiec61850 -DCMAKE_BUILD_TYPE=Release
cmake --build target/native --target iec61850-runtime --parallel
cargo build --locked
```

The adapter adds an existing `%SystemRoot%/System32/Npcap` to the child's PATH. It does not modify
machine PATH, driver installation or adapter settings. Runtime discovery lists stable interface IDs;
Windows resolves the exact ID to the index used by the pinned library immediately before opening it.
Unknown IDs and interfaces without an Ethernet MAC are rejected instead of selecting index zero.

Linux uses the same specification and native sources:

```sh
cmake -S native/runtime -B target/native -DLIBIEC61850_SOURCE_DIR=/path/to/libiec61850 -DCMAKE_BUILD_TYPE=Release
cmake --build target/native --target iec61850-runtime --parallel
cargo build --locked
```

Linux GOOSE requires raw Ethernet access. Grant the appropriate permissions through the test
environment's normal process. The CLI does not escalate privileges. Windows results do not
establish Linux or board success.

## First MMS simulation

Run from the repository root with new output paths. The SCL path in a specification is relative to
the specification file, not the working directory.

```powershell
target/debug/iec61850.exe icd create --spec examples/device.json -o examples/device.icd --references-only --json
target/debug/iec61850.exe runtime --runtime target/native/iec61850-runtime.exe --json
target/debug/iec61850.exe sim check --spec examples/simulation.json --genconfig C:/deps/libiec61850/tools/model_generator/genconfig.jar --runtime target/native/iec61850-runtime.exe --json
target/debug/iec61850.exe sim run --spec examples/simulation.json --genconfig C:/deps/libiec61850/tools/model_generator/genconfig.jar --runtime target/native/iec61850-runtime.exe --duration 60 --events events.jsonl --json
```

While running, use another terminal:

```powershell
target/debug/iec61850.exe mms browse --host 127.0.0.1 --port 8102 --runtime target/native/iec61850-runtime.exe --json
target/debug/iec61850.exe mms read --host 127.0.0.1 --port 8102 --reference DEMOLD0/GGIO1.AnIn1.mag.f --fc MX --runtime target/native/iec61850-runtime.exe --json
```

The example starts at 10.0 and changes to 25.5 after one second. It binds only to localhost.
`--duration` is a required positive number of **seconds**; timeline `at_ms` uses milliseconds.
`--timeout` controls model-generation and startup/request deadlines. Cleanup has a five-second
deadline; a nonresponsive process is terminated and reported as a failure.

## Specification and value contract

See [`examples/simulation.json`](../examples/simulation.json).

| Field | Meaning |
|---|---|
| `schema_version` | `1`; unknown input fields fail |
| `scl`, `ied`, `access_point` | Model path and unambiguous selection |
| `source_sha256` | Optional source pin from `scl inspect` |
| `mms` | Explicit `listen` IP and `port`; omit/null to disable |
| `goose` | Explicit interface and selected controls; omit/null to disable |
| `initial` | List of `{reference, value}` leaf updates |
| `timeline` | Increasing `{at_ms, values}` batches, each before run duration |

Enable at least one service. Both services use the same runtime model. All initial and future
updates are checked before network startup. Each batch holds the library's model lock. Unmentioned
values retain converted model initialization. Quality and timestamps are explicit values; changes
do not invent quality or automatically timestamp business data.

Updates support Boolean, signed/unsigned integers through 32 bits, numeric Enum, FLOAT32/FLOAT64,
visible/Unicode strings within model size, bit strings through 32 bits (including Quality), and
Timestamp as UTC epoch milliseconds. INT64 uses a decimal **string** to preserve precision.
Address individual structure leaves. Unsupported values, whole-array updates, invalid references
and out-of-range numbers fail before startup. These checks are not full NSD/CDC validation.

MMS reads return recursive `{type, value}` objects. Signed integers are decimal strings, bit strings
are bit sequences, octet strings are hexadecimal, and UTC timestamps are epoch milliseconds.
Ordinary scalar MMS data writes are supported as described below. Control/SBO/Operate and Report
subscription clients are not implemented. Server data writes remain denied unless explicitly configured.

## MMS writes

The standalone example [`examples/mms-write.json`](../examples/mms-write.json) uses
[`examples/mms-write.icd`](../examples/mms-write.icd), which includes analog, Boolean and integer
settings for local functional tests. It does not establish a real device's supported services or NSD completeness.

The server's `mms.writable` array contains exact `{reference, fc}` pairs. For example:

```json
{
  "listen": "127.0.0.1",
  "port": 8102,
  "writable": [
    { "reference": "DEMOLD0/GGIO1.Setpoint.setMag.f", "fc": "SP" }
  ]
}
```

This is the `mms` field of a full simulation specification. Missing/empty `writable` preserves the
read-only behavior of earlier configurations. Entries must resolve to supported scalar leaves with
the stated FC. The server supports ordinary writable FCs `SP/SV/CF/DC/BL`; `ST/MX` status/measurement
values are not opened for network writes. Setting-group editing and controls require their separate
protocol workflows. Duplicate, nonexistent, mismatched-FC and unsupported entries fail `sim check`
and prevent startup. Allowing a leaf does not allow writes to its parent structure or siblings.

Start the example server, then use another terminal to write and read back:

```powershell
target/release/iec61850.exe sim run --spec examples/mms-write.json --genconfig C:/deps/libiec61850/tools/model_generator/genconfig.jar --runtime target/release/iec61850-runtime.exe --duration 60 --events writes.jsonl --json
```

```powershell
target/release/iec61850.exe mms write --host 127.0.0.1 --port 8102 --reference DEMOLD0/GGIO1.Setpoint.setMag.f --fc SP --value 25.5 --json
target/release/iec61850.exe mms read --host 127.0.0.1 --port 8102 --reference DEMOLD0/GGIO1.Setpoint.setMag.f --fc SP --json
```

Use `--value` for one JSON scalar, or `--value-file PATH` for a UTF-8 file containing that scalar.
They are mutually exclusive. A string file contains JSON such as `"Written by MMS"`, including the
quotes. INT64 values use decimal strings. The client queries the target's MMS type before encoding
the value and checks its width/range/string bound. Unsupported structures/arrays, incompatible types
and invalid ranges fail without sending a write. Embedded NUL strings are rejected. `FC=CO` is not
accepted as a shortcut for control commands.

The client issues one write, with no automatic retry. `data.acknowledged: true` means the remote MMS
service returned success; it is not an automatic readback or proof of device behavior. Read back
explicitly when needed. A transport timeout or lost acknowledgement can leave the actual write
outcome unknown; reconcile the value before deciding whether another write is appropriate.

The server applies accepted writes to the same model used by MMS reads and GOOSE, using the
library update interface under its existing model lock. `mms_write` events include reference, FC,
peer and applied value. Configured handlers record rejected values as `mms_write_rejected`;
unlisted attributes receive a negative MMS response and produce no applied-write event. A later
timeline step can overwrite a network-written value: the last applied update wins. Stopping the
server joins callbacks before the terminal `result` event.

## GOOSE publisher

Create controls with `scl edit --patch examples/add-goose.json` and point the simulation at that new
ICD. Its `goose` field can be:

```json
{
  "interface": "COPY_AN_EXACT_ID_FROM_RUNTIME",
  "controls": [{
    "ld_inst": "LD0", "name": "gcbStatus",
    "address": {
      "mac": "01-0C-CD-01-00-01", "appid": 4096,
      "vlan_id": 0, "vlan_priority": 4, "vlan_tagged": false,
      "min_time_ms": 100, "max_time_ms": 1000
    }
  }]
}
```

This is a field value, not a complete specification. Set `mms` to null for GOOSE-only operation.
Only selected GoCBs are included in the runtime projection; the source SCL remains unchanged.
The generated DataSet's count and ordered references are checked against the SCL projection.

If `address` is omitted, use the exact matching `Communication/SubNetwork/ConnectedAP/GSE` address
and MinTime/MaxTime. Missing parameters fail. JSON `appid: 4096` means Ethernet APPID `0x1000`, not
the textual GSEControl `appID`/GoID. SCL APPID/VLAN-ID are parsed as hexadecimal. SCL-derived
addresses use VLAN tagging; an override can choose untagged frames. Timings must be whole
milliseconds because this runtime uses millisecond timing.

The standard template has no Communication section. Explicit overrides can provide simulation
addresses without changing that fact. Known missing-Communication/ConnectedAP warnings are
retained in `generator_warnings` after selected addresses are resolved. Other generator warnings
and errors prevent startup; a zero generator exit status alone is insufficient.

## GOOSE subscriber

Subscription JSON:

```json
{
  "schema_version": 1,
  "interface": "COPY_AN_EXACT_ID_FROM_RUNTIME",
  "gocb_ref": "DEMOLD0/LLN0$GO$gcbStatus",
  "mac": "01-0C-CD-01-00-01", "appid": 4096,
  "dataset": "DEMOLD0/LLN0$dsStatus", "conf_rev": 1
}
```

```sh
iec61850 goose subscribe --spec subscription.json --duration 60 --events received.jsonl --runtime /path/to/iec61850-runtime --json
```

The receiver filters control reference/MAC/APPID, checks DataSet/confRev and native validity, and
records typed values, stNum/sqNum, TTL, parse errors and expiry events. It does not yet infer member
types from subscriber SCL or validate ExtRef. No matching traffic is `incomplete`/exit 3. Invalid
frames or TTL expiry are `fail`/exit 1. Successful observation is scoped to that stream and window.

## Lifecycle and evidence

`--json` emits one final schema-version-1 result. The 0.4 CLI uses native protocol version 2 and
streams version-2 JSONL (`ready`, `update`, `goose`, `goose_timeout`, `mms_write`,
`mms_write_rejected`, `error`, `result`) to a new file without overwriting existing output.
The native backend also accepts protocol 1 for older read-only clients, echoing their version.
The 0.4 CLI rejects an older backend so writable configuration cannot silently be ignored.
Native stdout is separated from protocol output. Nonempty backend logs are preserved in results;
known upstream HAL send/filter failures cause execution failure even when the HAL's void API cannot
return the error. Streaming applies backpressure and does not retain an in-memory packet history.

The parent owns the native process and keeps its stdin open. Cancellation or parent death closes
the pipe, causing the event loop to stop and release its model, publishers, receiver and listener.
Ctrl+C produces an incomplete report when orderly shutdown is possible. Crashes, incompatible
protocol messages and missing final results remain errors.

This is a desktop simulation tool, not a hard-real-time or formal conformance test system. GOOSE
retransmission runs in libiec61850; timeline updates use a monotonic millisecond clock. Scheduler,
NIC, driver and output delays affect timing. Publisher startup proves enabled controls, not remote
reception. `sim check` opens no network interfaces and proves neither sending nor receiving.

## Verification

Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --locked -- -D warnings`, and
`cargo test --locked` for ordinary offline checks.

Native value/range serialization tests are also offline:

```sh
cmake --build target/native --target runtime-value-tests
ctest --test-dir target/native -R runtime-values --output-on-failure
```

```powershell
$env:IEC61850_TEST_RUNTIME = (Resolve-Path target/native/iec61850-runtime.exe).Path
$env:IEC61850_TEST_GENCONFIG = 'C:/deps/libiec61850/tools/model_generator/genconfig.jar'
cargo test --locked --test simulation real_mms -- --ignored --nocapture
```

For authorized GOOSE tests, record OS, library revision, driver, exact interface/network, MAC/APPID,
tagging and timers. Start a bounded subscriber and publisher, change both example values, and check
types/order, state/sequence numbers, TTL and cleanup. Record physical-board results separately.

Set `IEC61850_TEST_INTERFACE` to the authorized ID and run
`cargo test --locked --test simulation real_goose -- --ignored --test-threads=1` for the five-second
GOOSE-only and combined MMS/GOOSE cases. Their explicit MinTime/MaxTime are 100/1000 ms. The local
Windows/VMnet1 verification observed TTL expiry with a 4 ms MinTime, so it does not establish
reliable 4 ms operation. The receiver reports these expiries as failures instead of suppressing them.

With the same native dependency environment, run scalar roundtrips, denied writes and the opt-in
shared-GOOSE check with:

```sh
cargo test --locked --test mms_write real_ -- --ignored --test-threads=1
```

The shared-GOOSE check requires `IEC61850_TEST_INTERFACE`; select individual localhost tests when
no Ethernet test is authorized.

## Dependencies and memory

Wrapper source is Apache-2.0. libiec61850 is GPL-3.0-or-later with a commercial licensing option;
the linked native executable needs a compatible distribution arrangement. Process separation does
not itself settle licensing. cJSON 1.7.19 is vendored under MIT with its license alongside it. Npcap
has its own license and is not bundled.

The wrapper is C99. Model, timeline and update values are prepared before running and freed at
shutdown. Per-event JSON and the third-party stack allocate during execution, an explicit desktop
exception to the MCU no-allocation-after-initialization convention. Serialized event objects are
released immediately; the Rust side streams them.
