# Cloudflare Named Tunnel 生命周期与路由同步

## 背景

当前应用仅以 Tunnel Token 执行 `cloudflared tunnel run --token`，并把固定公网 URL 当作展示值。该 token 只能启动 connector，不能创建 DNS CNAME 或更新远程 ingress，因此 Named Tunnel 无法保证 hostname 实际指向当前本地 MCP/Actions 端口。

真实环境检查显示 `https://dell-coding-tools-mcp.gazy.top/mcp` 返回 Cloudflare 1016，当前工作区同时保存为 Quick Tunnel；本需求修复这种配置与实际远程状态脱节的问题。

## 范围

### In scope

- Named Tunnel 的启动、停止、重启和状态查询。
- 使用 Cloudflare API 更新 tunnel ingress，服务地址为 `http://127.0.0.1:<local_port>`。
- 创建或更新 hostname 对应的 CNAME，目标为 `<tunnel_id>.cfargotunnel.com`。
- 配置 Account ID、Tunnel ID、Zone ID 和 API Token；Tunnel Token 仍只用于启动 connector。
- 在工作区界面提供明确的 Named Tunnel 管理控件和错误状态。
- 保持 Quick Tunnel 行为、命令行参数和 URL 发现逻辑不变。

### Out of scope

- 创建或删除 Cloudflare Tunnel 实体。
- 修改 Cloudflare Access 策略、WAF、证书或 Zone 本身。
- 在未授权的情况下覆盖第三方 DNS 记录。

## 功能需求

### FR-1：Named Tunnel 配置校验

当 Cloudflare 模式为 `named` 时，系统必须要求 Tunnel Token、API Token、Account ID、Tunnel ID、Zone ID 和 HTTPS 公网 URL。缺失任一项时不得启动 connector，并返回具体缺失项。

### FR-2：远程 ingress 同步

在启动 Named Tunnel 前，系统必须读取并更新目标 tunnel 的远程配置：将当前 hostname 的服务指向 `http://127.0.0.1:<服务端口>`，保留其他 hostname 规则，并保证 catch-all 规则位于最后。

### FR-3：DNS route 同步

系统必须创建或更新目标 hostname 的 CNAME，使其指向 `<tunnel_id>.cfargotunnel.com` 并启用 Cloudflare 代理。若同名记录不是 CNAME 且用户未显式允许覆盖，系统必须拒绝操作而非静默破坏记录。

### FR-4：明确生命周期管理

Named Tunnel 必须可在界面中独立查询状态、启动、停止和重启；状态需显示 PID、已配置 hostname 和失败原因。

### FR-5：Quick Tunnel 兼容

Quick Tunnel 不得调用 Cloudflare 管理 API，继续以 `cloudflared tunnel --url` 启动并从输出解析 `trycloudflare.com` URL。

## 非功能需求

- API Token 只能作为工作区 secret 存储，不能进入普通工作区 JSON 字段、日志或错误文本。
- 失败必须在启动前停止并返回 Cloudflare API 的安全摘要，不泄露 token。
- 远程配置更新不得删除不属于当前 hostname 的 ingress 项。
- 所有网络请求必须遵循应用的 tunnel 代理设置。

## 真实验证门槛

真实注册需要 API Token 至少具有 `Account / Cloudflare Tunnel / Edit` 与 `Zone / DNS / Edit` 权限，并配置 Account ID、Tunnel ID、Zone ID。验证通过的定义是公网 `initialize` 请求收到 MCP JSON-RPC 响应或认证挑战，而不是 Cloudflare 5xx。
