//! Vhost generation for Apache — the counterpart of `nginx::render`, producing the same
//! site from the same inputs: HTTP redirected to HTTPS, TLS with the site's leaf
//! certificate, per-site logs at the paths the Logs module already tails, and either a
//! reverse proxy to a port or a Laravel `public/` served through PHP's FastCGI listener.
//!
//! Plain string templating for the same reason as the nginx side: the structure is fixed
//! and every variable piece is validated before it gets here.

use super::platform;
use crate::modules::nginx_ssl::limits::SiteLimits;
use crate::modules::nginx_ssl::nginx::render::BackendTarget;

/// A path as one Apache argument. Apache unescapes `\\` and `\"` inside quotes the same
/// way nginx does, so the escaping is shared in spirit — but the path conversion is
/// Apache's own, kept separate so the two servers can never disagree by accident.
pub fn quoted(path: &str) -> String {
    let path = platform::to_config_path(path).replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{path}\"")
}

fn indented(lines: Vec<String>, indent: &str) -> String {
    lines.iter().map(|l| format!("{indent}{l}\n")).collect()
}

fn backend_block(backend: &BackendTarget, limits: &SiteLimits) -> String {
    match backend {
        BackendTarget::Proxy { port } => format!(
            r#"    ProxyPreserveHost On
    ProxyPass / http://127.0.0.1:{port}/
    ProxyPassReverse / http://127.0.0.1:{port}/
    <IfModule headers_module>
        RequestHeader set X-Forwarded-Proto "https"
    </IfModule>
"#
        ),
        BackendTarget::Laravel {
            public_root,
            fastcgi_pass,
        } => {
            let root = quoted(public_root);
            let handler = platform::fcgi_handler(fastcgi_pass);
            let php = indented(
                platform::script_filename_fix()
                    .map(str::to_string)
                    .into_iter()
                    .chain(limits.apache_php_value_directive())
                    .collect(),
                "    ",
            );
            format!(
                r#"    DocumentRoot {root}
    <Directory {root}>
        Options FollowSymLinks
        AllowOverride All
        Require all granted
        DirectoryIndex index.php index.html
        CGIPassAuth On
        <IfModule dir_module>
            FallbackResource /index.php
        </IfModule>
    </Directory>

    <FilesMatch "\.php$">
        SetHandler "{handler}"
    </FilesMatch>
{php}
    <LocationMatch "/\.(?!well-known)">
        Require all denied
    </LocationMatch>
"#
            )
        }
    }
}

/// Every path written into the result goes through `quoted` first, for the same two
/// reasons as on the nginx side: Windows backslashes, and spaces in the account name
/// the app data directory sits under.
pub fn render_site_conf(
    hostname: &str,
    backend: &BackendTarget,
    limits: &SiteLimits,
    cert_path: &str,
    key_path: &str,
    access_log: &str,
    error_log: &str,
) -> String {
    let cert_path = quoted(cert_path);
    let key_path = quoted(key_path);
    let access_log = quoted(access_log);
    let error_log = quoted(error_log);
    let backend = backend_block(backend, limits);
    let tuning = indented(limits.apache_directives(), "    ");
    // The log format is spelled out rather than naming `combined`: that nickname is
    // defined by the stock httpd.conf, not by Apache, and a trimmed config without it
    // fails `httpd -t` on a line DevsPro wrote.
    format!(
        r#"# Managed by DevsPro — do not edit by hand, changes will be overwritten on next save.
<VirtualHost *:80>
    ServerName {hostname}
    <IfModule alias_module>
        Redirect permanent / https://{hostname}/
    </IfModule>
</VirtualHost>

<VirtualHost *:443>
    ServerName {hostname}

    SSLEngine on
    SSLCertificateFile {cert_path}
    SSLCertificateKeyFile {key_path}

    ErrorLog {error_log}
    CustomLog {access_log} "%h %l %u %t \"%r\" %>s %b \"%{{Referer}}i\" \"%{{User-Agent}}i\""

{backend}{tuning}</VirtualHost>
"#
    )
}

