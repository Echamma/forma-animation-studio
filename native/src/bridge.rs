//! The local bridge Forma MCP talks to, as Electron Forma's `mcp/desktop-bridge.cjs` runs it: HTTP on 127.0.0.1 at a
//! random port with a random bearer token, both in a discovery file (the same file Electron's uses), so Electron's
//! `forma-mcp.cjs` and `Forma.exe --mcp-server` both find the native editor. Requests from browsers or without the
//! token are refused; arguments are checked against the contract; calls reach the editor one at a time, and the
//! editor answers them on its UI thread.

use crate::contract::{ToolError, fail, validate};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// Tool arguments above this are refused, as Electron's bridge does.
const MOST_BYTES: usize = 1024 * 1024;
/// How long a call may take in the editor: video exports render every frame.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// A tool call for the editor, and where its answer goes.
pub struct Call {
    pub name: String,
    pub args: Value,
    pub reply: mpsc::Sender<Result<Value, ToolError>>,
}

/// The bridge while MCP control is on; dropping it turns control off and removes its discovery file.
pub struct Bridge {
    pub calls: mpsc::Receiver<Call>,
    port: u16,
    token: String,
    file: PathBuf,
    stop: Arc<AtomicBool>,
}

/// Where the bridge says where it is: `FORMA_BRIDGE_FILE`, or Electron Forma's place for it.
pub fn discovery_file() -> PathBuf {
    if let Some(file) = std::env::var_os("FORMA_BRIDGE_FILE") {
        return PathBuf::from(file);
    }
    let home = || {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_default()
    };
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join("AppData").join("Roaming"))
    } else if cfg!(target_os = "macos") {
        home().join("Library").join("Application Support")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".config"))
    };
    base.join("forma-animation-studio").join("mcp-bridge.json")
}

