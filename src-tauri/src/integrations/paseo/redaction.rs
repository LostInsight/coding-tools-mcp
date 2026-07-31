use std::sync::OnceLock;

use sha2::{Digest, Sha256};

const MAX_SUMMARY_CHARS: usize = 1_000;

pub fn redact(value: &str) -> String {
    let value = value.replace('\0', "");
    let value = pairing_pattern().replace_all(&value, "<pairing-offer-redacted>");
    let value = bearer_pattern().replace_all(&value, "$1<redacted>");
    let value = query_secret_pattern().replace_all(&value, "$1<redacted>");
    let value = env_pattern().replace_all(&value, "$1=<redacted>");
    let value = credential_pattern().replace_all(&value, "$1=<redacted>");
    let value = cookie_pattern().replace_all(&value, "$1<redacted>");
    let value = path_pattern().replace_all(&value, "<path>");
    bounded(&value, MAX_SUMMARY_CHARS)
}

pub fn redact_host(host: Option<&str>) -> String {
    let Some(host) = host.map(str::trim).filter(|host| !host.is_empty()) else {
        return "".into();
    };
    let redacted = query_secret_pattern().replace_all(host, "$1<redacted>");
    let without_scheme = redacted
        .strip_prefix("tcp://")
        .or_else(|| redacted.strip_prefix("https://"))
        .or_else(|| redacted.strip_prefix("http://"))
        .unwrap_or(&redacted);
    let visible = without_scheme.split('?').next().unwrap_or(without_scheme);
    let chars = visible.chars().collect::<Vec<_>>();
    if chars.len() <= 6 {
        return "<configured>".into();
    }
    format!(
        "{}…{}",
        chars[..3].iter().collect::<String>(),
        chars[chars.len() - 3..].iter().collect::<String>()
    )
}

pub fn normalize_error_signature(value: &str) -> String {
    let no_ansi = ansi_pattern().replace_all(value, "");
    let no_time = timestamp_pattern().replace_all(&no_ansi, "<time>");
    let no_uuid = uuid_pattern().replace_all(&no_time, "<id>");
    let no_paths = path_pattern().replace_all(&no_uuid, "<path>");
    let no_lines = line_pattern().replace_all(&no_paths, "line <n>");
    let normalized = bounded(&redact(&no_lines).to_ascii_lowercase(), 240);
    let digest = Sha256::digest(normalized.as_bytes());
    format!("err-{:x}", digest)[..20].to_string()
}

pub fn bounded(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn pairing_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r#"(?i)(paseo://[^\s"']+|pairing[-_ ]?offer[^\s"']*)"#)
            .expect("pairing regex")
    })
}

fn bearer_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"(?i)(bearer\s+)[A-Za-z0-9._~+/-]+").expect("bearer regex")
    })
}

fn query_secret_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"(?i)([?&](?:password|token|secret|key)=)[^&\s]+")
            .expect("query secret regex")
    })
}

fn env_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"(?im)\b([A-Z][A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|API_KEY))=([^\s]+)")
            .expect("env regex")
    })
}

fn credential_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"(?i)\b(password|token|secret|api[_-]?key)\s*[:=]\s*[^\s,&]+")
            .expect("credential regex")
    })
}

fn cookie_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"(?im)\b((?:set-)?cookie\s*[:=]\s*)[^\r\n]+").expect("cookie regex")
    })
}

fn ansi_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN.get_or_init(|| regex::Regex::new(r"\x1b\[[0-9;]*m").expect("ansi regex"))
}

fn timestamp_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        regex::Regex::new(r"\b\d{4}-\d{2}-\d{2}[T ][^\s]+\b").expect("timestamp regex")
    })
}

fn uuid_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| regex::Regex::new(r"\b[0-9a-f]{8}-[0-9a-f-]{27,36}\b").expect("uuid regex"))
}

fn path_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN.get_or_init(|| regex::Regex::new(r"(?i)(?:[a-z]:\\|/)[^\s:]+").expect("path regex"))
}

fn line_pattern() -> &'static regex::Regex {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    PATTERN.get_or_init(|| regex::Regex::new(r"(?i)line\s+\d+").expect("line regex"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_sensitive_values() {
        let result = redact(
            "Bearer abc123 password=secret API_TOKEN=value paseo://offer\nCookie: session=private",
        );
        assert!(!result.contains("abc123"));
        assert!(!result.contains("secret"));
        assert!(!result.contains("value"));
        assert!(!result.contains("paseo://offer"));
        assert!(!result.contains("private"));
    }

    #[test]
    fn signature_ignores_variable_identifiers() {
        let first = normalize_error_signature(
            "2026-01-01T10:00:00Z C:\\tmp\\a.rs line 9 uuid 123e4567-e89b-12d3-a456-426614174000",
        );
        let second = normalize_error_signature(
            "2026-02-02T10:00:00Z C:\\tmp\\b.rs line 42 uuid 123e4567-e89b-12d3-a456-426614174999",
        );
        assert_eq!(first, second);
    }
}
