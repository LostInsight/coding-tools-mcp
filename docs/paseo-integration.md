# Paseo Integration

Coding Tools MCP 可以把现有 Paseo daemon 作为一个可选、工作区级的控制与监控集成暴露给 MCP 客户端和 ChatGPT Actions。它不替代 Paseo 自身客户端；创建代理和处理权限只在 Control 模式、固定工具、精确目标和显式确认下可用，daemon 配置仍不会被修改。

## 功能定位

Paseo Integration 适合以下场景：

- 在 ChatGPT、Codex 或其他 MCP 客户端中检查 Paseo CLI 与 daemon；
- 一次列出和筛选多个现有代理；
- 查看经过脱敏和大小限制的近期活动；
- 查询待处理权限，但不批准或拒绝；
- 使用确定性规则识别等待、重复失败、崩溃、未完成 idle 或可能停滞；
- 用一个快照工具执行每小时低开销监控；
- 在明确启用 Assist/Control 后发送提示或停止当前运行。

集成默认关闭。关闭时不会运行 Paseo CLI、探测 daemon、创建快照或改变原有 core 工具清单。

## 安全边界

集成只允许固定操作，并通过独立策略层再次检查每次调用：

- CLI 使用 `std::process::Command` 参数数组直接启动；应用不调用 `cmd.exe /c`、不构造 shell 字符串；
- 不复用通用 `exec_command`；
- 不接受任意子命令、flag、管道、重定向或参数透传；
- agent ID、host、timeout、输出大小、prompt 和 reason 都有明确上限；
- stdout/stderr 并发读取并限制大小，超时会终止进程树；
- Windows 子进程使用隐藏窗口标志；
- 环境变量从空环境开始，只恢复 CLI/操作系统需要的最小集合；
- MCP 输出、错误、审计和快照使用统一 redactor；
- 活动只返回语义化事件摘要，不返回完整 prompt 或完整工具参数。

Paseo 代码不依赖 Coding Tools MCP 的 Harness、history session、Git、Patch 或普通命令 session。监控文件只写入应用数据目录，不写入当前项目。

## 安装 Paseo CLI

从 Paseo 官方发行渠道安装桌面应用或 CLI，然后在终端确认：

```text
paseo --version
paseo --help
```

Coding Tools MCP 不会下载、安装或升级 Paseo。Paseo binary 留空时，应用使用现有软件发现机制和 `PATH`；也可以在工作区的 **Paseo Integration** 区域手动选择可执行文件。

Windows 官方 Paseo 0.2.x/0.3.x 桌面分发可能在 `PATH` 中提供 `.cmd` launcher。集成只把该 launcher 当作受限的发现线索：它验证官方固定布局并解析到同一安装中的原生 `Paseo.exe`，实际子进程始终是 `.exe`。任意 `.cmd`、`.bat` 或无法解析到官方原生 executable 的 launcher 都会在启动前被拒绝，因此不会隐式进入 Windows command shell。

## 本地 daemon

1. 使用 Paseo 官方方式启动本地 daemon。
2. 在工作区 MCP 配置中打开 **Paseo Integration**。
3. Connection 选择 **Local daemon**。
4. 选择访问模式并保存。
5. 点击 **测试连接**，确认 CLI 版本与 daemon reachable。

本地健康检查使用 `paseo daemon status --json`。保存配置时，只重启当前工作区中已经运行的 MCP/Actions 服务；其他工作区不会重启。MCP server 声明 `tools.listChanged=true`，并在客户端以 `Accept: text/event-stream` 订阅 `/mcp` 时发送 `notifications/tools/list_changed`；不支持该通知通道的客户端仍需断开后重新连接才能刷新工具清单。

## 远程 host

Connection 选择 **Remote host**，输入 Paseo CLI 支持的固定 host 形式：

```text
host.example.com:6767
tcp://host.example.com:6767?ssl=true&password=secret
```

远程健康检查通过受限的只读 `ls` 请求验证连接，因为 Paseo CLI 0.2.2 的 `daemon status` 不支持 `--host`。应用不会自行改变 daemon 监听地址，也不会把 daemon 绑定到公网。

远程 host 可能包含 password。它通过现有工作区 secret store 保存；普通 workspace profile、`server_info`、MCP 错误和审计日志只表示 host 是否配置以及 local/remote，不返回完整值。

## Pairing offer 是敏感凭证

Pairing offer URL 可以授予连接能力，应按 bearer credential 处理：

- 不要粘贴到聊天、issue、截图或源码；
- 不要放进普通 workspace 配置、环境 dump 或调试日志；
- 怀疑泄露时在 Paseo 中撤销或轮换；
- Coding Tools MCP 的 redactor 会遮蔽识别到的 pairing URL，但这不是公开分享它的理由。

## 访问模式

| 模式 | 暴露能力 |
| --- | --- |
| Disabled | integration 关闭，不暴露任何 `paseo_` 工具 |
| Read only | health、agents、activity、permissions、diagnose、monitor snapshot |
| Assist | Read only + `paseo_send_agent_prompt` |
| Control | Assist + stop、精确 allow/deny permission、创建后台 agent |

