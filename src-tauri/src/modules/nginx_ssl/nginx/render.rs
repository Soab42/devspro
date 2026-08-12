//! Vhost generation. Plain string templating, not a template engine — the structure
//! is fixed and entirely known ahead of time, and the only variable pieces
//! (hostname/port/paths) are validated before they ever reach here (see
//! `sites::valid_hostname`, `laravel::is_laravel_project`), so a template engine
//! would add a dependency without buying anything.

pub enum BackendTarget {
    Proxy { port: u16 },
    /// Laravel is served directly by nginx + PHP-FPM through its `public/`
    /// front controller — no `artisan serve` process, no port to manage.
    Laravel { public_root: String, php_fpm_socket: String },
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
            php_fpm_socket,
        } => format!(
            r#"    root {public_root};
    index index.php index.html;

    location / {{
        try_files $uri $uri/ /index.php?$query_string;
    }}

    location ~ \.php$ {{
        include fastcgi_params;
        fastcgi_pass unix:{php_fpm_socket};
        fastcgi_param SCRIPT_FILENAME $realpath_root$fastcgi_script_name;
    }}

    location ~ /\.(?!well-known).* {{
        deny all;
    }}"#
        ),
    }
}

pub fn render_site_conf(
    hostname: &str,
    backend: &BackendTarget,
    cert_path: &str,
    key_path: &str,
    access_log: &str,
    error_log: &str,
) -> String {
    let location = location_block(backend);
    format!(
        r#"# Managed by DevSuite — do not edit by hand, changes will be overwritten on next save.
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
