# 任务清单：paseo-integration

## 概述

实现 paseo-integration 的任务分解；每条任务回链 FR 与 design 章节。

> **二元禁令**：交付物中不得保留占位符或省略实现。动手前先读相关代码并完成 impact analysis；单个实现文件超过 500 行必须拆分。

---

## 交付物清单（Scope-lock）

- 预计实现、fixture 与文档文件数: 39
- 预计任务数: 18

最终交付文件数为 47（不含本规格与根规划记录），比预计多 8 个。差异来自为保持每文件不超过 500 行并强化安全边界而拆出的参数、binary 解析、monitor orchestration、diagnostics tests/schema 等独立文件；范围未扩大。

实现完成前回读本清单逐项核对；规划记录文件和本规格目录不计入 39 个交付文件。

---

## 任务列表

### 阶段 1: 审计与规格

- [x] 1.1 审计工具、配置、运行时与 UI 调用链并刷新图谱
  - 证据块: `src-tauri/src/tools/registry.rs`, `dispatch.rs`, MCP/Actions listener, workspace/data/runtime, workspace Svelte page
  - 文件: `findings.md`, `progress.md`, `note.md`（每个不超过 250 行）
  - _需求: FR-1, FR-11, FR-12_ · _设计: 架构设计、设计决策_
- [x] 1.2 实测 Paseo 0.2.2 CLI 帮助与格式并完成 blast radius
  - 证据块: 八条要求的 help 命令、JSON shape probe、GitNexus impact
  - 文件: `task_plan.md`（不超过 250 行）
  - _需求: FR-2, FR-3_ · _设计: CLI 命令映射、风险评估_

---

### 阶段 2: 后端基础

- [x] 2.1 新建 integrations/paseo 配置、模型与策略模块，保持每文件不超过 500 行
  - 证据块: `src-tauri/src/workspace/model.rs`, `src-tauri/src/tools/context.rs`, `src-tauri/src/secret/keyring_store.rs`
  - 文件: `src-tauri/src/integrations/mod.rs`, `paseo/mod.rs`, `config.rs`, `model.rs`, `policy.rs`
  - _需求: FR-1, FR-7, FR-11_ · _设计: 数据模型_
- [x] 2.2 新建无 shell command runner、typed client 与 CLI backend，保持每文件不超过 500 行
  - 证据块: `src-tauri/src/tools/exec.rs`, `src-tauri/src/platform/*/paths.rs`, installed Paseo help
  - 文件: `command.rs`, `client.rs`, `cli.rs`
  - _需求: FR-2, FR-4, FR-5, FR-6, FR-9, FR-10_ · _设计: CLI 命令映射_
- [x] 2.3 新建版本化 parser 与统一 redactor，保持每文件不超过 500 行
  - 证据块: CLI JSON key probe and bracket/separator activity shape
  - 文件: `parser.rs`, `redaction.rs`, fixtures under `src-tauri/tests/fixtures/paseo/`
  - _需求: FR-3, FR-5, FR-6_ · _设计: 技术选型_

---

### 阶段 3: 工具与监控

- [x] 3.1 实现 health/list/activity/permissions 与结构化错误
  - 证据块: `src-tauri/src/tools/workspace.rs`, Paseo tool output requirements
  - 文件: `src-tauri/src/integrations/paseo/tools.rs`（不超过 500 行，超出则拆分）
  - _需求: FR-4, FR-5, FR-6_ · _设计: API 设计_
- [x] 3.2 实现十分类确定性 diagnostics 与稳定错误签名
  - 证据块: `model.rs`, requirements FR-7 rules
  - 文件: `diagnostics.rs`（不超过 500 行）
  - _需求: FR-7_ · _设计: 诊断与监控算法_
- [x] 3.3 实现应用数据目录原子 monitor store 与 snapshot diff
  - 证据块: `src-tauri/src/platform/mod.rs`, `src-tauri/src/data/migrate.rs`
  - 文件: `monitor_store.rs`（不超过 500 行）
  - _需求: FR-8_ · _设计: 诊断与监控算法_
- [x] 3.4 实现 Assist/Control 调用、限流和审计
  - 证据块: existing `append_profile_log`, CLI send/stop semantics
  - 文件: `tools.rs`, `policy.rs`
  - _需求: FR-9, FR-10_ · _设计: CLI 命令映射_

---

### 阶段 4: 注册与运行时集成

- [x] 4.1 添加 workspace integrations 默认值与 host secret allowlist
  - 证据块: `src-tauri/src/workspace/model.rs`, `src-tauri/src/commands/secrets.rs`
  - 文件: 两个现有 Rust 文件（各净增不超过 80 行）
  - _需求: FR-1, FR-3_ · _设计: 数据模型_
- [x] 4.2 添加 context-aware catalog，保持旧 profile/core 输出不变
  - 证据块: `src-tauri/src/tools/registry.rs`, registry contract tests
  - 文件: `registry.rs`, `tools/mod.rs`（各净增不超过 180 行）
  - _需求: FR-11_ · _设计: 决策 1_
- [x] 4.3 在 HIGH 风险 call_tool 添加单一 Paseo 早分支并更新 server_info
  - 证据块: GitNexus HIGH impact report and `dispatch.rs` current order
  - 文件: `dispatch.rs`（净增不超过 35 行）
  - _需求: FR-4, FR-11_ · _设计: 决策 2_
