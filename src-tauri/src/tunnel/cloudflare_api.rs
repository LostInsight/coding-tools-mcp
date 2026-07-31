use std::time::Duration;

use reqwest::{Client, Url};
use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::settings::ProxyConfig;

const CLOUDFLARE_API_BASE: &str = "https://api.cloudflare.com/client/v4";

/// Credentials and identifiers required to manage a remotely configured
/// Cloudflare Named Tunnel. The API token is kept in the workspace secret store.
pub(crate) struct NamedTunnelRoute {
    pub account_id: String,
    pub tunnel_id: String,
    pub zone_id: String,
    pub api_token: String,
    pub public_url: String,
    pub local_port: u16,
    pub overwrite_dns: bool,
}

impl NamedTunnelRoute {
    pub fn validate(&self) -> AppResult<String> {
        require_value("Cloudflare Account ID", &self.account_id)?;
        require_value("Cloudflare Tunnel ID", &self.tunnel_id)?;
        require_value("Cloudflare Zone ID", &self.zone_id)?;
        require_value("Cloudflare API Token", &self.api_token)?;
        if self.local_port == 0 {
            return Err(AppError::Message(
                "Cloudflare Named Tunnel 本地端口无效。".into(),
            ));
        }
        validate_identifier("Cloudflare Account ID", &self.account_id)?;
        validate_identifier("Cloudflare Tunnel ID", &self.tunnel_id)?;
        validate_identifier("Cloudflare Zone ID", &self.zone_id)?;
        normalize_named_hostname(&self.public_url)
    }
}

/// Synchronize the Cloudflare-managed ingress and DNS record before starting
/// the local connector. Tunnel Token authentication is deliberately not used
/// here: it can run a connector but cannot manage DNS or remote configuration.
pub(crate) async fn sync_named_tunnel_route(
    route: &NamedTunnelRoute,
    proxy: &ProxyConfig,
    use_proxy: bool,
) -> AppResult<()> {
    sync_named_tunnel_route_at_base(route, proxy, use_proxy, CLOUDFLARE_API_BASE).await
}

async fn sync_named_tunnel_route_at_base(
    route: &NamedTunnelRoute,
    proxy: &ProxyConfig,
    use_proxy: bool,
    api_base: &str,
) -> AppResult<()> {
    let hostname = route.validate()?;
    let client = build_client(proxy, use_proxy)?;
    let api_base = api_base.trim_end_matches('/');
    let config_url = format!(
        "{api_base}/accounts/{}/cfd_tunnel/{}/configurations",
        route.account_id.trim(),
        route.tunnel_id.trim()
    );

    let current = api_json(
        client.get(&config_url).bearer_auth(route.api_token.trim()),
        "读取 Named Tunnel ingress",
    )
    .await?;
    let current_config = current.get("config").cloned().unwrap_or(current);
    let origin = format!("http://127.0.0.1:{}", route.local_port);
    let updated_config = upsert_ingress_config(current_config, &hostname, &origin);

    api_json(
        client
            .put(&config_url)
            .bearer_auth(route.api_token.trim())
            .json(&json!({ "config": updated_config })),
        "更新 Named Tunnel ingress",
    )
    .await?;

    ensure_dns_route(&client, route, &hostname, api_base).await?;

    Ok(())
}

pub(crate) fn normalize_named_hostname(public_url: &str) -> AppResult<String> {
    let url = Url::parse(public_url.trim()).map_err(|_| {
        AppError::Message("Cloudflare Named Tunnel 需要有效的 HTTPS 公网 URL。".into())
    })?;
    if url.scheme() != "https" {
        return Err(AppError::Message(
            "Cloudflare Named Tunnel 公网 URL 必须使用 HTTPS。".into(),
        ));
    }
    if url.username() != "" || url.password().is_some() || url.port().is_some() {
        return Err(AppError::Message(
            "Cloudflare Named Tunnel 公网 URL 不能包含用户名、密码或端口。".into(),
        ));
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(AppError::Message(
            "Cloudflare Named Tunnel 公网 URL 只能包含协议和 hostname，例如 https://example.com。"
                .into(),
        ));
    }
    let hostname = url
        .host_str()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if hostname.is_empty() {
        return Err(AppError::Message(
            "Cloudflare Named Tunnel 公网 URL 缺少 hostname。".into(),
        ));
    }
    Ok(hostname)
}

