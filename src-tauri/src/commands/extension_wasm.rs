use crate::commands::extension_platform::ExtensionManifest;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use tauri::State;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

// ---------------------------------------------------------------------------
// tsserver process manager — spawned once per WasmHostState, shared across
// all tsserver_request calls from the TypeScript WASM extension.
// ---------------------------------------------------------------------------

struct TsServerProcess {
    child: Child,
    stdin: ChildStdin,
    reader: std::io::BufReader<std::process::ChildStdout>,
    seq: u32,
}

impl TsServerProcess {
    fn find_tsserver(workspace_folders: &[String]) -> Option<String> {
        for folder in workspace_folders {
            let p = std::path::Path::new(folder).join("node_modules/typescript/bin/tsserver");
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
        }

        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let candidates = [
            "node_modules/typescript/bin/tsserver",
            "../node_modules/typescript/bin/tsserver",
            "../../node_modules/typescript/bin/tsserver",
        ];
        for rel in &candidates {
            let p = cwd.join(rel);
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
        }

        let global_paths = [
            "/usr/local/lib/node_modules/typescript/bin/tsserver",
            "/opt/homebrew/lib/node_modules/typescript/bin/tsserver",
        ];
        for p in &global_paths {
            if std::path::Path::new(p).exists() {
                return Some(p.to_string());
            }
        }

        if let Ok(out) = std::process::Command::new("which").arg("tsserver").output() {
            if out.status.success() {
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !path.is_empty() {
                    return Some(path);
                }
            }
        }

        None
    }

