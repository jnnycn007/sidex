use sidex_extension_sdk::prelude::*;

pub struct RustLanguageExtension;

impl SidexExtension for RustLanguageExtension {
    fn activate() -> Result<(), String> {
        Ok(())
    }

    fn deactivate() {}

    fn get_name() -> String {
        "Rust Language Features".to_string()
    }

    fn get_activation_events() -> Vec<String> {
        vec!["onLanguage:rust".to_string()]
    }

    fn get_commands() -> Vec<CommandDefinition> {
        vec![]
    }

    fn provide_completion(ctx: DocumentContext, pos: Position) -> Option<CompletionList> {
        if ctx.language_id != "rust" {
            return None;
        }
        if let Some(resp) = lsp_request("textDocument/completion", &ctx, Some(pos), "") {
            if let Some(list) = parse_lsp_completions(&resp) {
                return Some(list);
            }
        }
        word_completions_from_doc(&ctx, pos)
    }

    fn provide_hover(ctx: DocumentContext, pos: Position) -> Option<HoverResult> {
        if ctx.language_id != "rust" {
            return None;
        }
        let resp = lsp_request("textDocument/hover", &ctx, Some(pos), "")?;
        parse_lsp_hover(&resp)
    }

    fn provide_definition(ctx: DocumentContext, pos: Position) -> Vec<Location> {
        if ctx.language_id != "rust" {
            return vec![];
        }
        lsp_request("textDocument/definition", &ctx, Some(pos), "")
            .and_then(|r| parse_lsp_locations(&r))
            .unwrap_or_default()
    }

    fn provide_references(ctx: DocumentContext, pos: Position) -> Vec<Location> {
        if ctx.language_id != "rust" {
            return vec![];
        }
        let extra = r#","context":{"includeDeclaration":true}"#;
        lsp_request("textDocument/references", &ctx, Some(pos), extra)
            .and_then(|r| parse_lsp_locations(&r))
            .unwrap_or_default()
    }

    fn provide_document_symbols(ctx: DocumentContext) -> Vec<DocumentSymbol> {
        if ctx.language_id != "rust" {
            return vec![];
        }
        lsp_request("textDocument/documentSymbol", &ctx, None, "")
            .and_then(|r| parse_lsp_symbols(&r))
            .unwrap_or_default()
    }

    fn provide_formatting(
        ctx: DocumentContext,
        tab_size: u32,
        insert_spaces: bool,
    ) -> Vec<TextEdit> {
        if ctx.language_id != "rust" {
            return vec![];
        }
        let extra = format!(
            r#","options":{{"tabSize":{},"insertSpaces":{}}}"#,
            tab_size, insert_spaces
        );
        lsp_request("textDocument/formatting", &ctx, None, &extra)
            .and_then(|r| parse_lsp_text_edits(&r))
            .unwrap_or_default()
    }

    fn on_file_event(events: Vec<FileEvent>) {
        for event in events {
            if event.uri.ends_with(".rs") {
                let ctx = DocumentContext {
                    uri: event.uri.clone(),
                    language_id: "rust".to_string(),
                    version: 0,
                };
                if let Some(resp) = lsp_request("textDocument/diagnostic", &ctx, None, "") {
                    if let Some(diags) = parse_lsp_diagnostics(&resp) {
                        host::publish_diagnostics(&event.uri, &diags);
                    }
                }
            }
        }
    }

    fn execute_command(id: String, _args: String) -> Result<String, String> {
        Err(format!("unknown: {id}"))
    }