Control 操作都要求 `confirm=true`。权限操作必须同时提供精确 `agent_id` 与 `request_id`，不暴露 `--all`；拒绝权限不会默认中断 agent。创建 agent 的 cwd 只能是当前工作区或其子目录。

## MCP 工具

### `paseo_health`

返回 CLI/daemon 可用性、版本、local/remote、访问模式与能力矩阵。`refresh=false` 可使用五秒健康缓存。`server_info` 只报告静态 integration 元数据，不触发健康检查。

### `paseo_list_agents`

使用稳定 agent ID 返回名称、状态、provider、workspace 和 CLI 可用的时间字段。支持 bounded status、workspace、glob name 与 label 过滤，最多返回 100 条。

### `paseo_get_agent_activity`

返回最近 1–100 个规范化事件，filter 为 `all`、`messages`、`tools` 或 `errors`。不支持 follow/streaming。完整消息、prompt、终端参数和路径不会回传。

### `paseo_list_pending_permissions`

查询待处理权限，返回精确 request ID、agent、类型、时间和脱敏摘要。Control 模式可把其中一个精确 request ID 交给 allow/deny 工具。

### `paseo_diagnose_agent`

返回固定 classification：

```text
HEALTHY
COMPLETED
WAITING_PERMISSION
WAITING_USER_INPUT
EXTERNAL_WAIT
POSSIBLY_STALLED
REPEATED_FAILURE
CRASHED
IDLE_INCOMPLETE
UNKNOWN
```

规则不调用模型。Paseo 0.2.2 的 activity 是无可靠时间戳的文本格式，因此第一次静默观察不会被判定为停滞；只有 CLI 时间证据或前后监控快照显示有效进展指纹持续不变，才计算 stalled duration。

### `paseo_monitor_snapshot`

一次完成代理发现、配置过滤、轻量诊断、前次比较和事件汇总。正常运行代理优先使用紧凑状态；只有潜在异常或达到检查阈值时才读取有限 activity。`refresh=false` 可直接读取已有快照而不运行 CLI。

快照保存在应用配置根目录下的 `data/paseo-monitor/<workspace-hash>/`，采用唯一临时文件加 rename，最多保留 24 份和 5 MiB。损坏文件会被忽略，实时查询仍继续。

### `paseo_send_agent_prompt`

仅 Assist/Control。prompt 必须为 1–8000 字符文本，始终作为一个进程参数传递，并固定使用 `--no-wait`。审计日志只记录 agent、脱敏 reason 和 prompt 长度。

### `paseo_stop_agent`

仅 Control，必须 `confirm=true` 且 reason 非空。Paseo CLI 将 `stop` 描述为中断当前运行，所以工具不声称删除或永久 kill 会话。返回停止前后状态，不提供 archive/delete/kill。

### `paseo_allow_permission` / `paseo_deny_permission`

仅 Control，必须 `confirm=true`，并使用 `paseo_list_pending_permissions` 返回的精确 `agent_id` 与 `request_id`。工具不会暴露 `--all`。deny 可把可选 reason 作为 Paseo denial message，但不会使用 `--interrupt`。

### `paseo_create_agent`

仅 Control，必须 `confirm=true`，并显式提供 provider（如 `codex/gpt-5.6-luna`）。使用 `paseo run --background --json` 创建 agent；prompt、title、provider 和 cwd 都作为独立进程参数传递。cwd 默认当前工作区，显式 cwd 也必须位于当前工作区内。

## ChatGPT 连接

Paseo 工具与普通 Coding Tools MCP 工具使用同一个 workspace MCP URL。启用 integration 并保存后：

1. 确认当前工作区 MCP 服务运行正常；
2. 在 Paseo 区域执行连接测试；
3. 断开并重新连接 ChatGPT MCP connector，或开始一个新的连接会话；
4. 调用 `server_info`，检查 `integrations.paseo.enabled`、mode 和 tool count；
5. 调用 `paseo_health`，再执行只读查询。

GPT Actions 使用同一实现和策略；OpenAPI 只包含当前 access mode 允许的 Paseo 工具。

## ChatGPT 每小时监控任务

可以把下面内容交给 ChatGPT 的定时任务：

```text
每小时调用 paseo_monitor_snapshot。

如果 changed=false 且没有新增异常、恢复或完成事件，不通知我。

如果有异常，对相关代理调用 paseo_diagnose_agent。

WAITING_PERMISSION：
只报告，不自动批准。

POSSIBLY_STALLED：
第一次调用 paseo_send_agent_prompt，要求代理总结最后成功步骤、阻塞位置并继续下一项可行操作。

REPEATED_FAILURE：
发送针对性提示，要求停止重复原操作，分析根因并改变方案。

同一代理连续两次仍无有效活动：
只建议人工停止；除非我明确启用了 control 模式并授权，否则不要调用 paseo_stop_agent。

通知应包含：
代理、状态、最后有效进展、卡住位置、判断依据、建议方案、已执行动作和是否需要用户处理。
```

