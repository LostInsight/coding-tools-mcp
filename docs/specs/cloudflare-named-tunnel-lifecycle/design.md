# 设计：Cloudflare Named Tunnel 生命周期与路由同步

## 凭据模型

```text
Tunnel Token                    -> cloudflared tunnel run --token
Cloudflare API Token            -> Cloudflare 管理 API Bearer 鉴权
Account ID + Tunnel ID          -> /accounts/{account}/cfd_tunnel/{tunnel}/configurations
Zone ID + hostname              -> /zones/{zone}/dns_records
```

Tunnel Token 与 API Token 的职责严格分离。前者不被用于写 DNS 或 ingress。

## 启动顺序

```text
用户点击启动 / 服务自动启动
  -> 校验 Named Tunnel 配置和 secret
  -> GET 当前 tunnel ingress
  -> PUT 当前 hostname 的 origin URL，保留其它规则
  -> 创建或更新 CNAME route
  -> cloudflared tunnel run --token
  -> 等待 "Registered tunnel connection"
  -> 返回运行状态
```

DNS 更新和 ingress 更新均使用 Cloudflare v4 API。DNS 操作只更新目标 hostname；默认拒绝覆盖同名非 CNAME 记录。远程 ingress 更新只替换相同 hostname 项，保留其它 hostname 和 catch-all 项。

## 数据模型

`TunnelConfig` 与 `ActionsConfig` 各增加以下非秘密字段：

- `cloudflare_account_id`
- `cloudflare_tunnel_id`
- `cloudflare_zone_id`
- `cloudflare_overwrite_dns`

每个服务各增加一个 workspace secret：

- `cloudflare_api_token`
- `actions_cloudflare_api_token`

## 错误与状态

Cloudflare API 使用统一 envelope 解析，失败信息只包含状态码与 Cloudflare 返回的安全错误摘要。Named connector 只有在输出包含 `Registered tunnel connection` 后才算 ready；进程输出结束不能伪造成功状态。

前端新增独立生命周期面板。配置保存不会自动把 Quick Tunnel 改为 Named Tunnel；Named Tunnel 管理 API 只在明确的 `named` 模式下调用。

## 测试策略

- 单元测试：hostname 规范化、ingress upsert、catch-all 保持最后、DNS 更新请求体与冲突判断。
- 回归测试：Quick Tunnel spawn 参数和 URL 解析保持不变；Named 输出未注册时不得报告 ready。
- 集成检查：Rust 测试、Svelte check、生产构建、Tauri release 构建。
- 真实验证：用最小权限 API Token 创建/更新 `dell-coding-tools-mcp.gazy.top`，启动本地 MCP 后发送 MCP `initialize`。