    fn get_semantic_tokens_legend() -> Option<SemanticTokensLegend> {
        None
    }
    fn provide_type_definition(_: DocumentContext, _: Position) -> Vec<Location> {
        vec![]
    }
    fn provide_implementation(_: DocumentContext, _: Position) -> Vec<Location> {
        vec![]
    }
    fn provide_declaration(_: DocumentContext, _: Position) -> Vec<Location> {
        vec![]
    }
    fn provide_code_actions(_: DocumentContext, _: Range, _: Vec<Diagnostic>) -> Vec<CodeAction> {
        vec![]
    }
    fn provide_code_lenses(_: DocumentContext) -> Vec<CodeLens> {
        vec![]
    }
    fn provide_range_formatting(_: DocumentContext, _: Range, _: u32, _: bool) -> Vec<TextEdit> {
        vec![]
    }
    fn provide_signature_help(_: DocumentContext, _: Position) -> Option<SignatureHelpResult> {
        None
    }
    fn provide_document_highlights(_: DocumentContext, _: Position) -> Vec<DocumentHighlight> {
        vec![]
    }
    fn provide_rename(_: DocumentContext, _: Position, _: String) -> Option<RenameResult> {
        None
    }
    fn prepare_rename(_: DocumentContext, _: Position) -> Option<RenameLocation> {
        None
    }
    fn provide_inlay_hints(_: DocumentContext, _: Range) -> Vec<InlayHint> {
        vec![]
    }
    fn provide_document_links(_: DocumentContext) -> Vec<DocumentLink> {
        vec![]
    }
    fn provide_selection_ranges(_: DocumentContext, _: Vec<Position>) -> Vec<SelectionRange> {
        vec![]
    }
    fn provide_semantic_tokens(_: DocumentContext) -> Option<SemanticTokens> {
        None
    }
    fn provide_document_colors(_: DocumentContext) -> Vec<ColorInfo> {
        vec![]
    }
    fn provide_folding_ranges(_: DocumentContext) -> Vec<FoldingRange> {
        vec![]
    }
    fn provide_workspace_symbols(_: String) -> Vec<DocumentSymbol> {
        vec![]
    }
    fn on_configuration_changed(_: String) {}
    fn get_tree_children(_: String, _: Option<String>) -> Vec<TreeItem> {
        vec![]
    }
}

fn lsp_request(
    method: &str,
    ctx: &DocumentContext,
    pos: Option<Position>,
    extra_params: &str,
) -> Option<String> {
    let uri = &ctx.uri;
    let pos_json = pos
        .map(|p| {
            format!(
                r#","position":{{"line":{},"character":{}}}"#,
                p.line, p.character
            )
        })
        .unwrap_or_default();
    let payload = format!(
        r#"{{"server":"rust-analyzer","cmd":"rust-analyzer","method":"{method}","params":{{"textDocument":{{"uri":"{uri}"}}{pos_json}{extra_params}}}}}"#
    );
    host::execute_command("__sidex.lsp", &payload).ok()
}

fn parse_lsp_completions(json: &str) -> Option<CompletionList> {
    // LSP returns either { "items": [...] } or { "result": { "items": [...] } }
    let items_start = json.find("\"items\"")?;
    let arr_start = json[items_start..].find('[')? + items_start;
    let mut items = Vec::new();
    let chars: Vec<char> = json.chars().collect();
    let mut pos = arr_start + 1;

    while pos < chars.len() && items.len() < 100 {
        while pos < chars.len() && chars[pos].is_whitespace() {
            pos += 1;
        }
        if pos >= chars.len() || chars[pos] == ']' {
            break;
        }
        if chars[pos] == '{' {
            let (obj, next) = extract_json_object(&chars, pos);
            if let Some(label) = extract_field_from_str(&obj, "label") {
                let kind_num = extract_field_from_str(&obj, "kind")
                    .and_then(|s| s.parse::<u32>().ok())
                    .map(lsp_completion_kind_to_sdk);
                let detail = extract_field_from_str(&obj, "detail");
                let insert = extract_field_from_str(&obj, "insertText")
                    .or_else(|| extract_field_from_str(&obj, "filterText"))
                    .unwrap_or_else(|| label.clone());
                let sort_text = extract_field_from_str(&obj, "sortText");
                let filter_text = extract_field_from_str(&obj, "filterText");
                items.push(CompletionItem {
                    label,
                    kind: kind_num,
                    detail,
                    documentation: None,
                    insert_text: Some(insert),
                    sort_text,
                    filter_text,
                });
            }
            pos = next;
        } else {
            pos += 1;
        }
        while pos < chars.len() && (chars[pos] == ',' || chars[pos].is_whitespace()) {
            pos += 1;
        }
    }

    if items.is_empty() {
        return None;
    }
    Some(CompletionList {
        items,
        is_incomplete: json.contains("\"isIncomplete\":true"),
    })
}