/// Replace only the configured hostname and preserve all other remote ingress
/// rules. Cloudflare requires the catch-all service rule to be the last entry.
pub(crate) fn upsert_ingress_config(mut config: Value, hostname: &str, service: &str) -> Value {
    if !config.is_object() {
        config = json!({});
    }
    let ingress = config
        .get_mut("ingress")
        .and_then(Value::as_array_mut)
        .map(std::mem::take)
        .unwrap_or_default();

    let mut hostname_rules = Vec::new();
    let mut catch_all_rules = Vec::new();
    for rule in ingress {
        if rule
            .get("hostname")
            .and_then(Value::as_str)
            .is_some_and(|current| current.eq_ignore_ascii_case(hostname))
        {
            continue;
        }
        if is_catch_all_rule(&rule) {
            catch_all_rules.push(rule);
        } else {
            hostname_rules.push(rule);
        }
    }

    hostname_rules.push(json!({ "hostname": hostname, "service": service }));
    if catch_all_rules.is_empty() {
        catch_all_rules.push(json!({ "service": "http_status:404" }));
    }
    hostname_rules.extend(catch_all_rules);
    config["ingress"] = Value::Array(hostname_rules);
    config
}

pub(crate) fn dns_record_payload(hostname: &str, tunnel_id: &str) -> Value {
    json!({
        "type": "CNAME",
        "name": hostname,
        "content": format!("{}.cfargotunnel.com", tunnel_id.trim()),
        "proxied": true,
        "ttl": 1,
    })
}

fn is_catch_all_rule(rule: &Value) -> bool {
    rule.get("hostname").is_none() && rule.get("service").is_some()
}

fn build_client(proxy: &ProxyConfig, use_proxy: bool) -> AppResult<Client> {
    let mut builder = Client::builder().timeout(Duration::from_secs(30));
    if !use_proxy || proxy.mode.trim() == "none" {
        builder = builder.no_proxy();
    } else if proxy.mode.trim() == "manual" {
        let url = proxy.url.trim();
        if url.is_empty() {
            return Err(AppError::Message(
                "已启用网络代理，但未配置 Cloudflare API 代理地址。".into(),
            ));
        }
        let configured_proxy = reqwest::Proxy::all(url)
            .map_err(|err| AppError::Message(format!("Cloudflare API 代理地址无效: {err}")))?;
        builder = builder.proxy(configured_proxy);
    }
    builder
        .build()
        .map_err(|err| AppError::Message(format!("创建 Cloudflare API 客户端失败: {err}")))
}

async fn ensure_dns_route(
    client: &Client,
    route: &NamedTunnelRoute,
    hostname: &str,
    api_base: &str,
) -> AppResult<()> {
    let records_url = format!(
        "{api_base}/zones/{}/dns_records",
        route.zone_id.trim()
    );
    let records = api_json(
        client
            .get(&records_url)
            .bearer_auth(route.api_token.trim())
            .query(&[("name", hostname)]),
        "读取 Cloudflare DNS route",
    )
    .await?;
    let records = records
        .as_array()
        .ok_or_else(|| AppError::Message("Cloudflare DNS route 查询返回了无效结果。".into()))?;
    let target = format!("{}.cfargotunnel.com", route.tunnel_id.trim()).to_ascii_lowercase();
    let mut matching_cname = None;
    let mut conflicts = Vec::new();

    for record in records {
        let Some(record_id) = record.get("id").and_then(Value::as_str) else {
            continue;
        };
        let record_type = record
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let content = record
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim_end_matches('.')
            .to_ascii_lowercase();
        if record_type == "CNAME" && content == target {
            matching_cname = Some(record_id.to_string());
        } else {
            conflicts.push(record_id.to_string());
        }
    }

    if !conflicts.is_empty() {
        if !route.overwrite_dns {
            return Err(AppError::Message(format!(
                "Cloudflare DNS 中的 {hostname} 已存在非目标 Tunnel 的记录。确认无误后可启用“覆盖现有 DNS 记录”。"
            )));
        }
        for record_id in conflicts {
            api_json(
                client
                    .delete(format!("{records_url}/{record_id}"))
                    .bearer_auth(route.api_token.trim()),
                "删除冲突的 Cloudflare DNS route",
            )
            .await?;
        }
    }

    let payload = dns_record_payload(hostname, &route.tunnel_id);
    if let Some(record_id) = matching_cname {
        api_json(
            client
                .put(format!("{records_url}/{record_id}"))
                .bearer_auth(route.api_token.trim())
                .json(&payload),
            "更新 Cloudflare DNS route",
        )
        .await?;
    } else {
        api_json(
            client
                .post(&records_url)
                .bearer_auth(route.api_token.trim())
                .json(&payload),
            "创建 Cloudflare DNS route",
        )
        .await?;
    }
    Ok(())
}

