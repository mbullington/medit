pub mod actions;
pub mod diagnostics;
pub mod transport;

use std::{collections::HashMap, io, path::Path};

use lsp_types::{Diagnostic, Position, PublishDiagnosticsParams, Range, Url};
use serde_json::{Value, json};

use crate::{buffer::Buffer, config::LspServerConfig};

use self::{
    actions::CodeActionItem,
    transport::{LspTransport, TransportPoll},
};

#[derive(Clone, Debug)]
pub enum LspEvent {
    Diagnostics(Vec<Diagnostic>),
    CodeActions(Vec<CodeActionItem>),
    Status(String),
    Failed(String),
}

#[derive(Clone, Debug)]
enum PendingRequest {
    Initialize,
    CodeActions,
    ExecuteCommand,
}

pub struct LspClient {
    transport: LspTransport,
    config: LspServerConfig,
    uri: Url,
    root_uri: Url,
    next_id: i64,
    pending: HashMap<i64, PendingRequest>,
    initialized: bool,
    opened: bool,
    save_include_text: Option<bool>,
    disconnected: bool,
}

impl LspClient {
    pub fn spawn(config: LspServerConfig, buffer: &Buffer, root: &Path) -> io::Result<Self> {
        let transport = LspTransport::spawn(&config)?;
        let path = buffer
            .path()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| root.join("untitled"));
        let uri = file_uri(&path)?;
        let root_uri = file_uri(root)?;
        let mut this = Self {
            transport,
            config,
            uri,
            root_uri,
            next_id: 1,
            pending: HashMap::new(),
            initialized: false,
            opened: false,
            save_include_text: None,
            disconnected: false,
        };
        this.initialize();
        Ok(this)
    }

    pub fn poll(&mut self, buffer: &Buffer) -> Vec<LspEvent> {
        let mut events = Vec::new();
        loop {
            match self.transport.try_recv() {
                TransportPoll::Message(message) => {
                    self.handle_message(message, buffer, &mut events);
                }
                TransportPoll::Empty => break,
                TransportPoll::Disconnected(error) => {
                    if !self.disconnected {
                        self.disconnected = true;
                        events.push(LspEvent::Failed(format!("LSP disconnected: {error}")));
                    }
                    break;
                }
            }
        }
        events
    }

    pub fn notify_did_change(&mut self, buffer: &Buffer) {
        if !self.initialized || !self.opened {
            return;
        }
        self.send_notification(
            "textDocument/didChange",
            json!({
                "textDocument": {
                    "uri": self.uri,
                    "version": buffer.version(),
                },
                "contentChanges": [{ "text": buffer.text() }],
            }),
        );
    }

    pub fn notify_did_save(&self, buffer: &Buffer) {
        if !self.initialized || !self.opened {
            return;
        }
        let Some(include_text) = self.save_include_text else {
            return;
        };
        let mut params = json!({ "textDocument": { "uri": self.uri } });
        if include_text {
            params["text"] = Value::String(buffer.text());
        }
        self.send_notification("textDocument/didSave", params);
    }

    pub fn request_code_actions(&mut self, buffer: &Buffer, diagnostics: Vec<Diagnostic>) {
        if !self.initialized || !self.opened {
            return;
        }
        let (line, byte_col) = buffer.line_col();
        let utf16_col = buffer.utf16_col_for_byte_col(line, byte_col);
        let range = diagnostics
            .first()
            .map(|d| d.range)
            .unwrap_or_else(|| Range {
                start: Position::new(line as u32, utf16_col as u32),
                end: Position::new(line as u32, utf16_col as u32),
            });
        let id = self.send_request(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": self.uri },
                "range": range,
                "context": {
                    "diagnostics": diagnostics,
                    "only": ["quickfix", "source.fixAll", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite"],
                },
            }),
        );
        self.pending.insert(id, PendingRequest::CodeActions);
    }

    pub fn execute_command(&mut self, command: &Value) {
        let Some(command_name) = command.get("command").and_then(|v| v.as_str()) else {
            return;
        };
        let arguments = command
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!([]));
        let id = self.send_request(
            "workspace/executeCommand",
            json!({ "command": command_name, "arguments": arguments }),
        );
        self.pending.insert(id, PendingRequest::ExecuteCommand);
    }

    pub fn language_id(&self) -> &str {
        &self.config.language_id
    }

    fn initialize(&mut self) {
        let id = self.send_request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": self.root_uri,
                "capabilities": {
                    "textDocument": {
                        "publishDiagnostics": { "relatedInformation": true },
                        "codeAction": {
                            "dynamicRegistration": false,
                            "codeActionLiteralSupport": {
                                "codeActionKind": {
                                    "valueSet": ["", "quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite", "source", "source.fixAll"]
                                }
                            }
                        },
                        "synchronization": { "didSave": true, "willSave": false, "willSaveWaitUntil": false }
                    },
                    "workspace": { "applyEdit": true, "configuration": true, "workspaceEdit": { "documentChanges": true } },
                    "general": { "positionEncodings": ["utf-16"] }
                },
                "workspaceFolders": null,
            }),
        );
        self.pending.insert(id, PendingRequest::Initialize);
    }

    fn did_open(&mut self, buffer: &Buffer) {
        self.send_notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": self.uri,
                    "languageId": self.config.language_id,
                    "version": buffer.version(),
                    "text": buffer.text(),
                }
            }),
        );
        self.opened = true;
    }

    fn handle_message(&mut self, message: Value, buffer: &Buffer, events: &mut Vec<LspEvent>) {
        if let Some(method) = message.get("method").and_then(|m| m.as_str()) {
            if let Some(id) = message.get("id").and_then(json_id) {
                self.handle_server_request(id, method, message.get("params"), events);
                return;
            }

            match method {
                "textDocument/publishDiagnostics" => {
                    if let Some(params) = message.get("params")
                        && let Ok(params) =
                            serde_json::from_value::<PublishDiagnosticsParams>(params.clone())
                        && let Some(diagnostics) =
                            diagnostics_for_buffer(params, &self.uri, buffer.version())
                    {
                        events.push(LspEvent::Diagnostics(diagnostics));
                    }
                }
                "window/showMessage" | "window/logMessage" => {
                    if let Some(msg) = message
                        .get("params")
                        .and_then(|p| p.get("message"))
                        .and_then(|m| m.as_str())
                    {
                        events.push(LspEvent::Status(format!("LSP: {msg}")));
                    }
                }
                _ => {}
            }
            return;
        }

        let Some(id) = message.get("id").and_then(json_id) else {
            return;
        };
        let Some(kind) = self.pending.remove(&id) else {
            return;
        };
        if let Some(error) = message.get("error") {
            let error = format!("LSP error: {error}");
            if matches!(kind, PendingRequest::Initialize) {
                events.push(LspEvent::Failed(error));
            } else {
                events.push(LspEvent::Status(error));
            }
            return;
        }
        let result = message.get("result").cloned().unwrap_or(Value::Null);
        match kind {
            PendingRequest::Initialize => {
                self.save_include_text = save_notification_options(&result);
                self.initialized = true;
                self.send_notification("initialized", json!({}));
                self.did_open(buffer);
                events.push(LspEvent::Status(format!(
                    "LSP attached ({})",
                    self.config.command
                )));
            }
            PendingRequest::CodeActions => {
                events.push(LspEvent::CodeActions(CodeActionItem::from_lsp_response(
                    &result,
                )));
            }
            PendingRequest::ExecuteCommand => {
                events.push(LspEvent::Status("LSP command executed".into()));
            }
        }
    }

    fn handle_server_request(
        &self,
        id: i64,
        method: &str,
        params: Option<&Value>,
        events: &mut Vec<LspEvent>,
    ) {
        match method {
            "workspace/configuration" => {
                let count = params
                    .and_then(|p| p.get("items"))
                    .and_then(|items| items.as_array())
                    .map_or(0, Vec::len);
                self.send_response(id, json!(vec![Value::Null; count]));
            }
            "workspace/applyEdit" => {
                self.send_response(
                    id,
                    json!({
                        "applied": false,
                        "failureReason": "medit applies edits from selected code actions only",
                    }),
                );
                events.push(LspEvent::Status(
                    "LSP requested workspace/applyEdit; not applied".into(),
                ));
            }
            "client/registerCapability"
            | "client/unregisterCapability"
            | "window/workDoneProgress/create"
            | "window/showMessageRequest" => {
                self.send_response(id, Value::Null);
            }
            _ => {
                self.send_response(id, Value::Null);
            }
        }
    }

    fn send_request(&mut self, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.transport.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        id
    }

    fn send_notification(&self, method: &str, params: Value) {
        self.transport.send(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }));
    }

    fn send_response(&self, id: i64, result: Value) {
        self.transport.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }));
    }
}

