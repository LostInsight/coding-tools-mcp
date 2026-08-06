# 设计文档：paseo-integration

## 概述

新增正交 integration 层，将 Coding Tools MCP transport 与 Paseo daemon 通过固定 Paseo CLI 适配器连接。Paseo 的配置、进程策略、解析、诊断、快照、脱敏和工具实现均位于独立模块；MCP 与 Actions 仅组合清单并通过现有 `call_tool` 进入该模块。

**对应需求:** FR-1 至 FR-13，NFR-1 至 NFR-5

---

## 技术方案

### 技术选型

| 类别 | 选择 | 理由 | 关联需求 |
|------|------|------|----------|
| Paseo 边界 | 官方 CLI 固定子命令 | 避免内部协议/包耦合 | FR-2, NFR-5 |
| 进程执行 | `std::process::Command` + 独立 reader/timeout runner | 不经 shell，可测试且可限制输出 | FR-2 |
| 解析 | serde JSON + 0.2.x 文本状态机 | 稳定、版本化，兼容旧分隔记录和 0.2.5 行事件 | FR-3 |
| 配置 | `WorkspaceProfile.integrations.paseo` + serde default | 工作区级且向后兼容 | FR-1 |
| 敏感 host | `SecretStore` 的 `paseo_host` key | 不进入普通 profile/debug 输出 | FR-1, FR-3 |
| 快照 | 应用配置根目录下的独立原子 JSON 文件 | 不污染项目，易限额与恢复 | FR-8 |
| UI | Svelte 5 独立组件 + Tauri commands | 符合现有 workspace 页面模式 | FR-12 |

### 架构设计

```text
MCP handle_tools_call / Actions execute_action
                    |
                    v
           tools::dispatch::call_tool
                    |
          fixed paseo_ prefix branch
                    |
                    v
       integrations::paseo::tools::call
          | policy | rate limit | audit
          v
              PaseoClient trait
                    |
              PaseoCliClient
                    |
          PaseoCommandRunner trait
                    |
          SystemPaseoCommandRunner
                    |
             paseo argument array
```

禁用状态只存在内存配置，不构造进程、不探测 binary、不创建目录。运行态上下文包含配置快照、可选 host secret 和同步速率限制状态；只有工具调用或显式 UI 测试才解析 binary 并启动 CLI。

---

## 数据模型

```rust
struct WorkspaceIntegrations {
    paseo: PaseoIntegrationConfig,
}

struct PaseoIntegrationConfig {
    enabled: bool,
    access_mode: PaseoAccessMode,
    binary_path: String,
    command_timeout_ms: u64,
    max_output_bytes: usize,
    monitor: PaseoMonitorConfig,
    host_configured: bool,
}

struct PaseoRuntimeContext {
    workspace_id: String,
    workspace_path: PathBuf,
    config: PaseoIntegrationConfig,
    host: Option<String>,
    state: Arc<PaseoRuntimeState>,
}
```

`PaseoRuntimeState` 仅持有内存缓存、并发标记和速率窗口。`PaseoAgentSnapshot`、`ActivityEvent`、`PermissionSummary` 与 `PaseoDiagnosis` 使用结构化 enum 和 optional 时间字段。快照只存 agent ID、classification、status、有效进展指纹/时间和异常状态。

---

## API 设计

| 方法/函数 | 路径/签名 | 入参/出参 | 关联需求 |
|------|------|------|----------|
| MCP list | `list_tools_for_context(profile, paseo)` | profile + config / schema array | FR-11 |
| MCP/Actions dispatch | `call_tool(ctx, name, args)` | fixed tool name + JSON / unified result | FR-11 |
| Tauri read | `get_paseo_integration_settings(id)` | workspace ID / redacted DTO | FR-12 |
| Tauri save | `save_paseo_integration_settings(id, input)` | config + optional host update / DTO | FR-1, FR-12 |
| Tauri test | `test_paseo_connection(id, draft)` | explicit draft / redacted health | FR-4, FR-12 |
| Tauri clear | `clear_paseo_monitor_snapshots(id)` | workspace ID / removed count | FR-8, FR-12 |
| MCP tools | eleven fixed `paseo_*` schemas | bounded documented inputs / structured outputs | FR-4 至 FR-10 |

MCP annotations are generated from separate Paseo definitions: six read-only tools are read-only/idempotent/open-world; send/create are write/non-destructive/non-idempotent/open-world; stop and exact permission allow/deny are write/destructive/non-idempotent/open-world.

---

## CLI 命令映射

| Operation | Fixed arguments | Format strategy |
|---|---|---|
| version | `--version` | bounded text |
| local health | `daemon status --json` | JSON |
| remote health/list | `ls -a -g --json --host <host>` | JSON |
| activity | `logs <id> --tail <n> [--filter <fixed>] [--host <host>]` | 0.2.2 text fallback |
| permissions | `permit ls --json [--host <host>]` | JSON |
| send | `send <id> --no-wait --json [--host <host>] -- <prompt>` | strict JSON |
| stop | `stop <id> --json [--host <host>]` | JSON |