- [x] 4.4 将配置上下文接入 MCP、Actions 与 RuntimeSupervisor
  - 证据块: `mcp/server.rs`, both listeners, `actions/openapi.rs`, `runtime/supervisor.rs`
  - 文件: 七个现有 Rust 文件（每个净增不超过 100 行）
  - _需求: FR-1, FR-11_ · _设计: 架构设计_

---

### 阶段 5: 桌面端

- [x] 5.1 新建 Paseo Tauri settings/test/clear commands 与前端 API/types
  - 证据块: `src-tauri/src/commands/workspace.rs`, `src/lib/api/workspaces.ts`, `src/lib/types.ts`
  - 文件: `src-tauri/src/commands/paseo.rs`, commands/lib registration, `src/lib/api/paseo.ts`, `src/lib/types.ts`
  - _需求: FR-1, FR-4, FR-8, FR-12_ · _设计: API 设计_
- [x] 5.2 新建独立 PaseoIntegrationForm 并接入工作区 MCP 配置
  - 证据块: `RuntimePolicyForm.svelte`, full workspace page, dialog and restart helpers
  - 文件: `src/lib/components/PaseoIntegrationForm.svelte`, workspace page（组件不超过 500 行）
  - _需求: FR-12_ · _设计: 决策 3_

---

### 阶段 6: 测试与文档

- [x] 6.1 用 fake runner 与 fixtures 覆盖注册、CLI 安全、解析、诊断、快照和迁移
  - 证据块: FR-13 的 42 类场景、现有 `src-tauri/tests/common/mod.rs` 和模块内测试模式
  - 验收点: requirements FR-13 的 42 类测试场景
  - _需求: FR-1 至 FR-13_
- [x] 6.2 编写详细指南并更新中英文 README 与版本说明
  - 证据块: `README.md`, `README.en.md`, current package scripts
  - 文件: `docs/paseo-integration.md`, `README.md`, `README.en.md`, `docs/release-notes.md`
  - _需求: FR-13_ · _设计: 文件结构_

---

### 阶段 7: 验证与清理

- [x] 7.1 运行 npm/cargo 全量检查并修复所有失败
  - 验收点: `npm run check`, `npm run build`, `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets -- -D warnings`
  - _需求: FR-13_
- [x] 7.2 运行 GitNexus change detection 与 diff/secret/artifact 审计
  - 验收点: existing core list unchanged; no host/token/snapshot/build artifacts; only scoped files remain
  - _需求: FR-3, FR-11, NFR-5_

---

## 检查点

- [x] 阶段 2 完成后：fake runner 能证明参数数组、无 shell、timeout 和输出上限。
- [x] 阶段 4 完成后：disabled/read_only/assist/control 工具清单和伪造调用策略全部通过。
- [x] 阶段 5 完成后：旧 profile 加载、设置保存重载、masked host、connection test、snapshot clear 均可验证。
- [x] 阶段 7 完成后：全量命令通过或准确记录环境限制，且 Git 差异无生成元数据噪声。

---

## 需求覆盖矩阵

| 需求 ID | 设计章节 | 任务编号 | 状态 |
|---------|----------|----------|------|
| FR-1 | 数据模型、决策 3 | 2.1, 4.1, 4.4, 5.1 | 完成 |
| FR-2 | CLI 命令映射 | 2.2 | 完成 |
| FR-3 | 技术选型、CLI 命令映射 | 2.3, 4.1 | 完成 |
| FR-4 | API 设计 | 3.1, 5.1 | 完成 |
| FR-5 | API 设计 | 2.2, 3.1 | 完成 |
| FR-6 | API 设计 | 2.3, 3.1 | 完成 |
| FR-7 | 诊断与监控算法 | 2.1, 3.2 | 完成 |
| FR-8 | 诊断与监控算法 | 3.3, 5.1 | 完成 |
| FR-9 | CLI 命令映射 | 2.2, 3.4 | 完成 |
| FR-10 | CLI 命令映射 | 2.2, 3.4 | 完成 |
| FR-11 | 决策 1、决策 2 | 4.2, 4.3, 4.4 | 完成 |
| FR-12 | API 设计、决策 3 | 5.1, 5.2 | 完成 |
| FR-13 | 风险评估 | 6.1, 6.2, 7.1, 7.2 | 完成 |

---

## 文件变更清单

| 文件组 | 操作 | 行数预算 | 说明 |
|------|------|----------|------|
| `src-tauri/src/integrations/paseo/*.rs` | 新建 | 每个 ≤500 | 独立后端模块 |
| `src-tauri/src/commands/paseo.rs` | 新建 | ≤300 | UI 命令 |
| tool/MCP/Actions/runtime/workspace wiring | 修改 | 每个净增 ≤180 | 加法式上下文集成 |
| `src-tauri/tests/fixtures/paseo/*` | 新建 | 每个 ≤120 | 版本化 fixtures |
| `src/lib/api/paseo.ts` | 新建 | ≤160 | Tauri API |
| `src/lib/components/PaseoIntegrationForm.svelte` | 新建 | ≤500 | 独立设置区域 |
| `src/lib/types.ts`, workspace page | 修改 | 每个净增 ≤160 | 类型与接线 |
| documentation files | 新建/修改 | 每个 ≤500 | 详细指南与入口 |

---

## 交付前自检

- [x] 无占位符或省略实现
- [x] 交付物数量与 Scope-lock 一致，差异有记录
- [x] 每个实现文件不超过 500 行、每条任务回链 FR
