# IEC 61850 Toolkit

用于 **SCL 文件工程与 MMS / GOOSE 通信模拟**的命令行工具。通过 Rust CLI 创建、编辑、校验和比较 ICD，通过 libiec61850 原生后端运行虚拟 IED；同一套 Agent Skill 可供 Codex 和 Claude Code 使用。

**v0.4.0** · Rust 1.86+ · Windows / Linux 构建路径 · [项目许可证](LICENSE)

[快速开始](#快速开始) · [MMS 写入示例](#mms-写入示例) · [Agent Skill](#agent-skill) · [文档](#文档)

## 能做什么

| 场景 | 命令 / 能力 |
|---|---|
| 创建 ICD | `icd create`：通过 JSON 定义 IED 和 SPS / MV 信号，使用内置 `minimal-ggio` 模板生成模型 |
| 查看模型 | `scl inspect`：查看 IED、逻辑节点、类型和 DataSet 等信息 |
| 受控编辑 | `scl edit`：修改描述，添加逻辑节点，增删改 DataSet，添加和调整 GOOSE 控制块 |
| 校验与比较 | `scl validate` 检查 XML 和支持的引用关系，可接入本地 XSD；`scl diff` 比较模型及有序 DataSet 成员 |
| 模拟 IED | `sim check` 检查运行模型；`sim run` 按初始值和时间线运行 MMS、GOOSE 或两者共享的模型 |
| MMS 客户端 | `mms browse`、`mms read`、`mms write`：浏览、读取和写入标量属性 |
| MMS 服务端 | 通过 `mms.writable` 开放指定属性，接受写入并更新共享模型；未配置的属性默认拒绝写入 |
| GOOSE 收发 | 模拟器发布指定控制块；`goose subscribe` 记录值、状态号、序列号、TTL 和超时事件 |
| 自动化集成 | `capabilities --json` 查询能力；命令输出带版本的 JSON，运行事件可单独写入 JSONL |

编辑保留未修改的 XML、命名空间、注释和厂商扩展，预览与保存使用同一条处理路径。DataSet 成员顺序变化会更新引用它的本地控制块版本号。输出必须使用新路径，失败不会覆盖原文件。

## 快速开始

需要 Rust 1.86+ 和对应平台的链接工具。Windows 可使用 MSVC 工具链及 Visual Studio Build Tools。从源码构建：

```sh
git clone https://github.com/Fzz748/iec61850-toolkit.git
cd iec61850-toolkit
cargo build --release --locked
cargo run --locked -- --help
cargo run --locked -- capabilities --json
```

优化后的 CLI 位于 `target/release/iec61850.exe`（Windows）或 `target/release/iec61850`（Linux）。SCL 文件操作不需要原生通信后端。

### 创建、编辑和比较 ICD

在仓库根目录执行，以下输出文件需尚不存在：

```sh
cargo run --locked -- icd create --spec examples/device.json -o demo.icd --references-only --json
cargo run --locked -- scl inspect demo.icd --ied DEMO --json
cargo run --locked -- scl edit demo.icd --patch examples/add-goose.json --dry-run --references-only --json
cargo run --locked -- scl edit demo.icd --patch examples/add-goose.json -o demo.updated.icd --references-only --json
cargo run --locked -- scl validate demo.updated.icd --references-only --json
cargo run --locked -- scl diff demo.icd demo.updated.icd --json
```

这个流程生成示例模型，预览并保存 DataSet / GOOSE 配置，最后报告校验范围和模型差异。输入格式见 [设备定义](examples/device.json) 和 [编辑补丁](examples/add-goose.json)。

> [!NOTE]
> 创建和编辑默认只检查 XML 与操作前提；`--references-only` 要求引用检查通过后再保存。完整 XSD 检查需要自行提供匹配的 schema 及 `xmllint`，例如 `--xsd /path/to/SCL.xsd --xmllint /path/to/xmllint`。缺少依赖会报告 `not_run` / `incomplete`，不会当作校验成功。编辑器保留原有 SCL 版本属性，不以版本白名单限制编辑。

## MMS 写入示例

本例同时演示**模拟可写 MMS 服务端**和**使用客户端发起写入**，只监听本机 `127.0.0.1:8102`。

先按 [原生后端构建说明](docs/runtime.md#build) 准备 CMake、C/C++ 工具链、Java 和 libiec61850 **v1.6.2.1**。Windows 后端还需要 Npcap 及其 SDK。下面假设后端已构建为 `target/native/iec61850-runtime.exe`，libiec61850 源码位于 `C:/deps/libiec61850`；请按实际路径调整。

终端 1（PowerShell）：检查模型，然后运行 60 秒模拟服务。

```powershell
$runtime = 'target/native/iec61850-runtime.exe'
$genconfig = 'C:/deps/libiec61850/tools/model_generator/genconfig.jar'

target/release/iec61850.exe sim check --spec examples/mms-write.json --genconfig $genconfig --runtime $runtime --json
target/release/iec61850.exe sim run --spec examples/mms-write.json --genconfig $genconfig --runtime $runtime --duration 60 --events writes.jsonl --json
```

终端 2（PowerShell）：在模拟服务运行期间，将设定值从 `10.0` 写为 `25.5`，再读回。

```powershell
$runtime = 'target/native/iec61850-runtime.exe'
$reference = 'DEMOLD0/GGIO1.Setpoint.setMag.f'

target/release/iec61850.exe mms write --host 127.0.0.1 --port 8102 --reference $reference --fc SP --value 25.5 --runtime $runtime --json
target/release/iec61850.exe mms read --host 127.0.0.1 --port 8102 --reference $reference --fc SP --runtime $runtime --json
```

两个终端均从仓库根目录执行，`writes.jsonl` 必须是新文件。预期写结果包含 `data.acknowledged: true`，读结果中的 `data.value.value` 为 `25.5`，服务端事件文件记录一次 `mms_write`。

[示例配置](examples/mms-write.json) 使用随附的 [ICD](examples/mms-write.icd)，并在 `mms.writable` 中列出可写属性：

```json
"writable": [
  { "reference": "DEMOLD0/GGIO1.Setpoint.setMag.f", "fc": "SP" }
]
```

这是 `mms` 对象内的配置片段。服务端支持开放 `SP/SV/CF/DC/BL` 下的标量叶属性；客户端按远端类型校验值，`--value-file` 可读取含单个 JSON 标量的文件，适合字符串和 INT64。详见 [写入契约](docs/runtime.md#mms-writes)。

> [!IMPORTANT]
> MMS 写入会实际修改目标模型。收到确认仅代表服务响应成功；超时可能意味着结果未知，CLI 不自动重试。本工具的模拟会产生真实网络报文，也不会自动设置 IEC simulation/test 标志。GOOSE 收发需指定已确认的测试网卡和网络参数。

### GOOSE 收发

通过 `runtime --runtime PATH --json` 获取网卡标识，在模拟配置中指定接口、控制块及 MAC / APPID / VLAN / MinTime / MaxTime。`sim run` 发布，`goose subscribe` 接收；两者均要求显式的运行时长。MMS 写入与时间线更新作用于同一个模型，相关 DataSet 的变化可触发 GOOSE 更新。

完整配置和命令见 [发布示例](docs/runtime.md#goose-publisher) 与 [订阅示例](docs/runtime.md#goose-subscriber)。`sim check` 不打开网络，不能代替实际收发验证。

## Agent Skill

仓库只维护一份 [iec61850 Skill](skills/iec61850/SKILL.md)，由 Skill 编排 CLI 工作流，协议与模型规则由代码实现。先构建 CLI，再将整个 `skills/iec61850` 目录复制到所用工具的技能目录：

| 工具 | 项目内安装 | 用户级安装 |
|---|---|---|
| Codex | `.agents/skills/iec61850/` | `~/.agents/skills/iec61850/` |
| Claude Code | `.claude/skills/iec61850/` | `~/.claude/skills/iec61850/` |

选择一种安装位置，保留目录内的 `references/`、元数据和许可文件。安装 Skill 不会安装 CLI、原生后端或 XSD 资源；调用时需确保 CLI 可定位。

例如，提供 CLI 路径和输入文件后请求：

> 使用 iec61850 技能检查这份 ICD，预览添加指定 DataSet 的变化，保存到新文件，并说明实际执行的校验范围。

## 验证与当前边界

常规检查：

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

v0.4.0 已在 Windows 本机完成 **40 项常规测试、9 项原生集成测试和 1 项原生值测试**。覆盖写入确认与读回、类型/范围拒绝、默认写保护、进程清理，以及 VMnet1 上 MMS 写入同步到 GOOSE。原生网络测试默认忽略，需按 [验证说明](docs/runtime.md#verification) 配置依赖与测试接口后单独运行。

| 范围 | 当前边界 |
|---|---|
| 平台 | Windows 本机已验证；保留 Linux 构建路径，Linux 运行与真实 IED 验证为 `not_run` |
| MMS | 支持普通标量数据读写；未实现整结构/数组写、SBO/Operate 控制、定值组操作流程和 Report 订阅客户端 |
| GOOSE | 支持二层发布与订阅；未实现抓包文件分析。Windows / VMnet1 在 MinTime 100 ms、MaxTime 1000 ms 下验证通过，4 ms MinTime 曾出现 TTL 超时 |
| SCL | 内置检查覆盖指定引用关系；未实现完整 NSD/CDC、ExtRef 校验、网络地址编辑和通用数据类型编辑 |
| 代码生成 | 未实现 C 代码生成；Java `genconfig` 仅用于构造模拟器运行模型 |

`--json` 输出一个最终结果对象，`--events` 将事件写入独立 JSONL 文件。退出码 `0` 表示请求范围通过，`1` 表示校验或观测失败，`2` 表示参数、执行或 I/O 错误，`3` 表示不完整；具体含义见 [CLI 契约](docs/cli.md)。

## 文档

| 入口 | 内容 |
|---|---|
| [CLI 契约](docs/cli.md) | JSON 输入输出、编辑操作、校验范围和退出码 |
| [MMS / GOOSE 运行指南](docs/runtime.md) | Windows / Linux 构建、依赖、模拟配置、收发验证和生命周期 |
| [架构说明](docs/architecture.md) | Rust 模块、原生后端与文件编辑流程 |
| [示例目录](examples/) | 设备定义、编辑补丁、时间线和 MMS 可写模型 |
| [共享 Skill](skills/iec61850/SKILL.md) | 模型工程、文件审查与通信模拟工作流 |

项目自有代码、文档与 Skill 使用 [Apache-2.0](LICENSE)，归属说明见 [NOTICE](NOTICE)。原生后端链接的 libiec61850 使用 GPL-3.0-or-later（另有商业许可选项），cJSON 使用 MIT，Npcap 另有许可且不随仓库分发；分发后端时需分别处理这些依赖的许可，详见 [依赖说明](docs/runtime.md#dependencies-and-memory)。
