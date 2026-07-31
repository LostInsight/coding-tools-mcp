# 需求文档：paseo-integration

## 功能概述

在 Coding Tools MCP Rust/Tauri 桌面应用中增加工作区级、默认关闭、严格隔离的 Paseo 控制与监控集成。集成只通过官方 Paseo CLI 的固定子命令访问 daemon，并通过现有 MCP/Actions 统一工具内核暴露八个 `paseo_` 工具。关闭时不得改变既有工具名称、数量、默认行为、延迟或磁盘状态。

## 历史经验与坑（来自记忆库）

- **可复用经验**: 当前工作区通过 serde 默认值兼容增量配置；MCP 与 Actions 共用 `call_tool`，适合在统一入口增加隔离分支；敏感值应复用 `SecretStore`。
- **必须规避的坑**: 当前 Paseo CLI 0.2.2 的 `logs` 即使使用全局 JSON 选项仍输出文本，且 `daemon status` 不接受 `--host`。当前 MCP 声明 `listChanged=false`，运行中的 listener 也持有启动时配置快照。

---

## 范围边界

- **In Scope**: 工作区级配置与迁移；安全 CLI runner；类型化 client；健康、列表、活动、权限、诊断、监控快照、发送提示、停止代理；策略、速率限制、审计与脱敏；MCP/Actions 注册和统一分发；独立桌面设置；fixture 测试；中英文文档和版本说明。
- **Out of Scope**: 权限批准/拒绝；创建、删除或归档代理；永久 kill；任意终端按键；任意 CLI 参数透传；daemon 配置修改；worktree、Git、发布或部署自动化；Paseo 内部协议、数据库或包依赖；后台主动轮询。

---

## 需求列表

### FR-1: 工作区级可选集成配置

**优先级:** Must
**用户故事:** 作为桌面应用用户，我想按工作区启用 Paseo 并选择访问模式，以便不影响未使用 Paseo 的项目。

#### 验收标准（EARS）

1. WHEN 旧工作区配置被加载 THEN 系统 SHALL 以 `enabled=false`、`access_mode=read_only` 和安全默认值补全 Paseo 配置。
2. WHEN Paseo 未启用 THEN 系统 SHALL 不暴露工具、不探测 CLI、不创建进程、不创建快照文件且不增加既有请求路径工作量。
3. WHEN 配置保存 THEN 系统 SHALL 只更新当前工作区，并仅重启当前工作区中正在运行且与工具清单相关的服务。
4. WHEN MCP 不支持工具清单变更通知 THEN UI SHALL 提示客户端可能需要重新连接。

### FR-2: 固定且安全的 CLI 适配边界

**优先级:** Must
**用户故事:** 作为安全管理员，我想让所有 Paseo 操作通过固定参数数组执行，以便客户端不能注入任意命令。

#### 验收标准（EARS）

1. WHEN 任一 Paseo 操作执行 THEN 系统 SHALL 使用 `PaseoCommandRunner` 与参数数组直接创建进程且不调用 shell。
2. WHEN 命令超时或输出超限 THEN 系统 SHALL 终止子进程、限制 stdout/stderr 并返回结构化错误或截断标识。
3. WHEN binary path 为空 THEN 系统 SHALL 先复用软件发现/PATH 机制且不得下载或安装 Paseo。
4. IF binary、host、agent ID 或输入长度无效 THEN 系统 SHALL 在进程启动前返回稳定的 `PASEO_*` 错误码。
5. WHEN 进程启动 THEN 系统 SHALL 使用最小环境、关闭颜色，并在 Windows 隐藏控制台窗口。

### FR-3: 版本化解析与统一脱敏

**优先级:** Must
**用户故事:** 作为客户端开发者，我想获得稳定的结构化结果和来源元数据，以便处理 CLI 版本差异。

#### 验收标准（EARS）

1. WHEN JSON 可用 THEN 系统 SHALL 使用 serde JSON 解析并返回 `source_format=json`、parser version、CLI version 与缺失字段信息。
2. WHEN Paseo 0.2.2 活动只提供文本 THEN 系统 SHALL 使用严格的、版本化的记录解析器并返回 `source_format=text_fallback`。
3. WHEN 输出不符合已知格式 THEN 系统 SHALL 返回 `PASEO_PARSE_ERROR`，不得进行随意字符串切分。
4. WHEN host、pairing URL、token、环境变量或活动内容进入日志、快照或 MCP 输出 THEN 系统 SHALL 先经统一 redactor 脱敏和长度限制。

### FR-4: 健康检查工具

**优先级:** Must
**用户故事:** 作为 MCP 客户端，我想调用 `paseo_health`，以便判断 CLI 与 daemon 是否可用。

#### 验收标准（EARS）