`--host` and prompt are always distinct arguments. The runner clears environment then restores only OS/runtime essentials. It drains stdout/stderr concurrently, retains at most configured bytes, kills on timeout, maps spawn/exit/parse classes to stable errors, and uses `CREATE_NO_WINDOW` on Windows.

Windows 0.2.2 may publish a `.cmd` PATH shim. The resolver treats it only as a bounded discovery file, follows at most two exact official launcher hops, and resolves the bundled native `Paseo.exe`; the runner rejects command scripts defensively and never executes them.

---

## 诊断与监控算法

Priority order: completed/stopped with completion evidence; crashed; pending permission; repeated failure; waiting user; external wait; stalled; idle incomplete; healthy; unknown. A classification includes evidence and never relies only on missing chat messages.

Effective progress includes new assistant text, successful tool completion, output growth, status transition, or a changed bounded activity fingerprint. Error signatures remove ANSI, timestamps, UUIDs, paths, line numbers, host secrets and long variable segments, then expose only a short SHA-256 identifier plus bounded category text.

For a first observation without reliable timestamps, a running agent cannot become stalled from silence alone. A later monitor snapshot can calculate `stalled_for_minutes` from the last time the effective-progress fingerprint changed. Long-running build/test/download events with changed output become `EXTERNAL_WAIT`.

Snapshot files are named by timestamp/UUID under `app_config_dir/data/paseo-monitor/<workspace-id>/`. Each file is written to a unique temporary name, synced, then renamed. Reads skip malformed files. Retention keeps a bounded count and total bytes. No project-relative path is used.

---

## 文件结构

```text
src-tauri/src/integrations/
  mod.rs
  paseo/
    mod.rs config.rs model.rs command.rs client.rs cli.rs parser.rs
    diagnostics.rs monitor_store.rs policy.rs redaction.rs tools.rs
src-tauri/src/commands/paseo.rs
src-tauri/tests/fixtures/paseo/
src/lib/api/paseo.ts
src/lib/components/PaseoIntegrationForm.svelte
docs/paseo-integration.md
```

Existing files receive only additive wiring in workspace model, runtime construction, tool context/registry/dispatch, MCP/Actions listeners, Tauri registration, frontend types/workspace page, READMEs and release notes.

---

## 设计决策

### 决策 1: 保留 profile API，新增 context-aware 组合（关联需求: FR-11）

**问题**: Paseo 是正交 integration，不能改变固定 core/profile。
**选项**: 将名称加入 core；替换 profile 函数；新增组合函数。
**决策**: 保留 `list_tools_for_profile` 和现有常量，新增 `list_tools_for_context`、contextual allowed/mutating helpers。

### 决策 2: HIGH 风险 dispatcher 只增加早分支（关联需求: FR-11）

**问题**: GitNexus 显示 `call_tool` HIGH，14 个直接调用方、62 个总影响。
**选项**: 重构整个 match；在 transport 执行；固定前缀早返回。
**决策**: 在任何 generic policy/Harness 操作前对已知 `paseo_` 名称调用独立模块；其他执行逐字保持原顺序。

### 决策 3: 保存后范围内重启（关联需求: FR-1, FR-12）

**问题**: listener 上下文与 Actions OpenAPI 在启动时固定，协议声明不支持 list change notification。
**选项**: 全局 watcher；只保存并要求手动重启；显式保存后重启当前工作区相关服务。
**决策**: UI 保存成功后只重启当前工作区正在运行的 MCP/Actions，并显示客户端重连提示，不触碰其他工作区。

### 决策 4: 监控证据不足时保持保守（关联需求: FR-7, FR-8）

**问题**: Paseo 0.2.2 日志文本无可靠时间戳。
**选项**: 猜测时间；读取内部数据库；利用快照变化。
**决策**: 不猜测、不访问内部数据；使用可选 CLI 时间字段与应用快照中的首次/最近进展观察时间。

---

## 风险评估

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| `call_tool` HIGH blast radius | 高 | 单一早分支、disabled 合约测试、全量 cargo test |
| CLI 版本输出漂移 | 高 | source/parser/CLI 元数据、fixtures、未知格式显式错误 |
| host 含 pairing/password | 高 | SecretStore、统一 redactor、DTO 仅 masked display |
| 子进程输出或超时耗尽资源 | 高 | 并发 drain、硬上限、kill/wait、速率限制 |
| 活动缺时间导致误判 | 中 | 首次观察不判 stalled；快照变化作为证据 |
| Actions OpenAPI 静态 | 中 | 当前工作区 scoped restart + client reconnect warning |
| 快照损坏或增长 | 低 | unique atomic files、skip corruption、count/bytes retention |
