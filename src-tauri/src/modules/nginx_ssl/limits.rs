//! Per-site nginx/PHP tuning — upload size, memory, timeouts — plus a raw
//! directive escape hatch.
//!
//! Everything here is applied **inside the vhost this app already owns and
//! rewrites**, never by editing the machine's `php.ini` or `nginx.conf`. PHP
//! settings ride along as a `fastcgi_param PHP_VALUE`, which PHP-FPM applies
//! per-request. Two consequences worth knowing:
//!
//!   - Nothing needs a PHP-FPM restart. A plain `nginx -s reload` — which every
//!     save in this module already does — is enough.
//!   - Two sites on the same PHP-FPM pool can hold different limits without
//!     fighting over a shared file.
//!
//! Under Apache the same settings travel as `ProxyFCGISetEnvIf ... PHP_VALUE`, the
//! upload cap is `LimitRequestBody`, and the read timeout is `ProxyTimeout` — one
//! `SiteLimits` drives either server, so switching servers keeps every site's tuning.
//! The raw escape hatch is the exception: nginx and Apache syntax have nothing in
//! common, so each server has its own box.
//!
//! Only `PHP_INI_ALL` / `PHP_INI_PERDIR` directives can be set this way, which is
//! why the exposed set stops where it does. `opcache.*` and friends are
//! `PHP_INI_SYSTEM`: settable only in php.ini, and silently ignored if sent
//! through `PHP_VALUE`. Adding one here would appear to work and quietly do
//! nothing, so don't — it needs a different mechanism.

use serde::{Deserialize, Serialize};

/// All-`None` means "leave everything at the system default" — an untouched site
/// renders exactly the vhost it did before this module existed, so the migration
/// that backfills `NULL` needs no special case.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteLimits {
    /// One field drives three directives: nginx's `client_max_body_size` and PHP's
    /// `upload_max_filesize` + `post_max_size`. They are split in the underlying
    /// stack but a mismatch between them is never what anyone wants — it produces
    /// a 413 from nginx, or a silently empty `$_POST`, depending on which is
    /// smallest. Exposing one number makes that class of bug unreachable.
    pub max_upload_size: Option<String>,
    pub memory_limit: Option<String>,
    pub max_execution_time: Option<u32>,
    pub max_input_vars: Option<u32>,
    /// Verbatim nginx directives injected into the `server{}` block. Validated
    /// only for brace balance here; `nginx -t` in the same manifest is the real
    /// check, and a bad value fails the save rather than landing broken.
    pub extra_nginx: Option<String>,
    /// Verbatim Apache directives injected into the `<VirtualHost *:443>` block —
    /// the same escape hatch for the other server, checked by `httpd -t` instead.
    pub extra_apache: Option<String>,
}

/// Apache's `LimitRequestBody` ceiling. Anything larger is written as `0`, which
/// Apache reads as "no limit" — the closest it can get to what was asked for.
const APACHE_MAX_BODY: u64 = 2_147_483_647;

