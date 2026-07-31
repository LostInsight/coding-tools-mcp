# 任务清单：Cloudflare Named Tunnel 生命周期与路由同步

## Scope-lock

- 预计新增 1 个后端 Cloudflare API 模块和 1 个前端生命周期组件。
- 预计修改 tunnel supervisor、配置模型、secret allowlist、Tauri command、workspace 表单与 API 封装。
- 不修改 Quick Tunnel 运行逻辑、FRP 线路或未关联的运行时逻辑。

## 任务

- [x] 在真实环境确认公网 endpoint 当前返回 Cloudflare 1016，并确认持久化工作区为 Quick Tunnel。
- [x] 明确 Token/API Token、Account/Tunnel/Zone ID 的权限与职责边界。
- [x] 添加 Cloudflare API 请求、ingress/DNS 请求构造的失败测试。
- [x] 实现 Named Tunnel ingress/DNS 同步，并在 connector 启动前调用。
- [x] 修正 Named Tunnel ready 判定，避免 connector 已退出仍显示成功。
- [x] 新增 Named Tunnel API secret 与非敏感配置字段。
- [x] 新增状态查询、启动、停止、重启前端控件。
- [x] 跑完整构建门禁。
- [ ] 用授权的 Cloudflare 管理凭据真实注册并验证 `https://dell-coding-tools-mcp.gazy.top/mcp`。

## 风险

- 当前公网域名返回 1016，说明 DNS route 与可用 tunnel 尚未匹配。
- 当前网络对 QUIC/7844 存在间歇性失败；实现将保留 cloudflared 的 HTTP/2 回退，不把单次 QUIC 失败误判为已启动失败。
- 若同名 DNS 记录属于其它资源，默认拒绝覆盖；用户需显式打开覆盖开关。