fn parse_lsp_hover(json: &str) -> Option<HoverResult> {
    // LSP hover: { "contents": { "kind": "markdown", "value": "..." } }
    // or { "contents": "..." } or { "result": { "contents": ... } }
    let value = extract_field_from_str(json, "value")
        .or_else(|| extract_field_from_str(json, "contents"))?;
    if value.is_empty() {
        return None;
    }
    Some(HoverResult {
        contents: vec![value],
        range: None,
    })
}

fn parse_lsp_locations(json: &str) -> Option<Vec<Location>> {
    let mut locs = Vec::new();
    let mut search = json;
    while let Some(uri_pos) = search.find("\"uri\":") {
        let after = &search[uri_pos + 6..];
        if let Some(uri) = extract_string_value(after) {
            let chunk = &search[uri_pos..];
            let start_line = find_nested_num(chunk, "start", "line").unwrap_or(0);
            let start_char = find_nested_num(chunk, "start", "character").unwrap_or(0);
            let end_line = find_nested_num(chunk, "end", "line").unwrap_or(start_line);
            let end_char = find_nested_num(chunk, "end", "character").unwrap_or(start_char);
            locs.push(Location {
                uri,
                range: Range {
                    start: Position {
                        line: start_line,
                        character: start_char,
                    },
                    end: Position {
                        line: end_line,
                        character: end_char,
                    },
                },
            });
            search = &search[uri_pos + 6..];
        } else {
            break;
        }
    }
    if locs.is_empty() {
        None
    } else {
        Some(locs)
    }
}

fn parse_lsp_symbols(json: &str) -> Option<Vec<DocumentSymbol>> {
    let mut symbols = Vec::new();
    let mut search = json;
    while let Some(name_pos) = search.find("\"name\":") {
        if let Some(name) = extract_string_value(&search[name_pos + 7..]) {
            let chunk = &search[name_pos..];
            let kind = extract_field_from_str(chunk, "kind")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(12);
            let detail = extract_field_from_str(chunk, "detail");
            let line = find_nested_num(chunk, "start", "line").unwrap_or(0);
            symbols.push(DocumentSymbol {
                name,
                detail,
                kind,
                range: Range {
                    start: Position { line, character: 0 },
                    end: Position { line, character: 0 },
                },
                selection_range: Range {
                    start: Position { line, character: 0 },
                    end: Position { line, character: 0 },
                },
            });
            search = &search[name_pos + 7..];
        } else {
            break;
        }
    }
    if symbols.is_empty() {
        None
    } else {
        Some(symbols)
    }
}

fn parse_lsp_text_edits(json: &str) -> Option<Vec<TextEdit>> {
    let mut edits = Vec::new();
    let mut search = json;
    while let Some(nt_pos) = search.find("\"newText\":") {
        if let Some(new_text) = extract_string_value(&search[nt_pos + 10..]) {
            let chunk = &search[nt_pos..];
            let start_line = find_nested_num(chunk, "start", "line").unwrap_or(0);
            let start_char = find_nested_num(chunk, "start", "character").unwrap_or(0);
            let end_line = find_nested_num(chunk, "end", "line").unwrap_or(start_line);
            let end_char = find_nested_num(chunk, "end", "character").unwrap_or(start_char);
            edits.push(TextEdit {
                range: Range {
                    start: Position {
                        line: start_line,
                        character: start_char,
                    },
                    end: Position {
                        line: end_line,
                        character: end_char,
                    },
                },
                new_text,
            });
            search = &search[nt_pos + 10..];
        } else {
            break;
        }
    }
    if edits.is_empty() {
        None
    } else {
        Some(edits)
    }
}

fn parse_lsp_diagnostics(json: &str) -> Option<Vec<Diagnostic>> {
    let mut diags = Vec::new();
    let mut search = json;
    while let Some(msg_pos) = search.find("\"message\":") {
        if let Some(message) = extract_string_value(&search[msg_pos + 10..]) {
            let chunk = &search[msg_pos..];
            let start_line = find_nested_num(chunk, "start", "line").unwrap_or(0);
            let start_char = find_nested_num(chunk, "start", "character").unwrap_or(0);
            let end_line = find_nested_num(chunk, "end", "line").unwrap_or(start_line);
            let end_char = find_nested_num(chunk, "end", "character").unwrap_or(start_char);
            let severity_num = extract_field_from_str(chunk, "severity")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(1);
            let severity = match severity_num {
                1 => DiagnosticSeverity::Error,
                2 => DiagnosticSeverity::Warning,
                3 => DiagnosticSeverity::Information,
                _ => DiagnosticSeverity::Hint,
            };
            let code = extract_field_from_str(chunk, "code");
            diags.push(Diagnostic {
                range: Range {
                    start: Position {
                        line: start_line,
                        character: start_char,
                    },
                    end: Position {
                        line: end_line,
                        character: end_char,
                    },
                },
                message,
                severity,
                source: Some("rust-analyzer".to_string()),
                code,
            });
            search = &search[msg_pos + 10..];
        } else {
            break;
        }
    }
    if diags.is_empty() {
        None
    } else {
        Some(diags)
    }
}