1. WHEN read-only 以上模式调用健康检查 THEN 系统 SHALL 返回 CLI 可用性、版本、daemon 可达性、连接类型、脱敏 host 状态、访问模式与能力矩阵。
2. WHEN连接为本地 THEN 系统 SHALL 使用 `daemon status --json`；WHEN连接为远程 THEN 系统 SHALL 使用受限只读远程查询验证可达性。
3. WHEN `refresh=false` 且短期缓存有效 THEN 系统 SHALL 不重复启动 CLI。
4. WHEN仅查询 `server_info` THEN 系统 SHALL 不运行 Paseo CLI。

### FR-5: 代理发现与有界筛选

**优先级:** Must
**用户故事:** 作为监控客户端，我想列出和筛选代理，以便快速定位目标代理。

#### 验收标准（EARS）

1. WHEN调用 `paseo_list_agents` THEN 系统 SHALL 使用稳定 agent ID 返回名称、状态、provider、workspace 和可用时间字段。
2. WHEN请求状态、名称、标签或 workspace 过滤 THEN 系统 SHALL 在受限结果集上确定性筛选，最大返回 100 个代理。
3. IF 可选字段缺失或状态未知 THEN 系统 SHALL 保留结果、标记缺失字段并映射状态为 `UNKNOWN`。
4. WHEN返回代理 THEN 系统 SHALL 不返回完整 prompt、凭证或无关日志。

### FR-6: 活动与权限只读查询

**优先级:** Must
**用户故事:** 作为支持人员，我想查看近期活动和待授权请求，以便确认代理阻塞位置。

#### 验收标准（EARS）

1. WHEN调用 `paseo_get_agent_activity` THEN 系统 SHALL 将 `tail`、filter 与字节上限约束在 schema 和策略上限内，不支持 follow/streaming。
2. WHEN活动返回 THEN 系统 SHALL 规范化消息、工具、错误和等待事件，脱敏摘要并明确截断与字段缺失。
3. WHEN调用 `paseo_list_pending_permissions` THEN 系统 SHALL 仅查询请求类型、时间、代理与脱敏摘要，不提供 approve/deny 能力。

### FR-7: 确定性代理诊断

**优先级:** Must
**用户故事:** 作为 MCP 客户端，我想获得透明、可测试的代理诊断，以便识别等待、失败或卡住状态。

#### 验收标准（EARS）

1. WHEN调用 `paseo_diagnose_agent` THEN 系统 SHALL 组合代理状态、最近活动、错误、权限与前次监控状态，并返回固定 classification 枚举。
2. WHEN存在 pending permission THEN 系统 SHALL 优先分类为 `WAITING_PERMISSION`。
3. WHEN规范化错误达到阈值 THEN 系统 SHALL 分类为 `REPEATED_FAILURE` 并只返回稳定摘要。
4. WHEN运行中超过阈值且有效进展指纹未变化 THEN 系统 SHALL 分类为 `POSSIBLY_STALLED`；IF 缺少时间或前次证据 THEN 系统 SHALL 不因单次沉默误判。
5. WHEN长时间 build/test/download 仍有输出变化 THEN 系统 SHALL 分类为 `EXTERNAL_WAIT`。
6. WHEN等待用户、崩溃、idle 未完成或已完成证据存在 THEN 系统 SHALL 分别返回 `WAITING_USER_INPUT`、`CRASHED`、`IDLE_INCOMPLETE` 或 `COMPLETED`。
7. WHEN诊断完成 THEN 系统 SHALL 返回有限 recommended action 枚举且不得自动发送提示或停止代理。

### FR-8: 每小时低开销监控快照

**优先级:** Must
**用户故事:** 作为 ChatGPT 定时任务，我想一次调用完成轻量监控，以便只在变化时通知用户。

#### 验收标准（EARS）

1. WHEN调用 `paseo_monitor_snapshot` THEN 系统 SHALL 一次完成发现、配置过滤、轻量诊断、前次比较和事件汇总。
2. WHEN代理看起来正常 THEN 系统 SHALL 避免读取大段活动；仅对潜在异常代理读取配置限定的活动尾部。
3. WHEN没有变化 THEN 系统 SHALL 返回 `changed=false` 和四个空变化数组。
4. WHEN写快照 THEN 系统 SHALL 在应用数据目录按工作区隔离、原子写入、限制数量与总大小，且不存完整日志、prompt 或 secret。
5. IF 快照损坏 THEN 系统 SHALL 忽略损坏数据、安全重建并继续实时查询。
6. WHEN同一工作区已有刷新 THEN 系统 SHALL 去重或返回可重试限流错误。

### FR-9: Assist 模式发送提示

**优先级:** Must
**用户故事:** 作为明确启用 Assist 的用户，我想向现有代理发送诊断或继续提示，以便协助恢复任务。

#### 验收标准（EARS）

