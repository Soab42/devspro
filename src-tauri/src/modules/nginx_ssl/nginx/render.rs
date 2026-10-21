//! Vhost generation. Plain string templating, not a template engine — the structure
//! is fixed and entirely known ahead of time, and the only variable pieces
//! (hostname/port/paths) are validated before they ever reach here (see
//! `sites::valid_hostname`, `laravel::is_laravel_project`), so a template engine
//! would add a dependency without buying anything.

pub enum BackendTarget {
    Proxy { port: u16 },
    /// Laravel is served directly by nginx + PHP through its `public/` front controller —
    /// no `artisan serve` process, no port to manage.
    Laravel {
        public_root: String,
        /// A complete `fastcgi_pass` value, already in the form nginx wants: `unix:<path>`
        /// for a PHP-FPM socket, `host:port` for a `php-cgi.exe` listener. Composed by
        /// `php_fpm::platform` rather than here, so this template doesn't need to know
        /// which kind of backend it is pointing at.
        fastcgi_pass: String,
    },
}

fn location_block(backend: &BackendTarget) -> String {
    match backend {
        BackendTarget::Proxy { port } => format!(
            r#"    location / {{
        proxy_pass http://127.0.0.1:{port};
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }}"#
        ),
        BackendTarget::Laravel {
            public_root,
            fastcgi_pass,
        } => {
            let public_root = super::to_config_path(public_root);
            format!(
                r#"    root {public_root};
    index index.php index.html;

    location / {{
        try_files $uri $uri/ /index.php?$query_string;
    }}

    location ~ \.php$ {{
        include fastcgi_params;
        fastcgi_pass {fastcgi_pass};
        fastcgi_param SCRIPT_FILENAME $realpath_root$fastcgi_script_name;
    }}

    location ~ /\.(?!well-known).* {{
        deny all;
    }}"#
            )
        }
    }
}

/// Every path written into the result goes through `to_config_path` first. This is the one
/// place paths enter an nginx config, so normalising here is what makes it impossible to
/// miss one: on Windows a backslash is an escape character to nginx's parser, so
/// `C:\Users\me\certs\app.pem` arrives with `\U`, `\m` and `\c` consumed and the
/// certificate unfindable. A no-op on Unix, where the separator is already what nginx wants.
pub fn render_site_conf(
    hostname: &str,
    backend: &BackendTarget,
    cert_path: &str,
    key_path: &str,
    access_log: &str,
    error_log: &str,
) -> String {
    let cert_path = super::to_config_path(cert_path);
    let key_path = super::to_config_path(key_path);
    let access_log = super::to_config_path(access_log);
    let error_log = super::to_config_path(error_log);
    let location = location_block(backend);
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

{location}
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(backend: &BackendTarget) -> String {
        render_site_conf(
            "app.test",
            backend,
            "/certs/app.test.pem",
            "/certs/app.test-key.pem",
            "/logs/app.access.log",
            "/logs/app.error.log",
        )
    }

    #[test]
    fn a_proxy_site_redirects_http_and_proxies_https_to_the_port() {
        let conf = render(&BackendTarget::Proxy { port: 3000 });
        assert!(conf.contains("return 301 https://$host$request_uri;"));
        assert!(conf.contains("proxy_pass http://127.0.0.1:3000;"));
        assert!(conf.contains("server_name app.test;"));
        assert!(conf.contains("ssl_certificate     /certs/app.test.pem;"));
    }

    /// The backend value is passed through verbatim, whichever form it takes — that is the
    /// whole reason `fastcgi_pass` is composed by the platform layer instead of here.
    #[test]
    fn a_laravel_site_uses_the_fastcgi_pass_it_was_given() {
        let unix_socket = render(&BackendTarget::Laravel {
            public_root: "/srv/app/public".to_string(),
            fastcgi_pass: "unix:/run/php/php8.3-fpm.sock".to_string(),
        });
        assert!(unix_socket.contains("fastcgi_pass unix:/run/php/php8.3-fpm.sock;"));
        assert!(unix_socket.contains("root /srv/app/public;"));

        let tcp_listener = render(&BackendTarget::Laravel {
            public_root: "/srv/app/public".to_string(),
            fastcgi_pass: "127.0.0.1:9803".to_string(),
        });
        assert!(tcp_listener.contains("fastcgi_pass 127.0.0.1:9803;"));
    }

    #[test]
    fn dotfiles_are_denied_except_well_known() {
        let conf = render(&BackendTarget::Laravel {
            public_root: "/srv/app/public".to_string(),
            fastcgi_pass: "unix:/run/php/php8.3-fpm.sock".to_string(),
        });
        assert!(conf.contains(r"location ~ /\.(?!well-known).*"));
    }

    /// On Windows every one of these arrives with backslashes, and nginx reads a backslash
    /// as an escape — so an unconverted path silently becomes a different, missing one.
    #[cfg(windows)]
    #[test]
    fn windows_paths_are_forward_slashed_everywhere_they_appear() {
        let conf = render_site_conf(
            "app.test",
            &BackendTarget::Laravel {
                public_root: r"D:\projects\app\public".to_string(),
                fastcgi_pass: "127.0.0.1:9803".to_string(),
            },
            r"C:\Users\me\certs\app.test.pem",
            r"C:\Users\me\certs\app.test-key.pem",
            r"C:\Users\me\logs\app.access.log",
            r"C:\Users\me\logs\app.error.log",
        );
        assert!(conf.contains("root D:/projects/app/public;"));
        assert!(conf.contains("ssl_certificate     C:/Users/me/certs/app.test.pem;"));
        assert!(conf.contains("ssl_certificate_key C:/Users/me/certs/app.test-key.pem;"));
        assert!(conf.contains("access_log C:/Users/me/logs/app.access.log;"));
        assert!(conf.contains("error_log  C:/Users/me/logs/app.error.log;"));

        // Checked directive by directive rather than by scanning the whole file for a
        // backslash: the template legitimately contains two, in the `location ~ \.php$`
        // and `/\.(?!well-known)` regexes, and those must stay exactly as they are.
        let path_directives: Vec<&str> = conf
            .lines()
            .filter(|line| {
                let directive = line.trim_start();
                ["root ", "ssl_certificate", "access_log", "error_log"]
                    .iter()
                    .any(|prefix| directive.starts_with(prefix))
            })
            .collect();
        assert_eq!(path_directives.len(), 5, "found {path_directives:?}");
        for directive in path_directives {
            assert!(!directive.contains('\\'), "unconverted path: {directive}");
        }
    }
}
