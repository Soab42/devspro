//! Who is on a port: nobody, a Mailpit (anyone's), or some other program.
//! All of it is a local TCP connect or an OS socket listing — nothing here
//! spawns `docker`, so the Mail Catcher page can ask on every open.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(200);

/// Something accepts connections on `port`. A connect rather than a test bind:
/// on Windows, binding 127.0.0.1 succeeds even while Docker Desktop's proxy holds
/// 0.0.0.0 on the same port, so a bind would call a taken port free.
pub fn listening(port: u16) -> bool {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).is_ok()
}

pub fn wait_until_free(ports: &[u16], timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while ports.iter().any(|&p| listening(p)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Whether the web server on `port` is a Mailpit — asked through Mailpit's own
/// `/api/v1/info`, so a different web app on 8025 isn't mistaken for one.
/// Plain HTTP/1.0 over a socket: one short local request doesn't need a client
/// library, and this runs on a blocking thread.
pub fn is_mailpit(port: u16) -> bool {
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(800)));
    if stream
        .write_all(b"GET /api/v1/info HTTP/1.0\r\nHost: localhost\r\nAccept: application/json\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut response = Vec::new();
    let _ = stream.take(64 * 1024).read_to_end(&mut response);
    looks_like_mailpit_info(&String::from_utf8_lossy(&response))
}

fn looks_like_mailpit_info(response: &str) -> bool {
    let Some((head, body)) = response.split_once("\r\n\r\n") else {
        return false;
    };
    let ok = head.lines().next().is_some_and(|status| status.split_whitespace().nth(1) == Some("200"));
    let Ok(json) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    ok && json.get("Version").is_some_and(|v| v.is_string()) && json.get("Messages").is_some_and(|v| v.is_number())
}

/// "pid 4120 (node.exe)" for whatever holds `port`, for a message that names the
/// culprit instead of only saying the port is taken.
pub fn owner_of(port: u16) -> String {
    let entries = crate::modules::processes::listening_ports().unwrap_or_default();
    let mut owners: Vec<String> = Vec::new();
    for entry in entries.iter().filter(|e| e.port == port) {
        let label = match (&entry.process_name, entry.pid) {
            (Some(name), Some(pid)) => format!("{name} (pid {pid})"),
            (None, Some(pid)) => format!("pid {pid}"),
            (Some(name), None) => name.clone(),
            (None, None) => continue,
        };
        if !owners.contains(&label) {
            owners.push(label);
        }
    }
    if owners.is_empty() {
        "another program".to_string()
    } else {
        owners.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{\"Version\":\"v1.32.0\",\"LatestVersion\":\"\",\"Database\":\"/data/mailpit.db\",\"DatabaseSize\":4096,\"Messages\":3,\"Unread\":1}";

    #[test]
    fn recognises_mailpits_info_response() {
        assert!(looks_like_mailpit_info(INFO));
    }

    #[test]
    fn another_web_app_is_not_mailpit() {
        assert!(!looks_like_mailpit_info("HTTP/1.1 200 OK\r\n\r\n<html>hello</html>"));
        assert!(!looks_like_mailpit_info("HTTP/1.1 200 OK\r\n\r\n{\"version\":\"1\"}"));
        assert!(!looks_like_mailpit_info("HTTP/1.1 404 Not Found\r\n\r\n{\"Version\":\"x\",\"Messages\":0}"));
        assert!(!looks_like_mailpit_info(""));
    }

    #[test]
    fn a_closed_port_is_not_listening() {
        // Bind and drop to get a port nothing is on.
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        assert!(!listening(port));
        assert!(!is_mailpit(port));
    }

    #[test]
    fn an_open_port_is_listening() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        assert!(listening(listener.local_addr().unwrap().port()));
    }
}