/// The one file DevsPro's include points at: the modules its vhosts need, a `Listen` for
/// each of 80/443 the user's own config doesn't already have, and the sites themselves.
///
/// Each `LoadModule` sits inside `<IfModule !name>`, so a module the main config already
/// loads is left alone and one it doesn't — Laragon ships `mod_proxy_fcgi` commented
/// out — is loaded here. That also keeps this file stable: it does not change depending
/// on whether a module happened to be loaded the last time it was rendered.
pub fn render_global_conf(modules: &[(String, String)], listen: &[u16], sites_glob: &str) -> String {
    let mut out = String::from("# Managed by DevsPro — do not edit by hand, changes will be overwritten on next save.\n");
    for (name, path) in modules {
        out.push_str(&format!(
            "<IfModule !{name}>\n    LoadModule {name} {}\n</IfModule>\n",
            quoted(path)
        ));
    }
    for port in listen {
        out.push_str(&format!("Listen {port}\n"));
    }
    out.push_str(&format!("IncludeOptional {}\n", quoted(sites_glob)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laravel() -> BackendTarget {
        BackendTarget::Laravel {
            public_root: "/srv/app/public".to_string(),
            fastcgi_pass: "unix:/run/php/php8.3-fpm.sock".to_string(),
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
    fn a_proxy_site_redirects_http_and_proxies_https_to_the_port() {
        let conf = render(&BackendTarget::Proxy { port: 3000 }, &SiteLimits::default());
        assert!(conf.contains("<VirtualHost *:80>"));
        assert!(conf.contains("Redirect permanent / https://app.test/"));
        assert!(conf.contains("<VirtualHost *:443>"));
        assert!(conf.contains("ProxyPass / http://127.0.0.1:3000/"));
        assert!(conf.contains(r#"SSLCertificateFile "/certs/app.test.pem""#));
        assert!(conf.contains(r#"SSLCertificateKeyFile "/certs/app.test-key.pem""#));
        assert!(conf.contains(r#"ErrorLog "/logs/app.error.log""#));
        assert!(conf.contains(r#"CustomLog "/logs/app.access.log""#));
        assert!(!conf.contains("FilesMatch"), "a proxy site runs no PHP");
    }

    #[test]
    fn a_laravel_site_serves_public_through_fastcgi() {
        let conf = render(&laravel(), &SiteLimits::default());
        assert!(conf.contains(r#"DocumentRoot "/srv/app/public""#));
        assert!(conf.contains("FallbackResource /index.php"));
        assert!(conf.contains("SetHandler \"proxy:"));
        assert!(conf.contains(r#"<LocationMatch "/\.(?!well-known)">"#));
        assert!(!conf.contains("PHP_VALUE"), "an untouched site sends no PHP settings");
    }

    #[test]
    fn limits_land_inside_the_https_vhost() {
        let limits = SiteLimits {
            max_upload_size: Some("64M".into()),
            memory_limit: Some("512M".into()),
            extra_apache: Some("Header set X-Dev 1".into()),
            ..Default::default()
        };
        let conf = render(&laravel(), &limits);
        let https = &conf[conf.find("<VirtualHost *:443>").unwrap()..];
        assert!(https.contains("LimitRequestBody 67108864"));
        assert!(https.contains("PHP_VALUE"));
        assert!(https.contains("upload_max_filesize=64M"));
        assert!(https.find("Header set X-Dev 1").unwrap() < https.rfind("</VirtualHost>").unwrap());
    }

    #[test]
    fn every_virtual_host_is_closed() {
        let conf = render(&laravel(), &SiteLimits::default());
        assert_eq!(conf.matches("<VirtualHost").count(), 2);
        assert_eq!(conf.matches("</VirtualHost>").count(), 2);
    }

    #[test]
    fn global_conf_guards_each_module_and_includes_the_sites() {
        let conf = render_global_conf(
            &[("proxy_fcgi_module".to_string(), "/mods/mod_proxy_fcgi.so".to_string())],
            &[443],
            "/data/apache/sites/*.conf",
        );
        assert!(conf.contains("<IfModule !proxy_fcgi_module>\n    LoadModule proxy_fcgi_module \"/mods/mod_proxy_fcgi.so\"\n</IfModule>"));
        assert!(conf.contains("Listen 443\n"));
        assert!(!conf.contains("Listen 80\n"));
        assert!(conf.ends_with("IncludeOptional \"/data/apache/sites/*.conf\"\n"));
    }

    #[test]
    fn a_path_with_a_space_stays_one_argument() {
        assert_eq!(
            quoted("/Users/me/Library/Application Support/x.pem"),
            r#""/Users/me/Library/Application Support/x.pem""#
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_are_forward_slashed_and_php_gets_a_real_script_path() {
        let conf = render_site_conf(
            "app.test",
            &BackendTarget::Laravel {
                public_root: r"D:\projects\app\public".to_string(),
                fastcgi_pass: "127.0.0.1:9803".to_string(),
            },
            &SiteLimits::default(),
            r"C:\Users\MD. Talha\certs\app.test.pem",
            r"C:\Users\MD. Talha\certs\app.test-key.pem",
            r"C:\Users\MD. Talha\logs\app.access.log",
            r"C:\Users\MD. Talha\logs\app.error.log",
        );
        assert!(conf.contains(r#"DocumentRoot "D:/projects/app/public""#));
        assert!(conf.contains(r#"SSLCertificateFile "C:/Users/MD. Talha/certs/app.test.pem""#));
        // Without the trailing slash Apache glues `C:/...` straight onto the port and
        // cannot parse the URL; without the SCRIPT_FILENAME fix php-cgi gets `/C:/...`.
        assert!(conf.contains(r#"SetHandler "proxy:fcgi://127.0.0.1:9803/""#));
        assert!(conf.contains(r#"SCRIPT_FILENAME "%{DOCUMENT_ROOT}%{reqenv:SCRIPT_NAME}""#));
    }

    #[cfg(unix)]
    #[test]
    fn unix_fastcgi_targets_use_the_documented_handler_forms() {
        assert_eq!(
            platform::fcgi_handler("unix:/run/php/php8.3-fpm.sock"),
            "proxy:unix:/run/php/php8.3-fpm.sock|fcgi://localhost/"
        );
        assert_eq!(platform::fcgi_handler("127.0.0.1:9000"), "proxy:fcgi://127.0.0.1:9000");
        assert_eq!(platform::script_filename_fix(), None);
    }
}
