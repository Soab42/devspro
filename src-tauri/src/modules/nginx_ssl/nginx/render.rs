//! Vhost generation. Plain string templating, not a template engine — the structure
//! is fixed and entirely known ahead of time, and the only variable pieces
//! (hostname/port/paths) are validated before they ever reach here (see
//! `sites::valid_hostname`, `laravel::is_laravel_project`, `limits::validate`), so a
//! template engine would add a dependency without buying anything.

use crate::modules::nginx_ssl::limits::SiteLimits;

pub enum BackendTarget {
    Proxy { port: u16 },
    /// Laravel is served directly by nginx + PHP-FPM through its `public/`
    /// front controller — no `artisan serve`, no port to manage.
    Laravel { public_root: String, php_fpm_socket: String },
}

/// Renders `lines` at `indent`, one per line with a trailing newline — or nothing
/// at all when empty, so a site with no tuning renders the same vhost it always
/// did rather than one pockmarked with blank lines.
fn indented(lines: Vec<String>, indent: &str) -> String {
    lines.iter().map(|l| format!("{indent}{l}\n")).collect()
}

fn location_block(backend: &BackendTarget, limits: &SiteLimits) -> String {
    match backend {
        BackendTarget::Proxy { port } => {
            let tuning = indented(
                limits.read_timeout_directive("proxy").into_iter().collect(),
                "        ",
            );
            format!(
                r#"    location / {{
        proxy_pass http://127.0.0.1:{port};
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
{tuning}    }}"#
            )
        }
        BackendTarget::Laravel {
            public_root,
            php_fpm_socket,
        } => {
            let tuning = indented(
                limits
                    .php_value_directive()
                    .into_iter()
                    .chain(limits.read_timeout_directive("fastcgi"))
                    .collect(),
                "        ",
            );
            format!(
                r#"    root {public_root};
    index index.php index.html;

    location / {{
        try_files $uri $uri/ /index.php?$query_string;
    }}

    location ~ \.php$ {{
        include fastcgi_params;
        fastcgi_pass unix:{php_fpm_socket};
        fastcgi_param SCRIPT_FILENAME $realpath_root$fastcgi_script_name;
{tuning}    }}

    location ~ /\.(?!well-known).* {{
        deny all;
    }}"#
            )
        }
    }
}

pub fn render_site_conf(
    hostname: &str,
    backend: &BackendTarget,
    limits: &SiteLimits,
    cert_path: &str,
    key_path: &str,
    access_log: &str,
    error_log: &str,
) -> String {
    let location = location_block(backend, limits);
    // Server-level tuning sits above the location block so a raw directive can be
    // overridden by the generated ones rather than the other way round — nginx
    // takes the last matching directive at a given level.
    let server_tuning = indented(limits.server_directives(), "    ");
    format!(
        r#"# Managed by DevsPro — do not edit by hand, changes will be overwritten on next save.
server {{
    listen 80;
    server_name {hostname};
    return 301 https://$host$request_uri;
}}

server {{
    listen 443 ssl;
    server_name {hostname};

    ssl_certificate     {cert_path};
    ssl_certificate_key {key_path};

    access_log {access_log};
    error_log  {error_log};

{server_tuning}
{location}
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laravel() -> BackendTarget {
        BackendTarget::Laravel {
            public_root: "/home/dev/app/public".to_string(),
            php_fpm_socket: "/run/php/php8.3-fpm.sock".to_string(),
        }
    }

    fn render(backend: &BackendTarget, limits: &SiteLimits) -> String {
        render_site_conf(
            "app.test",
            backend,
            limits,
            "/certs/app.test.pem",
            "/certs/app.test-key.pem",
            "/logs/app.access.log",
            "/logs/app.error.log",
        )
    }

    #[test]
    fn default_limits_add_no_directives() {
        let conf = render(&laravel(), &SiteLimits::default());
        assert!(!conf.contains("client_max_body_size"));
        assert!(!conf.contains("PHP_VALUE"));
        assert!(!conf.contains("fastcgi_read_timeout"));
    }

    #[test]
    fn upload_size_reaches_both_nginx_and_php() {
        let limits = SiteLimits {
            max_upload_size: Some("64M".into()),
            ..Default::default()
        };
        let conf = render(&laravel(), &limits);
        assert!(conf.contains("client_max_body_size 64M;"));
        assert!(conf.contains("upload_max_filesize=64M"));
        assert!(conf.contains("post_max_size=64M"));
    }

    #[test]
    fn proxy_sites_get_the_nginx_half_only() {
        // A proxied app enforces its own body limits; only nginx's cap and the
        // read timeout are ours to set, and PHP_VALUE would be meaningless here.
        let limits = SiteLimits {
            max_upload_size: Some("64M".into()),
            max_execution_time: Some(120),
            ..Default::default()
        };
        let conf = render(&BackendTarget::Proxy { port: 3000 }, &limits);
        assert!(conf.contains("client_max_body_size 64M;"));
        assert!(conf.contains("proxy_read_timeout 130s;"));
        assert!(!conf.contains("PHP_VALUE"));
    }

    #[test]
    fn extra_directives_land_inside_the_server_block() {
        let limits = SiteLimits {
            extra_nginx: Some("add_header X-Dev 1;".into()),
            ..Default::default()
        };
        let conf = render(&laravel(), &limits);
        let body = conf.split("listen 443 ssl;").nth(1).unwrap();
        assert!(body.contains("add_header X-Dev 1;"));
        // Must be inside the block, i.e. before its closing brace.
        assert!(body.find("add_header").unwrap() < body.rfind('}').unwrap());
    }
}