impl Bridge {
    /// Turns MCP control on. `wake` is called when a call arrives (the editor repaints to answer it).
    pub fn enable(wake: impl Fn() + Send + 'static) -> Result<Bridge, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|e| format!("Couldn't open the MCP bridge ({e})."))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let token = random_token();
        let file = discovery_file();
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let temporary = file.with_extension(format!("json.{}.tmp", std::process::id()));
        let discovery =
            json!({"version": 1, "pid": std::process::id(), "port": port, "token": token});
        std::fs::write(&temporary, discovery.to_string())
            .and_then(|()| std::fs::rename(&temporary, &file))
            .map_err(|e| format!("Couldn't write {} ({e}).", file.display()))?;
        let (send, calls) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (stopped, expected) = (stop.clone(), format!("Bearer {token}"));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                // One call at a time, in the order they come: the editor runs them one by one anyway.
                if let Ok(stream) = stream {
                    serve(stream, port, &expected, &send, &wake, &stopped);
                }
            }
        });
        Ok(Bridge {
            calls,
            port,
            token,
            file,
            stop,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the listener so its thread sees the stop.
        let _ = TcpStream::connect_timeout(
            &([127, 0, 0, 1], self.port).into(),
            Duration::from_millis(200),
        );
        // Remove the discovery file only if it's still ours (another Forma may have turned MCP on since).
        let ours = std::fs::read(&self.file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .is_some_and(|d| d["pid"] == std::process::id() && d["token"] == self.token);
        if ours {
            let _ = std::fs::remove_file(&self.file);
        }
    }
}

/// 32 random bytes as hex. The randomness is std's: each `RandomState` is keyed from the operating system.
fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut digest = Sha256::new();
    for i in 0..8u64 {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(i);
        digest.update(hasher.finish().to_le_bytes());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    digest.update(now.as_nanos().to_le_bytes());
    digest.update(std::process::id().to_le_bytes());
    format!("{:x}", digest.finalize())
}

/// Compares without stopping at the first difference, so timing doesn't tell how much of a token was right.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

fn serve(
    stream: TcpStream,
    port: u16,
    expected: &str,
    calls: &mpsc::Sender<Call>,
    wake: &dyn Fn(),
    stopped: &AtomicBool,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let answer = handle(&mut reader, port, expected, calls, wake, stopped);
    let (status, body) = match answer {
        Ok(result) => (200, json!({"result": result})),
        Err(error) => {
            let status = match error.code.as_str() {
                "UNAUTHORIZED" => 401,
                "FORBIDDEN" => 403,
                _ => 400,
            };
            (
                status,
                json!({"error": {"code": error.code, "message": error.message}}),
            )
        }
    };
    let body = body.to_string();
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        403 => "Forbidden",
        _ => "Bad Request",
    };
    let mut stream = stream;
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

fn handle(
    reader: &mut BufReader<TcpStream>,
    port: u16,
    expected: &str,
    calls: &mpsc::Sender<Call>,
    wake: &dyn Fn(),
    stopped: &AtomicBool,
) -> Result<Value, ToolError> {
    let invalid = || fail("INVALID_REQUEST", "Expected a JSON tool request.");
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|_| invalid())?;
    let mut parts = line.split_whitespace();
    let (method, target) = (
        parts.next().unwrap_or(""),
        parts.next().unwrap_or("").to_owned(),
    );
    let mut headers = Vec::new();
    let mut header_bytes = 0;
    loop {
        let mut header = String::new();
        let read = reader.read_line(&mut header).map_err(|_| invalid())?;
        header_bytes += read;
        if read == 0 || header_bytes > 64 * 1024 {
            return Err(invalid());
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    let get = |name: &str| {
        headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };
    // Read the body before any refusal: closing with it unread makes Windows reset the connection, and the client
    // would never see why.
    let length: usize = get("content-length")
        .and_then(|n| n.parse().ok())
        .ok_or_else(invalid)?;
    if length > MOST_BYTES {
        return Err(fail("REQUEST_TOO_LARGE", "Tool arguments exceed 1 MB."));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).map_err(|_| invalid())?;
    // Only this computer's MCP clients: the right Host, no browser Origin, then the token.
    if get("host") != Some(format!("127.0.0.1:{port}").as_str()) || get("origin").is_some() {
        return Err(fail(
            "FORBIDDEN",
            "Only authenticated local MCP clients are accepted.",
        ));
    }
    if !same(
        get("authorization").unwrap_or("").as_bytes(),
        expected.as_bytes(),
    ) {
        return Err(fail(
            "UNAUTHORIZED",
            "Local bridge token is missing or invalid.",
        ));
    }
    let json_body = get("content-type")
        .is_some_and(|t| t.split(';').next().unwrap_or("").trim() == "application/json");
    let Some(name) = target
        .strip_prefix("/tools/")
        .filter(|_| method == "POST" && json_body)
    else {
        return Err(invalid());
    };
    let args: Value = serde_json::from_slice(&body)
        .map_err(|_| fail("INVALID_ARGUMENTS", "Expected valid JSON arguments."))?;
    validate(name, &args)?;
    if stopped.load(Ordering::SeqCst) {
        return Err(fail("DISABLED", "MCP control has been turned off."));
    }
    let (reply, answer) = mpsc::channel();
    calls
        .send(Call {
            name: name.to_owned(),
            args,
            reply,
        })
        .map_err(|_| fail("DISABLED", "MCP control has been turned off."))?;
    wake();
    match answer.recv_timeout(CALL_TIMEOUT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(fail(
            "TIMEOUT",
            "Forma took too long; inspect the scene before retrying an edit.",
        )),
        // The editor let go of the call: control was turned off, or Forma is closing.
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(fail("DISABLED", "MCP control has been turned off."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A raw request to the bridge: its status and JSON body.
    pub fn post(
        port: u16,
        token: Option<&str>,
        extra: &str,
        path: &str,
        body: &str,
    ) -> (u16, Value) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let auth = token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        write!(stream, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}{extra}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let status = response.split_whitespace().nth(1).unwrap().parse().unwrap();
        let json = response
            .split_once("\r\n\r\n")
            .map(|(_, b)| serde_json::from_str(b).unwrap())
            .unwrap();
        (status, json)
    }

    #[test]
    fn the_bridge_takes_only_its_own_clients() {
        let dir = std::env::temp_dir().join(format!("forma-bridge-{}", crate::app::new_id()));
        // SAFETY: tests touching FORMA_BRIDGE_FILE run in this one test.
        unsafe { std::env::set_var("FORMA_BRIDGE_FILE", dir.join("bridge.json")) };
        let bridge = Bridge::enable(|| {}).unwrap();
        let discovery: Value =
            serde_json::from_slice(&std::fs::read(dir.join("bridge.json")).unwrap()).unwrap();
        let token = discovery["token"].as_str().unwrap().to_owned();
        assert!(
            token.len() == 64
                && token.chars().all(|c| c.is_ascii_hexdigit())
                && discovery["port"] == bridge.port()
        );
        // The editor's side: answer calls as they come.
        let port = bridge.port();
        let answering = std::thread::spawn(move || {
            let call = bridge.calls.recv().unwrap();
            let _ = call.reply.send(Ok(json!({"tool": call.name})));
            bridge
        });
        assert_eq!(post(port, None, "", "/tools/forma_get_scene", "{}").0, 401);
        assert_eq!(
            post(
                port,
                Some("0".repeat(64).as_str()),
                "",
                "/tools/forma_get_scene",
                "{}"
            )
            .0,
            401
        );
        assert_eq!(
            post(
                port,
                Some(&token),
                "Origin: http://evil.example\r\n",
                "/tools/forma_get_scene",
                "{}"
            )
            .0,
            403
        );
        let (status, body) = post(
            port,
            Some(&token),
            "",
            "/tools/forma_add_shape",
            r#"{"type":"blob"}"#,
        );
        assert_eq!(
            (status, body["error"]["code"].as_str()),
            (400, Some("INVALID_ARGUMENTS"))
        );
        let (status, body) = post(port, Some(&token), "", "/tools/forma_get_scene", "{}");
        assert_eq!(
            (status, body["result"]["tool"].as_str()),
            (200, Some("forma_get_scene"))
        );
        // Turning control off removes the discovery file.
        drop(answering.join().unwrap());
        assert!(!dir.join("bridge.json").exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