    fn spawn(workspace_folders: &[String]) -> Option<Self> {
        let tsserver_path = Self::find_tsserver(workspace_folders)?;

        let node_bin = std::env::var("SIDEX_NODE_BINARY").unwrap_or_else(|_| "node".to_string());

        let mut cmd = Command::new(&node_bin);
        cmd.arg(&tsserver_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = cmd
            .spawn()
            .map_err(|e| {
                log::error!("[tsserver] spawn failed: {e}");
                e
            })
            .ok()?;
        let stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        let reader = std::io::BufReader::with_capacity(256 * 1024, stdout);

        log::info!(
            "[tsserver] spawned (pid={}) via {node_bin} {tsserver_path}",
            child.id()
        );
        Some(Self {
            child,
            stdin,
            reader,
            seq: 0,
        })
    }

    /// Send a one-way message (no response expected, e.g. `open`).
    /// tsserver reads plain newline-terminated JSON on stdin.
    fn send_notification(&mut self, command: &str, arguments: &str) {
        self.seq += 1;
        let seq = self.seq;
        let msg = format!(
            r#"{{"seq":{seq},"type":"request","command":"{command}","arguments":{arguments}}}"#
        );
        let _ = self.stdin.write_all(msg.as_bytes());
        let _ = self.stdin.write_all(b"\n");
        let _ = self.stdin.flush();
        log::debug!("[tsserver] sent notification {command} seq={seq}");
    }

    /// Send a request and synchronously read the matching response.
    /// tsserver reads newline-terminated JSON on stdin, outputs Content-Length framed JSON on stdout.
    fn request_sync(&mut self, command: &str, arguments: &str) -> Option<String> {
        self.seq += 1;
        let seq = self.seq;
        let msg = format!(
            r#"{{"seq":{seq},"type":"request","command":"{command}","arguments":{arguments}}}"#
        );
        self.stdin.write_all(msg.as_bytes()).ok()?;
        self.stdin.write_all(b"\n").ok()?;
        self.stdin.flush().ok()?;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);

        loop {
            if std::time::Instant::now() > deadline {
                log::warn!("[tsserver] TIMEOUT {command} seq={seq}");
                return None;
            }

            let mut content_length: usize = 0;
            loop {
                let mut header_line = String::new();
                match self.reader.read_line(&mut header_line) {
                    Ok(0) => {
                        log::warn!("[tsserver] EOF reading headers for {command}");
                        return None;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("[tsserver] header read error: {e}");
                        return None;
                    }
                }
                let line = header_line.trim_end_matches(['\r', '\n', ' ']);
                if line.is_empty() {
                    break;
                }
                if let Some(rest) = line.strip_prefix("Content-Length: ") {
                    content_length = rest.trim().parse().unwrap_or(0);
                }
            }

            if content_length == 0 {
                continue;
            }

            let mut body = vec![0u8; content_length];
            use std::io::Read;
            if self.reader.read_exact(&mut body).is_err() {
                log::warn!("[tsserver] failed to read {content_length} byte body");
                return None;
            }

            let body_str = String::from_utf8_lossy(&body).to_string();

            if body_str.contains(&format!(r#""request_seq":{seq}"#))
                || body_str.contains(&format!(r#""request_seq": {seq}"#))
            {
                return Some(body_str);
            }
            log::debug!("[tsserver] skip (not seq {seq})");
        }
    }
}

impl Drop for TsServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

// ---------------------------------------------------------------------------
// Generic LSP client — speaks JSON-RPC over stdin/stdout with Content-Length
// framing. Extensions invoke it via host::execute_command("__sidex.lsp", ...).
// ---------------------------------------------------------------------------

struct LspServerProcess {
    child: Child,
    stdin: ChildStdin,
    reader: std::io::BufReader<std::process::ChildStdout>,
    req_id: u64,
    server_name: String,
    initialized: bool,
}

impl LspServerProcess {
    fn spawn(name: &str, cmd: &str, args: &[&str], root_uri: &str) -> Option<Self> {
        let mut command = Command::new(cmd);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let mut child = command
            .spawn()
            .map_err(|e| {
                log::error!("[lsp:{name}] spawn failed: {e}");
                e
            })
            .ok()?;

        let stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;
        let reader = std::io::BufReader::with_capacity(256 * 1024, stdout);

        let mut proc = Self {
            child,
            stdin,
            reader,
            req_id: 0,
            server_name: name.to_string(),
            initialized: false,
        };

        let init_params = format!(
            r#"{{"processId":{},"rootUri":"{}","capabilities":{{"textDocument":{{"completion":{{"completionItem":{{"snippetSupport":true}}}},"hover":{{"contentFormat":["markdown","plaintext"]}},"signatureHelp":{{"signatureInformation":{{"parameterInformation":{{"labelOffsetSupport":true}}}}}}}},"workspace":{{"workspaceFolders":true}}}}}}"#,
            std::process::id(),
            root_uri,
        );

        let resp = proc.request_sync("initialize", &init_params)?;
        log::info!("[lsp:{name}] initialized: {}b response", resp.len());

        proc.send_notification("initialized", "{}");
        proc.initialized = true;

        Some(proc)
    }

    fn send_notification(&mut self, method: &str, params: &str) {
        let msg = format!(r#"{{"jsonrpc":"2.0","method":"{method}","params":{params}}}"#);
        let frame = format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
        let _ = self.stdin.write_all(frame.as_bytes());
        let _ = self.stdin.flush();
    }

    fn request_sync(&mut self, method: &str, params: &str) -> Option<String> {
        self.req_id += 1;
        let id = self.req_id;
        let msg = format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#);
        let frame = format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
        self.stdin.write_all(frame.as_bytes()).ok()?;
        self.stdin.flush().ok()?;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let name = &self.server_name;

        loop {
            if std::time::Instant::now() > deadline {
                log::warn!("[lsp:{name}] timeout {method} id={id}");
                return None;
            }

            let mut content_length: usize = 0;
            loop {
                let mut header = String::new();
                match self.reader.read_line(&mut header) {
                    Ok(0) => return None,
                    Ok(_) => {}
                    Err(_) => return None,
                }
                let line = header.trim();
                if line.is_empty() {
                    break;
                }
                if let Some(rest) = line.strip_prefix("Content-Length: ") {
                    content_length = rest.trim().parse().unwrap_or(0);
                }
            }
            if content_length == 0 {
                continue;
            }

            let mut body = vec![0u8; content_length];
            use std::io::Read;
            if self.reader.read_exact(&mut body).is_err() {
                return None;
            }
            let body_str = String::from_utf8_lossy(&body).to_string();

            if body_str.contains(&format!(r#""id":{id}"#))
                || body_str.contains(&format!(r#""id": {id}"#))
            {
                return Some(body_str);
            }
        }
    }

    fn open_file(&mut self, uri: &str, language_id: &str, text: &str) {
        let escaped = text
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");
        let params = format!(
            r#"{{"textDocument":{{"uri":"{}","languageId":"{}","version":1,"text":"{}"}}}}"#,
            uri.replace('"', "\\\""),
            language_id,
            escaped,
        );
        self.send_notification("textDocument/didOpen", &params);
    }
}

impl Drop for LspServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        log::info!("[lsp:{}] killed", self.server_name);
    }
}

/// Find a binary on PATH or common install locations
fn find_binary(name: &str, extra_paths: &[&str]) -> Option<String> {
    if let Ok(out) = std::process::Command::new("which").arg(name).output() {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    for p in extra_paths {
        if std::path::Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    None
}

#[allow(unsafe_code, clippy::all, unused)]
mod wit_bindings {
    wasmtime::component::bindgen!({
        world: "sidex-extension",
        path: "wit/world.wit",
    });
}

use wit_bindings::sidex::extension::common_types as wit_types;
use wit_bindings::SidexExtension;

// ---------------------------------------------------------------------------
// Host state — the data accessible to WASM extensions via host imports
// ---------------------------------------------------------------------------

struct WasmHostState {
    table: ResourceTable,
    wasi_ctx: WasiCtx,
    documents: HashMap<String, DocumentData>,
    workspace_folders: Vec<String>,
    configuration: HashMap<String, HashMap<String, String>>,
    diagnostics: HashMap<String, Vec<wit_types::Diagnostic>>,
    status_bar_items: HashMap<String, wit_types::StatusBarItem>,
    log_buffer: Vec<String>,
    tsserver: Option<TsServerProcess>,
    tsserver_open_files: std::collections::HashSet<String>,
    lsp_servers: HashMap<String, LspServerProcess>,
    lsp_open_files: HashMap<String, std::collections::HashSet<String>>,
}

struct DocumentData {
    text: String,
    language_id: String,
}

impl WasiView for WasmHostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.table,
        }
    }
}

impl WasmHostState {
    fn new() -> Self {
        let wasi_ctx = WasiCtxBuilder::new()
            .inherit_stdout()
            .inherit_stderr()
            .build();
        Self {
            table: ResourceTable::new(),
            wasi_ctx,
            documents: HashMap::new(),
            workspace_folders: Vec::new(),
            configuration: HashMap::new(),
            diagnostics: HashMap::new(),
            status_bar_items: HashMap::new(),
            log_buffer: Vec::new(),
            tsserver: None,
            tsserver_open_files: std::collections::HashSet::new(),
            lsp_servers: HashMap::new(),
            lsp_open_files: HashMap::new(),
        }
    }
}

impl wit_bindings::sidex::extension::host_api::Host for WasmHostState {
    fn log_info(&mut self, message: String) {
        log::info!("[wasm-ext] {message}");
        self.log_buffer.push(format!("[info] {message}"));
    }

    fn log_warn(&mut self, message: String) {
        log::warn!("[wasm-ext] {message}");
        self.log_buffer.push(format!("[warn] {message}"));
    }

    fn log_error(&mut self, message: String) {
        log::error!("[wasm-ext] {message}");
        self.log_buffer.push(format!("[error] {message}"));
    }

    fn show_info_message(&mut self, message: String) {
        log::info!("[wasm-ext][notification] {message}");
    }

    fn show_warn_message(&mut self, message: String) {
        log::warn!("[wasm-ext][notification] {message}");
    }

    fn show_error_message(&mut self, message: String) {
        log::error!("[wasm-ext][notification] {message}");
    }

    fn output_channel_append(&mut self, channel: String, text: String) {
        log::info!("[wasm-ext][{channel}] {text}");
    }

    fn publish_diagnostics(&mut self, uri: String, diagnostics: Vec<wit_types::Diagnostic>) {
        self.diagnostics.insert(uri, diagnostics);
    }

    fn clear_diagnostics(&mut self, uri: String) {
        self.diagnostics.remove(&uri);
    }

    fn get_workspace_folders(&mut self) -> Vec<String> {
        self.workspace_folders.clone()
    }

    fn get_configuration(&mut self, section: String, key: String) -> Option<String> {
        self.configuration
            .get(&section)
            .and_then(|s| s.get(&key))
            .cloned()
    }

    fn find_files(&mut self, pattern: String, max_results: u32) -> Vec<String> {
        let mut results = Vec::new();
        // Extract file extension from glob pattern like "**/*.css"
        let ext_match = if let Some(star_dot) = pattern.rfind("*.") {
            Some(pattern[star_dot + 1..].to_string())
        } else {
            None
        };

        for folder in &self.workspace_folders {
            let walker = walkdir::WalkDir::new(folder)
                .max_depth(10)
                .into_iter()
                .flatten();
            for entry in walker {
                if results.len() >= max_results as usize {
                    break;
                }
                let path = entry.path();
                if path.is_file() {
                    let name = path.to_string_lossy();
                    let matched = if pattern == "**/*" {
                        true
                    } else if let Some(ref ext) = ext_match {
                        name.ends_with(ext)
                    } else {
                        name.contains(&pattern)
                    };
                    if matched {
                        results.push(name.to_string());
                    }
                }
            }
        }
        results
    }

    fn read_file(&mut self, uri: String) -> Result<String, String> {
        let path = uri.strip_prefix("file://").unwrap_or(&uri);
        std::fs::read_to_string(path).map_err(|e| {
            log::warn!("[wasm-host] read_file failed for {path}: {e}");
            e.to_string()
        })
    }

    fn read_file_bytes(&mut self, uri: String) -> Result<Vec<u8>, String> {
        let path = uri.strip_prefix("file://").unwrap_or(&uri);
        std::fs::read(path).map_err(|e| e.to_string())
    }

    fn write_file(&mut self, uri: String, content: String) -> Result<(), String> {
        let path = uri.strip_prefix("file://").unwrap_or(&uri);
        std::fs::write(path, content).map_err(|e| e.to_string())
    }

    fn stat_file(&mut self, uri: String) -> Result<wit_types::FileStat, String> {
        let path = uri.strip_prefix("file://").unwrap_or(&uri);
        let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
        use std::time::UNIX_EPOCH;
        let ctime = meta
            .created()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let file_type = if meta.is_dir() {
            2
        } else if meta.is_symlink() {
            64
        } else {
            1
        };
        Ok(wit_types::FileStat {
            file_type,
            size: meta.len(),
            ctime,
            mtime,
        })
    }

    fn list_dir(&mut self, uri: String) -> Result<Vec<String>, String> {
        let path = uri.strip_prefix("file://").unwrap_or(&uri);
        let entries = std::fs::read_dir(path).map_err(|e| e.to_string())?;
        Ok(entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect())
    }

    fn get_document_text(&mut self, uri: String) -> Option<String> {
        self.documents.get(&uri).map(|d| d.text.clone())
    }

    fn get_document_language(&mut self, uri: String) -> Option<String> {
        self.documents.get(&uri).map(|d| d.language_id.clone())
    }

    fn register_command(&mut self, _id: String) {}

    fn execute_command(&mut self, id: String, args: String) -> Result<String, String> {
        if id == "__sidex.tsserver" {
            return self.execute_tsserver_command(&args);
        }
        if id == "__sidex.lsp" {
            return self.execute_lsp_command(&args);
        }
        Err(format!("command not implemented: {id}"))
    }

    fn apply_workspace_edit(&mut self, _edit: wit_types::WorkspaceEdit) -> Result<(), String> {
        Ok(())
    }

    fn show_text_document(&mut self, _uri: String) {}

    fn set_status_bar_item(&mut self, item: wit_types::StatusBarItem) {
        self.status_bar_items.insert(item.id.clone(), item);
    }

    fn remove_status_bar_item(&mut self, id: String) {
        self.status_bar_items.remove(&id);
    }

    fn watch_files(&mut self, _pattern: String) -> Result<u64, String> {
        Ok(0)
    }

    fn unwatch_files(&mut self, _watch_id: u64) {}
}

impl wit_bindings::sidex::extension::common_types::Host for WasmHostState {}

// ---------------------------------------------------------------------------
// tsserver dispatch — called from execute_command("__sidex.tsserver", ...)
// ---------------------------------------------------------------------------

impl WasmHostState {
    fn tsserver_mut(&mut self) -> Option<&mut TsServerProcess> {
        if self.tsserver.is_none() {
            self.tsserver = TsServerProcess::spawn(&self.workspace_folders);
            if self.tsserver.is_none() {
                log::warn!("[tsserver] failed to spawn — TypeScript features unavailable");
            }
        }
        self.tsserver.as_mut()
    }

    fn ensure_file_open(&mut self, file: &str) {
        if self.tsserver_open_files.contains(file) {
            return;
        }
        let uri = if file.starts_with('/') {
            format!("file://{file}")
        } else {
            file.to_string()
        };
        let (content, script_kind) = if let Some(doc) = self.documents.get(&uri) {
            let kind = if doc.language_id.contains("react") {
                "4"
            } else {
                "3"
            };
            (doc.text.clone(), kind)
        } else {
            let disk_content = std::fs::read_to_string(file).unwrap_or_default();
            let kind = if file.ends_with(".tsx") || file.ends_with(".jsx") {
                "4"
            } else {
                "3"
            };
            (disk_content, kind)
        };

        let escaped = content
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");

        let open_args = format!(
            r#"{{"file":"{}","fileContent":"{}","scriptKindName":"{}"}}"#,
            file.replace('"', "\\\""),
            escaped,
            script_kind
        );

        if let Some(ts) = self.tsserver_mut() {
            ts.send_notification("open", &open_args);
        }
        self.tsserver_open_files.insert(file.to_string());
    }

    /// Handle __sidex.lsp commands. Payload format:
    /// {"server":"rust-analyzer","cmd":"rust-analyzer","args":[],"method":"textDocument/completion","params":{...}}
    fn execute_lsp_command(&mut self, payload: &str) -> Result<String, String> {
        let server_name = extract_json_string(payload, "server")
            .ok_or_else(|| "lsp: missing server".to_string())?;
        let method = extract_json_string(payload, "method")
            .ok_or_else(|| "lsp: missing method".to_string())?;
        let params = extract_json_object(payload, "params").unwrap_or_else(|| "{}".to_string());

        if !self.lsp_servers.contains_key(&server_name) {
            let cmd = extract_json_string(payload, "cmd").unwrap_or_else(|| server_name.clone());
            let extra_args = extract_json_string_array(payload, "args");
            let root = self
                .workspace_folders
                .first()
                .map(|f| format!("file://{f}"))
                .unwrap_or_else(|| "file:///tmp".to_string());
            let binary = find_binary(
                &cmd,
                &[
                    &format!("/usr/local/bin/{cmd}"),
                    &format!("/opt/homebrew/bin/{cmd}"),
                    &format!("/usr/bin/{cmd}"),
                ],
            )
            .ok_or_else(|| format!("lsp: {cmd} binary not found"))?;

            // Relative args are resolved against CARGO_MANIFEST_DIR for bundled servers
            let cargo_dir = env!("CARGO_MANIFEST_DIR");
            let resolved_args: Vec<String> = extra_args
                .iter()
                .map(|a| {
                    if !a.starts_with('/') {
                        let resolved = format!("{}/{}", cargo_dir, a);
                        if std::path::Path::new(&resolved).exists() {
                            return resolved;
                        }
                    }
                    a.clone()
                })
                .collect();
            let args_refs: Vec<&str> = resolved_args.iter().map(|s| s.as_str()).collect();
            let server = LspServerProcess::spawn(&server_name, &binary, &args_refs, &root)
                .ok_or_else(|| format!("lsp: failed to start {server_name}"))?;

            self.lsp_servers.insert(server_name.clone(), server);
            self.lsp_open_files
                .insert(server_name.clone(), std::collections::HashSet::new());
        }

        // Auto-open files for textDocument/* requests
        if let Some(td) = extract_json_object(&params, "textDocument") {
            if let Some(uri) = extract_json_string(&td, "uri") {
                let files = self
                    .lsp_open_files
                    .get(&server_name)
                    .cloned()
                    .unwrap_or_default();
                if !files.contains(&uri) {
                    let file_path = uri.strip_prefix("file://").unwrap_or(&uri);
                    let lang_id = extract_json_string(&td, "languageId").unwrap_or_else(|| {
                        if file_path.ends_with(".rs") {
                            "rust".to_string()
                        } else if file_path.ends_with(".go") {
                            "go".to_string()
                        } else if file_path.ends_with(".py") {
                            "python".to_string()
                        } else if file_path.ends_with(".c") || file_path.ends_with(".h") {
                            "c".to_string()
                        } else if file_path.ends_with(".cpp") || file_path.ends_with(".cc") {
                            "cpp".to_string()
                        } else if file_path.ends_with(".css") {
                            "css".to_string()
                        } else if file_path.ends_with(".scss") {
                            "scss".to_string()
                        } else if file_path.ends_with(".less") {
                            "less".to_string()
                        } else if file_path.ends_with(".html") || file_path.ends_with(".htm") {
                            "html".to_string()
                        } else if file_path.ends_with(".json") {
                            "json".to_string()
                        } else if file_path.ends_with(".jsonc") {
                            "jsonc".to_string()
                        } else if file_path.ends_with(".ts") || file_path.ends_with(".tsx") {
                            "typescript".to_string()
                        } else if file_path.ends_with(".js") || file_path.ends_with(".jsx") {
                            "javascript".to_string()
                        } else {
                            "plaintext".to_string()
                        }
                    });
                    let content = self
                        .documents
                        .get(&uri)
                        .map(|d| d.text.clone())
                        .or_else(|| std::fs::read_to_string(file_path).ok())
                        .unwrap_or_default();

                    if let Some(server) = self.lsp_servers.get_mut(&server_name) {
                        server.open_file(&uri, &lang_id, &content);
                    }
                    if let Some(files) = self.lsp_open_files.get_mut(&server_name) {
                        files.insert(uri);
                    }
                }
            }
        }

        let server = self
            .lsp_servers
            .get_mut(&server_name)
            .ok_or_else(|| format!("lsp: {server_name} not found"))?;
        let response = server
            .request_sync(&method, &params)
            .ok_or_else(|| format!("lsp: no response for {method}"))?;

        Ok(response)
    }

    fn execute_tsserver_command(&mut self, payload: &str) -> Result<String, String> {
        let command = extract_json_string(payload, "command")
            .ok_or_else(|| "tsserver: missing command field".to_string())?;
        let arguments = extract_json_object(payload, "arguments")
            .ok_or_else(|| "tsserver: missing arguments field".to_string())?;
        let file = extract_json_string(&arguments, "file")
            .ok_or_else(|| "tsserver: missing arguments.file".to_string())?;

        self.ensure_file_open(&file);

        let ts = self
            .tsserver_mut()
            .ok_or_else(|| "tsserver not available".to_string())?;

        let response = ts
            .request_sync(&command, &arguments)
            .ok_or_else(|| format!("tsserver: no response for {command}"))?;

        Ok(response)
    }
}

// Minimal JSON field extractor (no external dependency)
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let search = format!(r#""{key}":"#);
    let start = json.find(&search)? + search.len();
    let rest = json[start..].trim_start();
    if rest.starts_with('"') {
        let inner = &rest[1..];
        let mut result = String::new();
        let mut chars = inner.chars();
        loop {
            match chars.next()? {
                '\\' => match chars.next()? {
                    '"' => result.push('"'),
                    '\\' => result.push('\\'),
                    'n' => result.push('\n'),
                    'r' => result.push('\r'),
                    't' => result.push('\t'),
                    c => {
                        result.push('\\');
                        result.push(c);
                    }
                },
                '"' => break,
                c => result.push(c),
            }
        }
        Some(result)
    } else {
        None
    }
}

fn extract_json_object(json: &str, key: &str) -> Option<String> {
    let search = format!(r#""{key}":"#);
    let start = json.find(&search)? + search.len();
    let rest = json[start..].trim_start();
    if !rest.starts_with('{') {
        return None;
    }
    let mut depth = 0usize;
    let mut end = 0;
    for (i, c) in rest.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    if end > 0 {
        Some(rest[..end].to_string())
    } else {
        None
    }
}

fn extract_json_string_array(json: &str, key: &str) -> Vec<String> {
    let search = format!(r#""{key}":"#);
    let start = match json.find(&search) {
        Some(s) => s + search.len(),
        None => return vec![],
    };
    let rest = json[start..].trim_start();
    if !rest.starts_with('[') {
        return vec![];
    }
    let mut items = Vec::new();
    let mut i = 1; // skip '['
    let bytes = rest.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b']' {
            break;
        }
        if bytes[i] == b'"' {
            let str_start = i + 1;
            let mut str_end = str_start;
            while str_end < bytes.len() && bytes[str_end] != b'"' {
                if bytes[str_end] == b'\\' {
                    str_end += 1;
                }
                str_end += 1;
            }
            items.push(rest[str_start..str_end].to_string());
            i = str_end + 1;
        } else {
            i += 1;
        }
    }
    items
}

// ---------------------------------------------------------------------------
// Loaded WASM extension instance
// ---------------------------------------------------------------------------

struct LoadedWasmExtension {
    #[allow(dead_code)]
    id: String,
    store: Store<WasmHostState>,
    bindings: SidexExtension,
}

// ---------------------------------------------------------------------------
// WASM extension runtime — manages wasmtime engine and all loaded extensions
// ---------------------------------------------------------------------------

pub struct WasmExtensionRuntime {
    inner: Mutex<WasmRuntimeState>,
}

struct WasmRuntimeState {
    engine: Engine,
    linker: Linker<WasmHostState>,
    extensions: HashMap<String, LoadedWasmExtension>,
    shared_documents: HashMap<String, DocumentData>,
    shared_workspace_folders: Vec<String>,
}

impl WasmExtensionRuntime {
    pub fn new() -> Result<Self> {
        let mut config = Config::new();
        config.wasm_component_model(true);

        let engine =
            Engine::new(&config).map_err(|e| anyhow::anyhow!("create wasmtime engine: {e}"))?;
        let mut linker: Linker<WasmHostState> = Linker::new(&engine);

        wasmtime_wasi::p2::add_to_linker_sync(&mut linker)
            .map_err(|e| anyhow::anyhow!("add WASI to linker: {e}"))?;

        SidexExtension::add_to_linker::<_, wasmtime::component::HasSelf<_>>(&mut linker, |x| x)
            .map_err(|e| anyhow::anyhow!("add WIT bindings to linker: {e}"))?;

        Ok(Self {
            inner: Mutex::new(WasmRuntimeState {
                engine,
                linker,
                extensions: HashMap::new(),
                shared_documents: HashMap::new(),
                shared_workspace_folders: Vec::new(),
            }),
        })
    }

    pub fn load_extension(&self, manifest: &ExtensionManifest) -> Result<(), String> {
        let wasm_file = manifest
            .wasm_binary
            .as_ref()
            .ok_or("manifest has no wasm_binary")?;
        let wasm_path = Path::new(&manifest.path).join(wasm_file);

        log::info!(
            "loading WASM extension: {} from {}",
            manifest.id,
            wasm_path.display()
        );

        let mut guard = self.inner.lock().map_err(|e| e.to_string())?;

        let component = Component::from_file(&guard.engine, &wasm_path)
            .map_err(|e| format!("failed to load wasm component {}: {e}", wasm_path.display()))?;

        let mut store = Store::new(&guard.engine, WasmHostState::new());

        {
            let host_state = store.data_mut();
            for (uri, doc) in &guard.shared_documents {
                host_state.documents.insert(
                    uri.clone(),
                    DocumentData {
                        text: doc.text.clone(),
                        language_id: doc.language_id.clone(),
                    },
                );
            }
            host_state.workspace_folders = guard.shared_workspace_folders.clone();
        }

        let bindings = SidexExtension::instantiate(&mut store, &component, &guard.linker)
            .map_err(|e| format!("failed to instantiate {}: {e}", manifest.id))?;

        bindings
            .sidex_extension_extension_api()
            .call_activate(&mut store)
            .map_err(|e| format!("activate failed for {}: {e}", manifest.id))?
            .map_err(|e| format!("extension {} returned error: {e}", manifest.id))?;

        log::info!("loaded WASM extension: {}", manifest.id);

        guard.extensions.insert(
            manifest.id.clone(),
            LoadedWasmExtension {
                id: manifest.id.clone(),
                store,
                bindings,
            },
        );

        Ok(())
    }

    pub fn unload_extension(&self, id: &str) -> Result<(), String> {
        let mut guard = self.inner.lock().map_err(|e| e.to_string())?;
        if let Some(mut ext) = guard.extensions.remove(id) {
            let _ = ext
                .bindings
                .sidex_extension_extension_api()
                .call_deactivate(&mut ext.store);
            log::info!("unloaded WASM extension: {id}");
        }
        Ok(())
    }

    pub fn loaded_extension_ids(&self) -> Vec<String> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.extensions.keys().cloned().collect()
    }

    pub fn sync_document(&self, uri: &str, language_id: &str, text: &str) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.shared_documents.insert(
                uri.to_string(),
                DocumentData {
                    text: text.to_string(),
                    language_id: language_id.to_string(),
                },
            );
            for ext in guard.extensions.values_mut() {
                ext.store.data_mut().documents.insert(
                    uri.to_string(),
                    DocumentData {
                        text: text.to_string(),
                        language_id: language_id.to_string(),
                    },
                );
                let state = ext.store.data_mut();
                let file = uri.strip_prefix("file://").unwrap_or(uri);
                if state.tsserver_open_files.contains(file) {
                    state.tsserver_open_files.remove(file);
                    state.ensure_file_open(file);
                }
            }
        }
    }

    pub fn close_document(&self, uri: &str) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.shared_documents.remove(uri);
            for ext in guard.extensions.values_mut() {
                ext.store.data_mut().documents.remove(uri);
            }
        }
    }

    pub fn sync_workspace_folders(&self, folders: &[String]) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.shared_workspace_folders = folders.to_vec();
            for ext in guard.extensions.values_mut() {
                ext.store.data_mut().workspace_folders = folders.to_vec();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Serialized provider result types for Tauri commands
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmCompletionResult {
    pub items: Vec<serde_json::Value>,
    pub is_incomplete: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmHoverResult {
    pub contents: Vec<String>,
    pub range: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Provider dispatch helpers
// ---------------------------------------------------------------------------

fn make_doc_ctx(uri: &str, language_id: &str, version: u32) -> wit_types::DocumentContext {
    wit_types::DocumentContext {
        uri: uri.to_string(),
        language_id: language_id.to_string(),
        version,
    }
}

fn make_position(line: u32, character: u32) -> wit_types::Position {
    wit_types::Position { line, character }
}

fn serialize_range(r: &wit_types::Range) -> serde_json::Value {
    serde_json::json!({
        "start": { "line": r.start.line, "character": r.start.character },
        "end": { "line": r.end.line, "character": r.end.character },
    })
}

fn serialize_location(l: &wit_types::Location) -> serde_json::Value {
    serde_json::json!({
        "uri": l.uri,
        "range": serialize_range(&l.range),
    })
}

// ---------------------------------------------------------------------------
// Tauri commands — WASM provider dispatch
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WasmProviderParams {
    pub extension_id: String,
    pub uri: String,
    pub language_id: String,
    pub version: u32,
    pub line: u32,
    pub character: u32,
}

#[tauri::command]
pub async fn wasm_provide_completion(
    params: WasmProviderParams,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<Option<WasmCompletionResult>, String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    let ext = guard
        .extensions
        .get_mut(&params.extension_id)
        .ok_or_else(|| format!("extension not loaded: {}", params.extension_id))?;

    let ctx = make_doc_ctx(&params.uri, &params.language_id, params.version);
    let pos = make_position(params.line, params.character);

    let result = ext
        .bindings
        .sidex_extension_extension_api()
        .call_provide_completion(&mut ext.store, &ctx, pos)
        .map_err(|e| format!("completion call failed: {e}"))?;

    Ok(result.map(|cl| WasmCompletionResult {
        items: cl
            .items
            .iter()
            .map(|item| {
                serde_json::json!({
                    "label": item.label,
                    "kind": item.kind,
                    "detail": item.detail,
                    "documentation": item.documentation,
                    "insertText": item.insert_text.as_deref().unwrap_or(&item.label),
                    "sortText": item.sort_text,
                    "filterText": item.filter_text,
                })
            })
            .collect(),
        is_incomplete: cl.is_incomplete,
    }))
}

#[tauri::command]
pub async fn wasm_provide_hover(
    params: WasmProviderParams,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<Option<WasmHoverResult>, String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    let ext = guard
        .extensions
        .get_mut(&params.extension_id)
        .ok_or_else(|| format!("extension not loaded: {}", params.extension_id))?;

    let ctx = make_doc_ctx(&params.uri, &params.language_id, params.version);
    let pos = make_position(params.line, params.character);

    let result = ext
        .bindings
        .sidex_extension_extension_api()
        .call_provide_hover(&mut ext.store, &ctx, pos)
        .map_err(|e| format!("hover call failed: {e}"))?;

    Ok(result.map(|h| WasmHoverResult {
        contents: h.contents,
        range: h.range.as_ref().map(serialize_range),
    }))
}

#[tauri::command]
pub async fn wasm_provide_definition(
    params: WasmProviderParams,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    let ext = guard
        .extensions
        .get_mut(&params.extension_id)
        .ok_or_else(|| format!("extension not loaded: {}", params.extension_id))?;

    let ctx = make_doc_ctx(&params.uri, &params.language_id, params.version);
    let pos = make_position(params.line, params.character);

    let result = ext
        .bindings
        .sidex_extension_extension_api()
        .call_provide_definition(&mut ext.store, &ctx, pos)
        .map_err(|e| format!("definition call failed: {e}"))?;

    Ok(result.iter().map(serialize_location).collect())
}

#[tauri::command]
pub async fn wasm_provide_references(
    params: WasmProviderParams,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    let ext = guard
        .extensions
        .get_mut(&params.extension_id)
        .ok_or_else(|| format!("extension not loaded: {}", params.extension_id))?;

    let ctx = make_doc_ctx(&params.uri, &params.language_id, params.version);
    let pos = make_position(params.line, params.character);

    let result = ext
        .bindings
        .sidex_extension_extension_api()
        .call_provide_references(&mut ext.store, &ctx, pos)
        .map_err(|e| format!("references call failed: {e}"))?;

    Ok(result.iter().map(serialize_location).collect())
}

#[tauri::command]
pub async fn wasm_provide_document_symbols(
    params: WasmProviderParams,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    let ext = guard
        .extensions
        .get_mut(&params.extension_id)
        .ok_or_else(|| format!("extension not loaded: {}", params.extension_id))?;

    let ctx = make_doc_ctx(&params.uri, &params.language_id, params.version);

    let result = ext
        .bindings
        .sidex_extension_extension_api()
        .call_provide_document_symbols(&mut ext.store, &ctx)
        .map_err(|e| format!("document symbols call failed: {e}"))?;

    Ok(result
        .iter()
        .map(|s| {
            serde_json::json!({
                "name": s.name,
                "detail": s.detail,
                "kind": s.kind,
                "range": serialize_range(&s.range),
                "selectionRange": serialize_range(&s.selection_range),
            })
        })
        .collect())
}

#[tauri::command]
pub async fn wasm_provide_formatting(
    params: WasmProviderParams,
    tab_size: u32,
    insert_spaces: bool,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    let ext = guard
        .extensions
        .get_mut(&params.extension_id)
        .ok_or_else(|| format!("extension not loaded: {}", params.extension_id))?;

    let ctx = make_doc_ctx(&params.uri, &params.language_id, params.version);

    let result = ext
        .bindings
        .sidex_extension_extension_api()
        .call_provide_formatting(&mut ext.store, &ctx, tab_size, insert_spaces)
        .map_err(|e| format!("formatting call failed: {e}"))?;

    Ok(result
        .iter()
        .map(|e| {
            serde_json::json!({
                "range": serialize_range(&e.range),
                "newText": e.new_text,
            })
        })
        .collect())
}

#[tauri::command]
pub async fn wasm_load_extension(
    extension_id: String,
    wasm_path: String,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<(), String> {
    let manifest = ExtensionManifest {
        id: extension_id.clone(),
        publisher: String::new(),
        name: extension_id.clone(),
        display_name: extension_id.clone(),
        version: "0.0.0".to_string(),
        path: std::path::Path::new(&wasm_path)
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .to_string_lossy()
            .to_string(),
        kind: crate::commands::extension_platform::ExtensionKind::Wasm,
        main: None,
        browser: None,
        wasm_binary: Some(
            std::path::Path::new(&wasm_path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
        ),
        source: "user".to_string(),
        builtin: false,
        activation_events: Vec::new(),
        contributes_keys: Vec::new(),
    };
    state.load_extension(&manifest)
}

#[tauri::command]
pub async fn wasm_unload_extension(
    extension_id: String,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<(), String> {
    state.unload_extension(&extension_id)
}

#[tauri::command]
pub async fn wasm_list_extensions(
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<Vec<String>, String> {
    Ok(state.loaded_extension_ids())
}

#[tauri::command]
pub async fn wasm_sync_document(
    uri: String,
    language_id: String,
    text: String,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<(), String> {
    state.sync_document(&uri, &language_id, &text);
    Ok(())
}

#[tauri::command]
pub async fn wasm_close_document(
    uri: String,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<(), String> {
    state.close_document(&uri);
    Ok(())
}

#[tauri::command]
pub async fn wasm_sync_workspace_folders(
    folders: Vec<String>,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<(), String> {
    state.sync_workspace_folders(&folders);
    Ok(())
}

/// Broadcast completion request to all loaded WASM extensions and merge results.
#[tauri::command]
pub async fn wasm_provide_completion_all(
    uri: String,
    language_id: String,
    version: u32,
    line: u32,
    character: u32,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<Option<WasmCompletionResult>, String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    let ctx = make_doc_ctx(&uri, &language_id, version);
    let pos = make_position(line, character);

    let mut all_items = Vec::new();
    let mut any_incomplete = false;

    let ext_ids: Vec<String> = guard.extensions.keys().cloned().collect();
    for ext_id in &ext_ids {
        if let Some(ext) = guard.extensions.get_mut(ext_id) {
            match ext
                .bindings
                .sidex_extension_extension_api()
                .call_provide_completion(&mut ext.store, &ctx, pos)
            {
                Ok(Some(cl)) => {
                    any_incomplete = any_incomplete || cl.is_incomplete;
                    for item in &cl.items {
                        all_items.push(serde_json::json!({
                            "label": item.label,
                            "kind": item.kind,
                            "detail": item.detail,
                            "documentation": item.documentation,
                            "insertText": item.insert_text.as_deref().unwrap_or(&item.label),
                            // "0" prefix so WASM extension results sort before other sources
                            "sortText": item.sort_text.as_deref().map(|s| format!("0{s}")).unwrap_or_else(|| format!("0{}", item.label)),
                            "filterText": item.filter_text,
                        }));
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    log::warn!("WASM completion error from {ext_id}: {e}");
                }
            }
        }
    }

    if all_items.is_empty() {
        Ok(None)
    } else {
        Ok(Some(WasmCompletionResult {
            items: all_items,
            is_incomplete: any_incomplete,
        }))
    }
}

/// Broadcast hover request to all loaded WASM extensions and return first non-empty result.
#[tauri::command]
pub async fn wasm_provide_hover_all(
    uri: String,
    language_id: String,
    version: u32,
    line: u32,
    character: u32,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<Option<WasmHoverResult>, String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    let ctx = make_doc_ctx(&uri, &language_id, version);
    let pos = make_position(line, character);

    let ext_ids: Vec<String> = guard.extensions.keys().cloned().collect();
    for ext_id in &ext_ids {
        if let Some(ext) = guard.extensions.get_mut(ext_id) {
            match ext
                .bindings
                .sidex_extension_extension_api()
                .call_provide_hover(&mut ext.store, &ctx, pos)
            {
                Ok(Some(h)) if !h.contents.is_empty() => {
                    return Ok(Some(WasmHoverResult {
                        contents: h.contents,
                        range: h.range.as_ref().map(serialize_range),
                    }));
                }
                Ok(_) => {}
                Err(e) => {
                    log::warn!("WASM hover error from {ext_id}: {e}");
                }
            }
        }
    }
    Ok(None)
}

/// Broadcast definition request to all loaded WASM extensions.
#[tauri::command]
pub async fn wasm_provide_definition_all(
    uri: String,
    language_id: String,
    version: u32,
    line: u32,
    character: u32,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    let ctx = make_doc_ctx(&uri, &language_id, version);
    let pos = make_position(line, character);

    let mut all_locs = Vec::new();
    let ext_ids: Vec<String> = guard.extensions.keys().cloned().collect();
    for ext_id in &ext_ids {
        if let Some(ext) = guard.extensions.get_mut(ext_id) {
            match ext
                .bindings
                .sidex_extension_extension_api()
                .call_provide_definition(&mut ext.store, &ctx, pos)
            {
                Ok(locs) => {
                    for l in &locs {
                        all_locs.push(serialize_location(l));
                    }
                }
                Err(e) => {
                    log::warn!("WASM definition error from {ext_id}: {e}");
                }
            }
        }
    }
    Ok(all_locs)
}

/// Broadcast document symbols request to all loaded WASM extensions.
#[tauri::command]
pub async fn wasm_provide_document_symbols_all(
    uri: String,
    language_id: String,
    version: u32,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    let ctx = make_doc_ctx(&uri, &language_id, version);

    let mut all_symbols = Vec::new();
    let ext_ids: Vec<String> = guard.extensions.keys().cloned().collect();
    for ext_id in &ext_ids {
        if let Some(ext) = guard.extensions.get_mut(ext_id) {
            match ext
                .bindings
                .sidex_extension_extension_api()
                .call_provide_document_symbols(&mut ext.store, &ctx)
            {
                Ok(symbols) => {
                    for s in &symbols {
                        all_symbols.push(serde_json::json!({
                            "name": s.name,
                            "detail": s.detail,
                            "kind": s.kind,
                            "range": serialize_range(&s.range),
                            "selectionRange": serialize_range(&s.selection_range),
                        }));
                    }
                }
                Err(e) => {
                    log::warn!("WASM document symbols error from {ext_id}: {e}");
                }
            }
        }
    }
    Ok(all_symbols)
}

/// Broadcast formatting request to all loaded WASM extensions.
#[tauri::command]
pub async fn wasm_provide_formatting_all(
    uri: String,
    language_id: String,
    version: u32,
    tab_size: u32,
    insert_spaces: bool,
    state: State<'_, Arc<WasmExtensionRuntime>>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut guard = state.inner.lock().map_err(|e| e.to_string())?;
    let ctx = make_doc_ctx(&uri, &language_id, version);

    let ext_ids: Vec<String> = guard.extensions.keys().cloned().collect();
    for ext_id in &ext_ids {
        if let Some(ext) = guard.extensions.get_mut(ext_id) {
            match ext
                .bindings
                .sidex_extension_extension_api()
                .call_provide_formatting(&mut ext.store, &ctx, tab_size, insert_spaces)
            {
                Ok(edits) if !edits.is_empty() => {
                    return Ok(edits
                        .iter()
                        .map(|e| {
                            serde_json::json!({
                                "range": serialize_range(&e.range),
                                "newText": e.new_text,
                            })
                        })
                        .collect());
                }
                Ok(_) => {}
                Err(e) => {
                    log::warn!("WASM formatting error from {ext_id}: {e}");
                }
            }
        }
    }
    Ok(vec![])
}
