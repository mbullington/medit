use serde_json::Value;

use crate::buffer::Buffer;

#[derive(Clone, Debug)]
pub struct CodeActionItem {
    pub title: String,
    pub edit: Option<Value>,
    pub command: Option<Value>,
}

impl CodeActionItem {
    pub fn from_lsp_response(value: &Value) -> Vec<Self> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        if let Some(command) = item.get("command").and_then(|v| v.as_str()) {
                            return Some(Self {
                                title: command.to_string(),
                                edit: None,
                                command: Some(item.clone()),
                            });
                        }
                        let title = item.get("title")?.as_str()?.to_string();
                        Some(Self {
                            title,
                            edit: item.get("edit").cloned(),
                            command: item.get("command").cloned(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug)]
struct PendingEdit {
    start: usize,
    end: usize,
    new_text: String,
}

pub fn apply_workspace_edit(buffer: &mut Buffer, edit: &Value) -> usize {
    let mut edits = Vec::new();

    if let Some(changes) = edit.get("changes").and_then(|v| v.as_object()) {
        for text_edits in changes.values() {
            collect_text_edits(buffer, text_edits, &mut edits);
        }
    }

    if let Some(document_changes) = edit.get("documentChanges").and_then(|v| v.as_array()) {
        for change in document_changes {
            if let Some(text_edits) = change.get("edits") {
                collect_text_edits(buffer, text_edits, &mut edits);
            }
        }
    }

    edits.sort_by(|a, b| b.start.cmp(&a.start).then_with(|| b.end.cmp(&a.end)));
    let count = edits.len();
    for edit in edits {
        buffer.replace_range(edit.start..edit.end, &edit.new_text);
    }
    count
}

fn collect_text_edits(buffer: &Buffer, text_edits: &Value, out: &mut Vec<PendingEdit>) {
    let Some(items) = text_edits.as_array() else {
        return;
    };
    for item in items {
        let Some(range) = item.get("range") else {
            continue;
        };
        let Some(start) = range.get("start") else {
            continue;
        };
        let Some(end) = range.get("end") else {
            continue;
        };
        let Some(start_line) = start.get("line").and_then(|v| v.as_u64()) else {
            continue;
        };
        let Some(start_col) = start.get("character").and_then(|v| v.as_u64()) else {
            continue;
        };
        let Some(end_line) = end.get("line").and_then(|v| v.as_u64()) else {
            continue;
        };
        let Some(end_col) = end.get("character").and_then(|v| v.as_u64()) else {
            continue;
        };
        let new_text = item
            .get("newText")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        out.push(PendingEdit {
            start: buffer.offset_for_line_col(start_line as usize, start_col as usize),
            end: buffer.offset_for_line_col(end_line as usize, end_col as usize),
            new_text,
        });
    }
}
