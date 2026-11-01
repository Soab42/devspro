//! Turns a web server's "could not bind" error into what the user needs to know: which
//! program already holds the port, and what to do about it.
//!
//! nginx tests its config by binding the ports for real, so `nginx -t` fails with
//! `bind() to 0.0.0.0:80 failed (10013: An attempt was made to access a socket in a way
//! forbidden ...)` whenever anything else — very often Laragon's Apache — already
//! listens there. That message names neither the culprit nor the fix. Apache's own
//! variant, `could not bind to address 0.0.0.0:80`, surfaces when it restarts.

use super::web_server::WebServer;
use crate::modules::processes::PortEntry;

/// Every port a bind failure in `message` names, in order, once each.
pub fn ports_in_bind_error(message: &str) -> Vec<u16> {
    const MARKERS: &[&str] = &["bind() to ", "could not bind to address "];
    let mut ports = Vec::new();
    for marker in MARKERS {
        for (start, _) in message.match_indices(marker) {
            let rest = &message[start + marker.len()..];
            let address = rest.split_whitespace().next().unwrap_or_default();
            let port = address
                .trim_end_matches(|c: char| !c.is_ascii_digit())
                .rsplit(':')
                .next()
                .and_then(|p| p.parse::<u16>().ok());
            if let Some(port) = port {
                if !ports.contains(&port) {
                    ports.push(port);
                }
            }
        }
    }
    ports
}

/// `message` unchanged unless it is a bind failure; otherwise an explanation first and
/// the server's own words after it.
pub fn explain(message: &str, server: WebServer) -> String {
    let ports = ports_in_bind_error(message);
    if ports.is_empty() {
        return message.to_string();
    }
    let listeners = crate::modules::processes::listening_ports();
    let explanation = describe(&ports, listeners.as_deref().ok(), server);
    format!("{explanation} ({} said: {})", server.label(), message.trim())
}

fn describe(ports: &[u16], listeners: Option<&[PortEntry]>, server: WebServer) -> String {
    let mut sentences = Vec::new();
    for &port in ports {
        // One process usually listens twice — 0.0.0.0 and [::] — and is named once.
        let mut owners: Vec<&PortEntry> = Vec::new();
        for entry in listeners.unwrap_or_default().iter().filter(|e| e.port == port) {
            if !owners.iter().any(|o| o.pid == entry.pid && o.process_name == entry.process_name) {
                owners.push(entry);
            }
        }

        let named: Vec<String> = owners
            .iter()
            .map(|entry| match (&entry.process_name, entry.pid) {
                (Some(name), Some(pid)) => format!("{name} (PID {pid})"),
                (Some(name), None) => name.clone(),
                (None, Some(pid)) => format!("PID {pid}"),
                (None, None) => "another program".to_string(),
            })
            .collect();

        let sentence = if !named.is_empty() {
            let mut sentence = format!(
                "Port {port} is already in use by {}, so {} cannot listen on it. Only one web server can own a port — stop that program first, then try again.",
                named.join(" and "),
                server.label()
            );
            if owners.iter().any(|entry| is_other_server(entry, server)) {
                sentence.push_str(&format!(
                    " It is {} — if you want to keep it, switch DevsPro to {} on the {} tab instead, so your sites are served by the server that is already running. In Laragon, the server is chosen under Menu → Preferences → Services & Ports.",
                    server.other().label(),
                    server.other().label(),
                    server.other().label()
                ));
            }
            sentence
        } else if listeners.is_some() {
            if cfg!(windows) {
                format!(
                    "Windows refused access to port {port}, but no program is listening on it — the port is most likely reserved by Windows (check `netsh interface ipv4 show excludedportrange protocol=tcp`) or blocked by security software."
                )
            } else {
                format!(
                    "Port {port} is already in use, but DevsPro can't see by which program — it probably runs as another user. `sudo ss -tlnp 'sport = :{port}'` shows it; stop it, then try again."
                )
            }
        } else {
            format!(
                "Port {port} is already in use by another program. `sudo lsof -iTCP:{port} -sTCP:LISTEN` shows which; stop it, then try again."
            )
        };
        sentences.push(sentence);
    }
    sentences.join(" ")
}

/// Whether the port's owner is the web server DevsPro is *not* set to use — the common
/// case of Laragon's Apache holding 80/443 while DevsPro writes nginx sites.
fn is_other_server(entry: &PortEntry, server: WebServer) -> bool {
    let name = entry.process_name.as_deref().unwrap_or_default().to_ascii_lowercase();
    let name = name.trim_end_matches(".exe");
    match server.other() {
        WebServer::Apache => name == "httpd" || name == "apache2",
        WebServer::Nginx => name == "nginx",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listener(port: u16, name: &str, pid: u32) -> PortEntry {
        PortEntry {
            port,
            address: "0.0.0.0".to_string(),
            pid: Some(pid),
            process_name: Some(name.to_string()),
            command: None,
        }
    }

    #[test]
    fn finds_the_port_in_nginx_and_apache_bind_errors() {
        // The exact message from the Laragon report.
        assert_eq!(
            ports_in_bind_error(
                "nginx: the configuration file C:/laragon/bin/nginx/nginx-1.22.0/conf/nginx.conf syntax is ok nginx: [emerg] bind() to 0.0.0.0:80 failed (10013: An attempt was made to access a socket in a way forbidden by its access permissions)"
            ),
            vec![80]
        );
        assert_eq!(
            ports_in_bind_error("nginx: [emerg] bind() to [::]:443 failed (98: Address already in use)"),
            vec![443]
        );
        assert_eq!(
            ports_in_bind_error(
                "(OS 10048)Only one usage of each socket address is normally permitted.  : AH00072: make_sock: could not bind to address 0.0.0.0:80"
            ),
            vec![80]
        );
        assert!(ports_in_bind_error("nginx: [emerg] unknown directive \"foo\"").is_empty());
    }

    #[test]
    fn names_the_owner_and_suggests_switching_when_it_is_the_other_server() {
        let listeners = vec![listener(80, "httpd.exe", 17940)];
        let text = describe(&[80], Some(&listeners), WebServer::Nginx);
        assert!(text.contains("Port 80 is already in use by httpd.exe (PID 17940)"));
        assert!(text.contains("switch DevsPro to Apache on the Apache tab"));
    }

    #[test]
    fn a_process_listening_on_ipv4_and_ipv6_is_named_once() {
        let mut v6 = listener(80, "httpd.exe", 17940);
        v6.address = "::".to_string();
        let listeners = vec![listener(80, "httpd.exe", 17940), listener(443, "httpd.exe", 17940), v6];
        let text = describe(&[80], Some(&listeners), WebServer::Nginx);
        assert_eq!(text.matches("httpd.exe").count(), 1, "{text}");
    }

    #[test]
    fn an_unrelated_owner_gets_no_switch_suggestion() {
        let listeners = vec![listener(80, "skype.exe", 42)];
        let text = describe(&[80], Some(&listeners), WebServer::Nginx);
        assert!(text.contains("skype.exe (PID 42)"));
        assert!(!text.contains("switch DevsPro"));
    }

    #[test]
    fn says_so_when_the_owner_cannot_be_listed() {
        let text = describe(&[443], None, WebServer::Apache);
        assert!(text.contains("Port 443 is already in use by another program"));
    }

    #[test]
    fn leaves_other_failures_untouched() {
        let message = "nginx: [emerg] unknown directive \"foo\"";
        assert_eq!(explain(message, WebServer::Nginx), message);
    }
}