async fn api_json(request: reqwest::RequestBuilder, action: &str) -> AppResult<Value> {
    let response = request
        .send()
        .await
        .map_err(|err| AppError::Message(format!("Cloudflare API {action}失败: {err}")))?;
    let status = response.status();
    let body = response.json::<Value>().await.map_err(|err| {
        AppError::Message(format!(
            "Cloudflare API {action}返回了无法解析的响应: {err}"
        ))
    })?;
    if !status.is_success()
        || !body
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return Err(AppError::Message(format!(
            "Cloudflare API {action}失败（HTTP {}）：{}",
            status.as_u16(),
            api_error_summary(&body)
        )));
    }
    Ok(body.get("result").cloned().unwrap_or(Value::Null))
}

fn api_error_summary(body: &Value) -> String {
    let summary = body
        .get("errors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("message").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("；");
    if summary.is_empty() {
        "Cloudflare 未返回详细错误。".into()
    } else {
        summary.chars().take(500).collect()
    }
}

fn require_value(label: &str, value: &str) -> AppResult<()> {
    if value.trim().is_empty() {
        return Err(AppError::Message(format!(
            "Cloudflare Named Tunnel 需要填写 {label}。"
        )));
    }
    Ok(())
}

fn validate_identifier(label: &str, value: &str) -> AppResult<()> {
    if value
        .trim()
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        Ok(())
    } else {
        Err(AppError::Message(format!("{label} 格式无效。")))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        body::to_bytes,
        extract::{Request, State},
        http::{Method, StatusCode},
        routing::any,
        Json, Router,
    };
    use serde_json::json;
    use tokio::net::TcpListener;

    use super::{
        dns_record_payload, normalize_named_hostname, sync_named_tunnel_route_at_base,
        upsert_ingress_config, NamedTunnelRoute,
    };
    use crate::settings::ProxyConfig;

    #[test]
    fn normalizes_https_named_hostname() {
        assert_eq!(
            normalize_named_hostname("https://dell-coding-tools-mcp.gazy.top/").unwrap(),
            "dell-coding-tools-mcp.gazy.top"
        );
        assert!(normalize_named_hostname("http://dell-coding-tools-mcp.gazy.top").is_err());
        assert!(normalize_named_hostname("https://dell-coding-tools-mcp.gazy.top/mcp").is_err());
    }

    #[test]
    fn ingress_upsert_preserves_other_hosts_and_moves_catch_all_last() {
        let existing = json!({
            "ingress": [
                { "service": "http_status:404" },
                { "hostname": "other.gazy.top", "service": "http://127.0.0.1:3000" },
                { "hostname": "dell-coding-tools-mcp.gazy.top", "service": "http://127.0.0.1:1" }
            ]
        });

        let updated = upsert_ingress_config(
            existing,
            "dell-coding-tools-mcp.gazy.top",
            "http://127.0.0.1:28767",
        );
        let ingress = updated["ingress"].as_array().unwrap();

        assert_eq!(ingress.len(), 3);
        assert_eq!(ingress[0]["hostname"], "other.gazy.top");
        assert_eq!(ingress[1]["hostname"], "dell-coding-tools-mcp.gazy.top");
        assert_eq!(ingress[1]["service"], "http://127.0.0.1:28767");
        assert_eq!(ingress[2]["service"], "http_status:404");
    }

    #[test]
    fn dns_payload_targets_tunnel_cname() {
        let payload = dns_record_payload(
            "dell-coding-tools-mcp.gazy.top",
            "75e5b9e6-98eb-4f9b-9e66-8866c866f9b0",
        );

        assert_eq!(payload["type"], "CNAME");
        assert_eq!(
            payload["content"],
            "75e5b9e6-98eb-4f9b-9e66-8866c866f9b0.cfargotunnel.com"
        );
        assert_eq!(payload["proxied"], true);
    }

    #[test]
    fn named_route_requires_an_api_token() {
        let route = NamedTunnelRoute {
            account_id: "0123456789abcdef0123456789abcdef".into(),
            tunnel_id: "75e5b9e6-98eb-4f9b-9e66-8866c866f9b0".into(),
            zone_id: "abcdef0123456789abcdef0123456789".into(),
            api_token: String::new(),
            public_url: "https://dell-coding-tools-mcp.gazy.top".into(),
            local_port: 28767,
            overwrite_dns: false,
        };

        let error = route.validate().unwrap_err();
        assert!(error.to_string().contains("API Token"));
    }

    #[tokio::test]
    async fn sync_updates_ingress_before_creating_dns_route() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .fallback(any(mock_cloudflare_api))
            .with_state(requests.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let route = NamedTunnelRoute {
            account_id: "account".into(),
            tunnel_id: "tunnel".into(),
            zone_id: "zone".into(),
            api_token: "test-api-token".into(),
            public_url: "https://dell-coding-tools-mcp.gazy.top".into(),
            local_port: 28767,
            overwrite_dns: false,
        };
        let base_url = format!("http://{address}/client/v4");

        sync_named_tunnel_route_at_base(&route, &ProxyConfig::default(), false, &base_url)
            .await
            .unwrap();

        server.abort();
        let _ = server.await;

        let requests = requests.lock().unwrap();
        let steps = requests
            .iter()
            .map(|request: &RecordedRequest| format!("{} {}", request.method, request.path))
            .collect::<Vec<_>>();
        assert_eq!(
            steps,
            vec![
                "GET /client/v4/accounts/account/cfd_tunnel/tunnel/configurations",
                "PUT /client/v4/accounts/account/cfd_tunnel/tunnel/configurations",
                "GET /client/v4/zones/zone/dns_records",
                "POST /client/v4/zones/zone/dns_records",
            ]
        );
        assert_eq!(
            requests[1].body["config"]["ingress"][0]["service"],
            "http://127.0.0.1:28767"
        );
        assert_eq!(
            requests[3].body["content"],
            "tunnel.cfargotunnel.com"
        );
    }

    #[derive(Debug)]
    struct RecordedRequest {
        method: String,
        path: String,
        body: serde_json::Value,
    }

    async fn mock_cloudflare_api(
        State(requests): State<Arc<Mutex<Vec<RecordedRequest>>>>,
        request: Request,
    ) -> (StatusCode, Json<serde_json::Value>) {
        let method = request.method().clone();
        let path = request.uri().path().to_string();
        let body = to_bytes(request.into_body(), usize::MAX).await.unwrap();
        let body = if body.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&body).unwrap()
        };
        requests.lock().unwrap().push(RecordedRequest {
            method: method.to_string(),
            path: path.clone(),
            body,
        });

        let result = match (method, path.as_str()) {
            (Method::GET, "/client/v4/accounts/account/cfd_tunnel/tunnel/configurations") => {
                json!({
                    "success": true,
                    "result": {
                        "config": {
                            "ingress": [{ "service": "http_status:404" }]
                        }
                    }
                })
            }
            (Method::PUT, "/client/v4/accounts/account/cfd_tunnel/tunnel/configurations") => {
                json!({ "success": true, "result": {} })
            }
            (Method::GET, "/client/v4/zones/zone/dns_records") => {
                json!({ "success": true, "result": [] })
            }
            (Method::POST, "/client/v4/zones/zone/dns_records") => {
                json!({ "success": true, "result": {} })
            }
            _ => json!({
                "success": false,
                "errors": [{ "message": "unexpected request" }]
            }),
        };

        (StatusCode::OK, Json(result))
    }
}