定时任务不会自动批准权限，也不会由服务端自动发送 prompt 或 stop。所有动作仍由独立 MCP 调用触发。

## 常见错误

| 错误码 | 含义 | 建议 |
| --- | --- | --- |
| `PASEO_DISABLED` | 当前工作区未启用 | 在 Paseo Integration 中启用并保存 |
| `PASEO_ACCESS_DENIED` | access mode 不允许操作 | 使用只读工具或明确提升模式 |
| `PASEO_CLI_NOT_FOUND` | 找不到/不能启动 binary | 安装 CLI 或手动选择 binary |
| `PASEO_VERSION_UNSUPPORTED` | CLI 版本/文本格式不受支持 | 使用兼容版本并查看版本说明 |
| `PASEO_DAEMON_UNREACHABLE` | daemon/remote host 不可达 | 调用 health，检查 daemon、网络和 host |
| `PASEO_AUTH_FAILED` | 远程认证失败 | 检查或轮换 host secret |
| `PASEO_HOST_INVALID` | host 格式不合法 | 使用 `host:port` 或受支持的 `tcp://` 形式 |
| `PASEO_AGENT_NOT_FOUND` | agent ID 不存在或不可见 | 重新 list agents 并使用完整稳定 ID |
| `PASEO_ARGUMENT_INVALID` | 参数为空、超长或越界 | 按 tool schema 修正 |
| `PASEO_CLI_TIMEOUT` | CLI 超时并被终止 | 检查 daemon 负载，必要时谨慎提高 timeout |
| `PASEO_PARSE_PARTIAL` | 仅部分 activity 可识别或输出被截断 | 保留已识别事件，缩小 tail/max_bytes 后重试 |
| `PASEO_UNKNOWN_ACTIVITY` | 非空 activity 不符合已知 text/JSON 格式 | 核对 CLI 版本并采集脱敏后的原始输出样本 |
| `PASEO_OUTPUT_LIMIT` | JSON 超出上限而无法完整解析 | 收窄过滤或提高受限输出上限 |
| `PASEO_PARSE_ERROR` | CLI 返回未知格式 | 检查 CLI 版本和兼容说明 |
| `PASEO_COMMAND_FAILED` | CLI 非零退出 | 查看脱敏 stderr summary 并调用 health |
| `PASEO_CONFIRMATION_REQUIRED` | Control 操作缺少显式确认 | 核对目标后传 `confirm=true` |
| `PASEO_PERMISSION_NOT_FOUND` | 精确 agent/request ID 不再待处理 | 重新查询 pending permissions |
| `PASEO_RATE_LIMITED` | 请求过快或同操作正在进行 | 等待返回建议时间后重试 |
| `PASEO_SNAPSHOT_CORRUPTED` | 快照存储不可写/不可解析 | 清除当前工作区快照并重试 |

## 版本兼容

验证基线覆盖 Paseo CLI `0.2.x-0.3.x`，真实 smoke test 已覆盖 `0.2.5` 和 `0.3.0`：

- `ls`、`permit ls`、`send`、`stop` 与本地 `daemon status` 使用 JSON；
- `logs` 在命令级没有 `--json`，全局 `--json`/`--format json` 仍返回文本；
- activity parser 版本为 `paseo-0.2.x-0.3.x-activity-text-v3`，兼容旧 `---` 分隔、0.2.5 和 0.3.0 的逐行 `[Kind] payload` 输出，并把 `No activity to display.` 作为有效空结果；
- remote health 不使用不受支持的 `daemon status --host`。

工具结果返回 `source_format`、`parser_version`、CLI version、missing fields 和 truncation。未知 activity 文本版本会返回 `PASEO_VERSION_UNSUPPORTED`，不会猜测字段。

## 为什么不开放任意 Paseo 命令

通用 `paseo_exec` 会绕过 access mode、确认、参数上限、脱敏和审计，并可能暴露 archive/delete/kill、permission approval、daemon 修改等高风险能力。固定工具让 MCP schema、annotations、策略与测试保持一致，也能阻止 shell/flag 注入。

## 为什么只允许精确权限操作

权限请求可能授权文件写入、命令执行、网络访问或敏感资源。因此工具只在 Control 模式暴露，要求显式确认、精确 agent/request ID、并发去重和脱敏审计；不会提供批量 `--all`，也不会在 deny 时默认中断 agent。

## 完全关闭

1. 打开工作区的 MCP 配置。
2. 在 **Paseo Integration** 关闭“启用”。
3. 保存配置。
4. 重新连接 MCP 客户端，使工具清单刷新。
5. 可选点击“清除快照”，删除当前工作区的 Paseo 监控历史。

关闭后不暴露 `paseo_` 工具，也不会自动运行、探测或轮询 Paseo。