1. WHEN access mode 为 assist/control 且 prompt 合法 THEN 系统 SHALL 以单一进程参数调用 `send <id> --no-wait <prompt>` 并返回排队状态。
2. IF prompt 为空、超长或 agent ID 无效 THEN 系统 SHALL 在执行前拒绝。
3. WHEN调用成功或失败 THEN 系统 SHALL 写不含 prompt 正文的应用审计事件。
4. IF integration 未启用或模式不足 THEN 系统 SHALL 返回 `PASEO_DISABLED` 或 `PASEO_ACCESS_DENIED`。

### FR-10: Control 模式停止代理

**优先级:** Must
**用户故事:** 作为明确启用 Control 的用户，我想中断代理当前运行，以便处理无法恢复的循环。

#### 验收标准（EARS）

1. WHEN access mode 为 control、`confirm=true` 且 reason 非空 THEN 系统 SHALL 调用 `stop <id>` 并返回停止前后状态。
2. IF confirm 不为 true THEN 系统 SHALL 返回 `PASEO_CONFIRMATION_REQUIRED`。
3. WHEN工具注册 THEN 系统 SHALL 将 stop 标为 destructive、非幂等、open-world。
4. WHEN停止执行 THEN 系统 SHALL 防止同一代理并发重复停止并记录脱敏审计事件。

### FR-11: Context-aware 注册与双重策略门

**优先级:** Must
**用户故事:** 作为现有 MCP/Actions 用户，我想保持原工具契约不变，同时按 Paseo 模式获得额外工具。

#### 验收标准（EARS）

1. WHEN integration disabled THEN `list_tools_for_profile`、core 常量和既有工具清单 SHALL 与变更前一致。
2. WHEN read_only/assist/control 启用 THEN context-aware 清单 SHALL 分别追加 6/7/8 个固定 `paseo_` 工具。
3. WHEN伪造 `tools/call` 绕过清单 THEN Paseo policy SHALL 再次验证 enabled、mode、参数和速率限制。
4. WHEN MCP 或 Actions 执行 Paseo 工具 THEN 两者 SHALL 进入同一 `call_tool` 与同一 Paseo 实现。
5. WHEN返回 `server_info` THEN系统 SHALL 只返回静态 integration enabled、mode、tool count 与 unknown health。

### FR-12: 独立桌面设置体验

**优先级:** Must
**用户故事:** 作为桌面用户，我想在独立 Paseo Integration 区域完成配置和验证，以便理解暴露能力与风险。

#### 验收标准（EARS）

1. WHEN打开工作区 MCP 配置 THEN UI SHALL 提供独立 Paseo Integration 区域，包含 enable、mode、binary 自动/手选、local/remote、默认遮蔽 host、连接测试、timeout、activity tail、stalled threshold、include/exclude filters、工具预览与快照清除。
2. WHEN选择 Assist 或 Control THEN UI SHALL 显示对应风险提示；Control 不要求应用每次启动重复确认。
3. WHEN点击连接测试 THEN UI SHALL 显示脱敏健康结果和 CLI 版本，不在控制台打印 secret。
4. WHEN清除快照 THEN 系统 SHALL 只删除当前工作区应用数据中的 Paseo 快照。

### FR-13: 测试与文档

**优先级:** Must
**用户故事:** 作为维护者，我想有无需真实 Paseo 的回归测试与完整文档，以便跨平台维护。

#### 验收标准（EARS）

1. WHEN运行 Rust 测试 THEN fake runner 与 fixtures SHALL 覆盖注册隔离、CLI 参数安全、版本解析、十种诊断分类、快照差异/损坏/脱敏和配置迁移。
2. WHEN运行前端静态检查与构建 THEN Paseo 设置组件 SHALL 通过 TypeScript/Svelte 校验。
3. WHEN阅读文档 THEN 用户 SHALL 能找到安装、local/remote、pairing 敏感性、access mode、工具、ChatGPT 每小时任务、错误、版本兼容、安全边界与关闭方式。

---

## 非功能需求

- **NFR-1 安全**: 不调用 shell；不开放任意参数；所有输出在配置上限 1 KiB 至 1 MiB 内；所有错误/审计/快照先脱敏。
- **NFR-2 性能**: disabled 请求路径只做一次内存布尔判断；health 缓存；list 最短间隔数秒；监控同工作区单飞；正常代理不读取详细日志。
- **NFR-3 兼容**: Windows 10、macOS 与 Linux 可编译；Windows 子进程不弹窗；含空格 binary path 与 workspace path 正确处理。
- **NFR-4 稳定**: CLI 0.2.2 为验证基线；未知版本/格式返回结构化错误而非 panic；损坏快照不阻塞实时查询。
- **NFR-5 隔离**: Paseo 模块不依赖 Harness、history、Git、patch、exec session 或项目目录持久化。

---

## 依赖关系

- 现有 serde/serde_json、regex、sha2、which/fs2 与标准库进程、文件和同步原语。
- 现有 `WorkspaceProfile`、`SecretStore`、统一工具结果包装、Tauri invoke、日志目录和 dialog 插件。
- 用户单独安装的 Paseo CLI；应用不得自动下载或安装。