fn word_completions_from_doc(ctx: &DocumentContext, pos: Position) -> Option<CompletionList> {
    let text = host::get_document_text(&ctx.uri)?;
    let lines: Vec<&str> = text.lines().collect();
    let line_idx = pos.line as usize;
    if line_idx >= lines.len() {
        return None;
    }
    let line = lines[line_idx];
    let col = (pos.character as usize).min(line.len());

    let before = &line[..col];
    let prefix_start = before
        .rfind(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|i| i + 1)
        .unwrap_or(0);
    let prefix = &before[prefix_start..];

    if prefix.len() < 2 {
        return None;
    }

    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    for word in text.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if word.len() > prefix.len() && word.starts_with(prefix) && !seen.contains(word) {
            seen.insert(word.to_string());
            items.push(CompletionItem {
                label: word.to_string(),
                kind: Some(0),
                detail: None,
                documentation: None,
                insert_text: Some(word.to_string()),
                sort_text: Some(format!("9{word}")),
                filter_text: None,
            });
            if items.len() >= 50 {
                break;
            }
        }
    }

    if items.is_empty() {
        None
    } else {
        Some(CompletionList {
            items,
            is_incomplete: false,
        })
    }
}

fn extract_field_from_str(json: &str, field: &str) -> Option<String> {
    let key = format!("\"{}\":", field);
    let start = json.find(&key)? + key.len();
    let rest = json[start..].trim_start();
    if rest.starts_with('"') {
        extract_string_value(rest)
    } else {
        let end = rest
            .find(|c: char| c == ',' || c == '}' || c == ']' || c == '\n')
            .unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    }
}

fn extract_string_value(s: &str) -> Option<String> {
    let s = s.trim_start();
    if !s.starts_with('"') {
        return None;
    }
    let mut chars = s[1..].chars();
    let mut out = String::new();
    let mut escaped = false;
    loop {
        match chars.next()? {
            '\\' if !escaped => escaped = true,
            '"' if !escaped => break,
            c => {
                out.push(c);
                escaped = false;
            }
        }
    }
    Some(out)
}

fn extract_json_object(chars: &[char], start: usize) -> (String, usize) {
    let mut depth = 0i32;
    let mut end = start;
    for i in start..chars.len() {
        if chars[i] == '{' {
            depth += 1;
        }
        if chars[i] == '}' {
            depth -= 1;
            if depth == 0 {
                end = i + 1;
                break;
            }
        }
    }
    let s: String = chars[start..end].iter().collect();
    (s, end)
}

fn find_nested_num(json: &str, obj_key: &str, field: &str) -> Option<u32> {
    let obj_start = json.find(&format!("\"{}\"", obj_key))?;
    let brace = json[obj_start..].find('{')?;
    let sub = &json[obj_start + brace..];
    let close = sub.find('}')?;
    let inner = &sub[..close + 1];
    extract_field_from_str(inner, field)?.parse::<u32>().ok()
}

fn lsp_completion_kind_to_sdk(kind: u32) -> u32 {
    // LSP CompletionItemKind -> SDK kind (same numeric values in many cases)
    match kind {
        1 => 0,   // Text
        2 => 1,   // Method
        3 => 2,   // Function
        4 => 3,   // Constructor
        5 => 4,   // Field
        6 => 5,   // Variable
        7 => 6,   // Class
        8 => 7,   // Interface
        9 => 8,   // Module
        10 => 9,  // Property
        11 => 10, // Unit
        12 => 11, // Value
        13 => 12, // Enum
        14 => 13, // Keyword
        15 => 14, // Snippet
        22 => 24, // Struct
        _ => 0,
    }
}

sidex_extension_sdk::export_extension!(RustLanguageExtension);