fn blank_to_none(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// An nginx/PHP size: digits with an optional K/M/G suffix. Both stacks accept
/// this same shape, which is why one validator covers both.
fn valid_size(value: &str) -> bool {
    let digits = value.strip_suffix(['K', 'M', 'G', 'k', 'm', 'g']).unwrap_or(value);
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

/// Bytes in an nginx/PHP size. A bare number is already bytes in both.
fn size_in_bytes(value: &str) -> Option<u64> {
    let (digits, unit) = match value.char_indices().last()? {
        (i, c) if c.is_ascii_alphabetic() => (&value[..i], c.to_ascii_uppercase()),
        _ => (value, 'B'),
    };
    let n: u64 = digits.parse().ok()?;
    let factor: u64 = match unit {
        'K' => 1 << 10,
        'M' => 1 << 20,
        'G' => 1 << 30,
        _ => 1,
    };
    n.checked_mul(factor)
}

/// The Apache counterpart of `braces_balanced`: a `</VirtualHost>` in the raw box
/// would close DevsPro's vhost early, and anything after it would land outside any
/// vhost — valid Apache that silently changes the server-wide config.
fn stays_inside_virtual_host(block: &str) -> bool {
    !block.to_ascii_lowercase().contains("virtualhost")
}

/// Rejects a raw block that would escape the `server{}` it gets embedded in.
/// `nginx -t` would catch an unbalanced trailing `}` as a syntax error, but a
/// *balanced* early close — `} server { ... }` — is valid nginx that silently
/// defines a second vhost outside our control. Depth must never go negative and
/// must land back at zero.
fn braces_balanced(block: &str) -> bool {
    let mut depth: i32 = 0;
    for c in block.chars() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

impl SiteLimits {
    /// Normalises blank strings to `None` and rejects malformed values, so a
    /// stored `SiteLimits` is always renderable.
    pub fn validate(self) -> Result<Self, String> {
        let limits = SiteLimits {
            max_upload_size: blank_to_none(self.max_upload_size),
            memory_limit: blank_to_none(self.memory_limit),
            extra_nginx: blank_to_none(self.extra_nginx),
            extra_apache: blank_to_none(self.extra_apache),
            ..self
        };

        if let Some(size) = &limits.max_upload_size {
            if !valid_size(size) {
                return Err(format!(
                    "max upload size \"{size}\" is not a valid size — use a number with an optional K, M or G suffix, e.g. 64M"
                ));
            }
        }
        if let Some(size) = &limits.memory_limit {
            // -1 is PHP's documented "no limit", and worth allowing: it's a
            // routine thing to want on a dev box running a big import.
            if size != "-1" && !valid_size(size) {
                return Err(format!(
                    "memory limit \"{size}\" is not a valid size — use a number with an optional K, M or G suffix (e.g. 512M), or -1 for no limit"
                ));
            }
        }
        if let Some(block) = &limits.extra_nginx {
            if !braces_balanced(block) {
                return Err(
                    "the extra nginx directives have unbalanced { } braces — every block must be closed inside this box"
                        .to_string(),
                );
            }
        }
        if let Some(block) = &limits.extra_apache {
            if !stays_inside_virtual_host(block) {
                return Err(
                    "the extra Apache directives may not open or close a <VirtualHost> — they already sit inside this site's"
                        .to_string(),
                );
            }
        }
        Ok(limits)
    }

    pub fn is_empty(&self) -> bool {
        *self == SiteLimits::default()
    }

    /// Directives for the `server{}` block, applying to proxy and PHP sites alike.
    pub fn server_directives(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(size) = &self.max_upload_size {
            lines.push(format!("client_max_body_size {size};"));
        }
        if let Some(block) = &self.extra_nginx {
            lines.extend(block.lines().map(|l| l.trim_end().to_string()));
        }
        lines
    }

    /// nginx's own read timeout, raised alongside PHP's `max_execution_time`.
    /// Without this, nginx returns its default 60s gateway timeout while PHP is
    /// still happily working — the request fails at exactly the limit the user
    /// thought they had just raised. The extra 10s keeps PHP the side that hits
    /// its limit first, so the error says what actually happened.
    pub fn read_timeout_directive(&self, upstream: &str) -> Option<String> {
        self.max_execution_time
            .map(|secs| format!("{upstream}_read_timeout {}s;", secs + 10))
    }

    /// The same knobs for Apache's `<VirtualHost>`: the upload cap, the proxy timeout
    /// raised past PHP's (for the same reason `read_timeout_directive` is), and the raw
    /// box — last, so a hand-written directive overrides the generated ones.
    pub fn apache_directives(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(bytes) = self.max_upload_size.as_deref().and_then(size_in_bytes) {
            let bytes = if bytes > APACHE_MAX_BODY { 0 } else { bytes };
            lines.push(format!("LimitRequestBody {bytes}"));
        }
        if let Some(secs) = self.max_execution_time {
            lines.push(format!("ProxyTimeout {}", secs + 10));
        }
        if let Some(block) = &self.extra_apache {
            lines.extend(block.lines().map(|l| l.trim_end().to_string()));
        }
        lines
    }

    /// `php_value_directive` for Apache. `\n` inside an Apache expression string is a
    /// real newline by the time PHP-FPM reads it — checked against a live Apache 2.4.
    pub fn apache_php_value_directive(&self) -> Option<String> {
        let settings = self.php_settings();
        if settings.is_empty() {
            return None;
        }
        Some(format!(
            "ProxyFCGISetEnvIf \"true\" PHP_VALUE \"{}\"",
            settings.join("\\n")
        ))
    }

    fn php_settings(&self) -> Vec<String> {
        let mut settings = Vec::new();
        if let Some(size) = &self.max_upload_size {
            settings.push(format!("upload_max_filesize={size}"));
            settings.push(format!("post_max_size={size}"));
        }
        if let Some(limit) = &self.memory_limit {
            settings.push(format!("memory_limit={limit}"));
        }
        if let Some(secs) = self.max_execution_time {
            settings.push(format!("max_execution_time={secs}"));
        }
        if let Some(vars) = self.max_input_vars {
            settings.push(format!("max_input_vars={vars}"));
        }
        settings
    }

    /// PHP settings as a single `fastcgi_param PHP_VALUE`, newline-separated —
    /// the format PHP-FPM parses. `None` when nothing PHP-side is set, so an
    /// untouched site renders no param at all.
    pub fn php_value_directive(&self) -> Option<String> {
        let settings = self.php_settings();
        if settings.is_empty() {
            return None;
        }
        Some(format!(
            "fastcgi_param PHP_VALUE \"{}\";",
            settings.join("\n                    ")
        ))
    }
}

/// Round-trips through the single `limits` TEXT column. A malformed or absent
/// value degrades to defaults rather than failing the whole site lookup — a site
/// that loads with system-default limits beats a site that won't list at all.
pub fn from_column(raw: Option<String>) -> SiteLimits {
    raw.and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

pub fn to_column(limits: &SiteLimits) -> Option<String> {
    if limits.is_empty() {
        return None;
    }
    serde_json::to_string(limits).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uploads(size: &str) -> SiteLimits {
        SiteLimits {
            max_upload_size: Some(size.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn valid_size_accepts_nginx_and_php_shapes() {
        assert!(valid_size("64M"));
        assert!(valid_size("512"));
        assert!(valid_size("2G"));
        assert!(valid_size("1024k"));
    }

    #[test]
    fn valid_size_rejects_junk() {
        assert!(!valid_size(""));
        assert!(!valid_size("M"));
        assert!(!valid_size("64MB"));
        assert!(!valid_size("64 M"));
        assert!(!valid_size("-1"));
        assert!(!valid_size("6.4M"));
    }

    #[test]
    fn memory_limit_allows_unlimited_but_upload_does_not() {
        assert!(SiteLimits { memory_limit: Some("-1".into()), ..Default::default() }
            .validate()
            .is_ok());
        assert!(uploads("-1").validate().is_err());
    }

    #[test]
    fn validate_blanks_out_empty_strings() {
        let limits = SiteLimits {
            max_upload_size: Some("   ".into()),
            extra_nginx: Some("".into()),
            ..Default::default()
        }
        .validate()
        .unwrap();
        assert!(limits.is_empty());
    }

    #[test]
    fn braces_balanced_accepts_ordinary_blocks() {
        assert!(braces_balanced("gzip on;"));
        assert!(braces_balanced("location /health {\n  return 200;\n}"));
        assert!(braces_balanced(""));
    }

    #[test]
    fn braces_balanced_rejects_escaping_the_server_block() {
        // The dangerous case: balanced overall, but closes our server{} early and
        // opens a second vhost we would not control.
        assert!(!braces_balanced("} server { listen 8080; "));
        assert!(!braces_balanced("}"));
        assert!(!braces_balanced("location / {"));
    }

    #[test]
    fn upload_size_drives_all_three_directives() {
        let limits = uploads("64M");
        assert_eq!(limits.server_directives(), vec!["client_max_body_size 64M;"]);
        let php = limits.php_value_directive().unwrap();
        assert!(php.contains("upload_max_filesize=64M"));
        assert!(php.contains("post_max_size=64M"));
    }

    #[test]
    fn untouched_limits_render_nothing() {
        let limits = SiteLimits::default();
        assert!(limits.server_directives().is_empty());
        assert_eq!(limits.php_value_directive(), None);
        assert_eq!(limits.read_timeout_directive("fastcgi"), None);
    }

    #[test]
    fn read_timeout_outlives_php_execution_limit() {
        let limits = SiteLimits {
            max_execution_time: Some(120),
            ..Default::default()
        };
        assert_eq!(
            limits.read_timeout_directive("fastcgi"),
            Some("fastcgi_read_timeout 130s;".to_string())
        );
        assert_eq!(
            limits.read_timeout_directive("proxy"),
            Some("proxy_read_timeout 130s;".to_string())
        );
    }

    #[test]
    fn extra_nginx_lines_land_in_the_server_block() {
        let limits = SiteLimits {
            extra_nginx: Some("gzip on;\nadd_header X-Dev 1;".into()),
            ..Default::default()
        };
        assert_eq!(
            limits.server_directives(),
            vec!["gzip on;", "add_header X-Dev 1;"]
        );
    }

    #[test]
    fn column_round_trip_preserves_values() {
        let limits = SiteLimits {
            max_upload_size: Some("64M".into()),
            memory_limit: Some("512M".into()),
            max_execution_time: Some(120),
            max_input_vars: Some(5000),
            extra_nginx: Some("gzip on;".into()),
            extra_apache: Some("Header set X-Dev 1".into()),
        };
        let stored = to_column(&limits).unwrap();
        assert_eq!(from_column(Some(stored)), limits);
    }

    #[test]
    fn empty_limits_store_as_null_and_read_back_as_default() {
        assert_eq!(to_column(&SiteLimits::default()), None);
        assert_eq!(from_column(None), SiteLimits::default());
        // Rows written before this column existed, and any value that stops
        // parsing after a future field change, must not break `list()`.
        assert_eq!(from_column(Some("not json".into())), SiteLimits::default());
    }

    #[test]
    fn apache_upload_size_is_bytes_and_huge_means_unlimited() {
        assert_eq!(uploads("64M").apache_directives(), vec!["LimitRequestBody 67108864"]);
        assert_eq!(uploads("1024").apache_directives(), vec!["LimitRequestBody 1024"]);
        assert_eq!(uploads("4G").apache_directives(), vec!["LimitRequestBody 0"]);
    }

    #[test]
    fn apache_timeout_outlives_php_execution_limit() {
        let limits = SiteLimits {
            max_execution_time: Some(120),
            ..Default::default()
        };
        assert_eq!(limits.apache_directives(), vec!["ProxyTimeout 130"]);
    }

    #[test]
    fn apache_php_value_is_one_expression_with_escaped_newlines() {
        let limits = SiteLimits {
            memory_limit: Some("512M".into()),
            max_execution_time: Some(120),
            ..Default::default()
        };
        assert_eq!(
            limits.apache_php_value_directive().as_deref(),
            Some(r#"ProxyFCGISetEnvIf "true" PHP_VALUE "memory_limit=512M\nmax_execution_time=120""#)
        );
        assert_eq!(SiteLimits::default().apache_php_value_directive(), None);
    }

    #[test]
    fn extra_apache_cannot_close_the_virtual_host() {
        let escaping = SiteLimits {
            extra_apache: Some("</VirtualHost>\n<VirtualHost *:8080>".into()),
            ..Default::default()
        };
        assert!(escaping.validate().is_err());
        let fine = SiteLimits {
            extra_apache: Some("Header set X-Dev 1".into()),
            ..Default::default()
        };
        assert_eq!(fine.validate().unwrap().apache_directives(), vec!["Header set X-Dev 1"]);
    }
}