fn json_id(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

fn save_notification_options(initialize_result: &Value) -> Option<bool> {
    let save = initialize_result.pointer("/capabilities/textDocumentSync/save")?;
    match save {
        Value::Bool(true) => Some(false),
        Value::Object(options) => Some(
            options
                .get("includeText")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),
        _ => None,
    }
}

fn diagnostics_for_buffer(
    params: PublishDiagnosticsParams,
    uri: &Url,
    version: i32,
) -> Option<Vec<Diagnostic>> {
    if &params.uri != uri || params.version.is_some_and(|published| published != version) {
        return None;
    }
    Some(params.diagnostics)
}

fn file_uri(path: &Path) -> io::Result<Url> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let absolute = absolute.canonicalize().unwrap_or(absolute);
    Url::from_file_path(&absolute).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("cannot convert {} to a file URI", absolute.display()),
        )
    })
}

#[cfg(test)]
mod tests {
    use lsp_types::{Diagnostic, Position};

    use super::*;

    fn params(uri: Url, version: Option<i32>) -> PublishDiagnosticsParams {
        PublishDiagnosticsParams::new(
            uri,
            vec![Diagnostic::new_simple(
                Range::new(Position::new(0, 0), Position::new(0, 1)),
                "test".into(),
            )],
            version,
        )
    }

    #[test]
    fn accepts_only_current_document_diagnostics() {
        let current = Url::parse("file:///current.rs").unwrap();
        let other = Url::parse("file:///other.rs").unwrap();

        assert!(diagnostics_for_buffer(params(current.clone(), Some(3)), &current, 3).is_some());
        assert!(diagnostics_for_buffer(params(current.clone(), None), &current, 3).is_some());
        assert!(diagnostics_for_buffer(params(current.clone(), Some(2)), &current, 3).is_none());
        assert!(diagnostics_for_buffer(params(other, Some(3)), &current, 3).is_none());
    }

    #[test]
    fn reads_save_notification_options() {
        assert_eq!(
            save_notification_options(&json!({
                "capabilities": { "textDocumentSync": { "save": true } }
            })),
            Some(false)
        );
        assert_eq!(
            save_notification_options(&json!({
                "capabilities": {
                    "textDocumentSync": { "save": { "includeText": true } }
                }
            })),
            Some(true)
        );
        assert_eq!(
            save_notification_options(&json!({
                "capabilities": { "textDocumentSync": 1 }
            })),
            None
        );
    }

    #[test]
    fn file_uris_escape_reserved_characters() {
        let uri = file_uri(Path::new("/tmp/medit # test.rs")).unwrap();

        assert_eq!(uri.as_str(), "file:///tmp/medit%20%23%20test.rs");
    }
}
