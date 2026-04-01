use serde::Deserialize;
use std::io::BufRead;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

/// Holds the running Node.js extension-host process.
pub struct ExtHostProcess {
    inner: Mutex<Option<ExtHostState>>,
}

struct ExtHostState {
    child: Child,
    port: u16,
}

impl ExtHostProcess {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }
}

#[derive(Deserialize)]
struct PortMessage {
    port: u16,
}

/// Locate the `node` binary, preferring the one on PATH.
fn find_node() -> Result<String, String> {
    let candidates = if cfg!(target_os = "windows") {
        vec!["node.exe", "node"]
    } else {
        vec![
            "node",
            "/usr/local/bin/node",
            "/opt/homebrew/bin/node",
        ]
    };

    for c in &candidates {
        if Command::new(c)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
        {
            return Ok(c.to_string());
        }
    }

    Err("Node.js not found. Install Node.js (>=18) to use Node extensions.".into())
}

#[tauri::command]
pub async fn start_extension_host(
    app: AppHandle,
    state: State<'_, ExtHostProcess>,
) -> Result<u16, String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;

    if let Some(ref s) = *guard {
        return Ok(s.port);
    }

    let node = find_node()?;

    let server_js = {
        // Try Tauri resource path first (production)
        let resource_path = app
            .path()
            .resolve("extension-host/server.js", tauri::path::BaseDirectory::Resource)
            .ok();
        
        if let Some(ref p) = resource_path {
            if p.exists() {
                p.clone()
            } else {
                // Dev mode fallback: relative to Cargo manifest dir
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("extension-host/server.js")
            }
        } else {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("extension-host/server.js")
        }
    };

    if !server_js.exists() {
        return Err(format!(
            "extension host script not found at {}",
            server_js.display()
        ));
    }

    let mut child = Command::new(&node)
        .arg(server_js)
        .stdin(Stdio::piped()) // kept open so child detects parent death
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn extension host: {e}"))?;

    // Read the first line of stdout — it contains `{"port": <n>}`
    let stdout = child
        .stdout
        .take()
        .ok_or("failed to capture extension host stdout")?;

    let port = {
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| format!("failed to read extension host port: {e}"))?;
        let msg: PortMessage =
            serde_json::from_str(line.trim()).map_err(|e| format!("bad port message: {e}"))?;
        msg.port
    };

    // Pipe stderr → log (fire-and-forget thread)
    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines().flatten() {
                log::info!("{}", line);
            }
        });
    }

    log::info!("extension host started on port {port}");
    *guard = Some(ExtHostState { child, port });
    Ok(port)
}

#[tauri::command]
pub async fn stop_extension_host(state: State<'_, ExtHostProcess>) -> Result<(), String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    if let Some(mut s) = guard.take() {
        let _ = s.child.kill();
        let _ = s.child.wait();
        log::info!("extension host stopped");
    }
    Ok(())
}

#[tauri::command]
pub async fn extension_host_port(state: State<'_, ExtHostProcess>) -> Result<Option<u16>, String> {
    let guard = state.inner.lock().map_err(|e| e.to_string())?;
    Ok(guard.as_ref().map(|s| s.port))
}
