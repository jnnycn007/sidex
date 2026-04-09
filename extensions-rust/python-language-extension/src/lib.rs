use sidex_extension_sdk::prelude::*;

/// Python language features via pyright LSP.
/// Provides: completion, hover, definition, references, document symbols, formatting.
pub struct PythonLanguageExtension;

const SERVER: &str = "pyright-langserver";
const CMD: &str = "pyright-langserver";

impl SidexExtension for PythonLanguageExtension {
    fn activate() -> Result<(), String> {
        Ok(())
    }

    fn deactivate() {}

    fn get_name() -> String {
        "Python Language Features".to_string()
    }

    fn get_activation_events() -> Vec<String> {
        vec!["onLanguage:python".to_string()]
    }

    fn get_commands() -> Vec<CommandDefinition> {
        vec![]
    }

    fn provide_completion(ctx: DocumentContext, pos: Position) -> Option<CompletionList> {
        if ctx.language_id != "python" {
            return None;
        }
        if let Some(resp) = lsp_request(SERVER, CMD, "textDocument/completion", &ctx, Some(pos)) {
            if let Some(list) = parse_lsp_completions(&resp) {
                return Some(list);
            }
        }
        word_completions_from_doc(&ctx, pos)
    }

    fn provide_hover(ctx: DocumentContext, pos: Position) -> Option<HoverResult> {
        if ctx.language_id != "python" {
            return None;
        }
        let resp = lsp_request(SERVER, CMD, "textDocument/hover", &ctx, Some(pos))?;
        parse_lsp_hover(&resp)
    }

    fn provide_definition(ctx: DocumentContext, pos: Position) -> Vec<Location> {
        if ctx.language_id != "python" {
            return vec![];
        }
        lsp_request(SERVER, CMD, "textDocument/definition", &ctx, Some(pos))
            .and_then(|r| parse_lsp_locations(&r))
            .unwrap_or_default()
    }

    fn provide_references(ctx: DocumentContext, pos: Position) -> Vec<Location> {
        if ctx.language_id != "python" {
            return vec![];
        }
        lsp_request(SERVER, CMD, "textDocument/references", &ctx, Some(pos))
            .and_then(|r| parse_lsp_locations(&r))
            .unwrap_or_default()
    }

    fn provide_document_symbols(ctx: DocumentContext) -> Vec<DocumentSymbol> {
        if ctx.language_id != "python" {
            return vec![];
        }
        lsp_request(SERVER, CMD, "textDocument/documentSymbol", &ctx, None)
            .and_then(|r| parse_lsp_symbols(&r))
            .unwrap_or_default()
    }

    fn provide_formatting(
        ctx: DocumentContext,
        tab_size: u32,
        insert_spaces: bool,
    ) -> Vec<TextEdit> {
        if ctx.language_id != "python" {
            return vec![];
        }
        let extra_params = format!(
            r#","options":{{"tabSize":{},"insertSpaces":{}}}"#,
            tab_size, insert_spaces
        );
        let uri = &ctx.uri;
        let payload = format!(
            r#"{{"server":"{SERVER}","cmd":"{CMD}","args":["--stdio"],"method":"textDocument/formatting","params":{{"textDocument":{{"uri":"{uri}"}}{extra_params}}}}}"#
        );
        host::execute_command("__sidex.lsp", &payload)
            .ok()
            .and_then(|r| parse_lsp_text_edits(&r))
            .unwrap_or_default()
    }

    fn on_file_event(_events: Vec<FileEvent>) {}

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
    fn execute_command(id: String, _: String) -> Result<String, String> {
        Err(format!("unknown: {id}"))
    }
    fn on_configuration_changed(_: String) {}
    fn get_tree_children(_: String, _: Option<String>) -> Vec<TreeItem> {
        vec![]
    }
}

fn lsp_request(
    server: &str,
    cmd: &str,
    method: &str,
    ctx: &DocumentContext,
    pos: Option<Position>,
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
        r#"{{"server":"{server}","cmd":"{cmd}","args":["--stdio"],"method":"{method}","params":{{"textDocument":{{"uri":"{uri}"}}{pos_json}}}}}"#
    );
    host::execute_command("__sidex.lsp", &payload).ok()
}

fn parse_lsp_completions(json: &str) -> Option<CompletionList> {
    let items_start = find_items_array(json)?;
    let mut items = Vec::new();
    let chars: Vec<char> = json.chars().collect();
    let mut pos = items_start + 1;

    while pos < chars.len() {
        skip_ws(&chars, &mut pos);
        if pos >= chars.len() || chars[pos] == ']' {
            break;
        }
        if chars[pos] == '{' {
            let (obj, next) = extract_json_object(&chars, pos);
            let label = extract_field_from_str(&obj, "label").unwrap_or_default();
            if !label.is_empty() {
                let kind_num = extract_field_from_str(&obj, "kind")
                    .and_then(|s| s.parse::<u32>().ok())
                    .map(lsp_completion_kind);
                let detail = extract_field_from_str(&obj, "detail");
                let insert_text = extract_field_from_str(&obj, "insertText")
                    .or_else(|| extract_text_edit_new_text(&obj))
                    .unwrap_or_else(|| label.clone());
                items.push(CompletionItem {
                    label,
                    kind: kind_num,
                    detail,
                    documentation: extract_field_from_str(&obj, "documentation"),
                    insert_text: Some(insert_text),
                    sort_text: extract_field_from_str(&obj, "sortText"),
                    filter_text: extract_field_from_str(&obj, "filterText"),
                });
            }
            pos = next;
        } else {
            pos += 1;
        }
        skip_comma_ws(&chars, &mut pos);
    }

    if items.is_empty() {
        return None;
    }
    Some(CompletionList {
        items,
        is_incomplete: false,
    })
}

fn parse_lsp_hover(json: &str) -> Option<HoverResult> {
    let result_start = json.find("\"result\"")?;
    let rest = &json[result_start..];
    let contents_key = rest.find("\"contents\"")?;
    let after = &rest[contents_key + 10..].trim_start();
    let after = after.strip_prefix(':')?;
    let after = after.trim_start();

    let value = if after.starts_with('"') {
        extract_string_value(after)?
    } else if after.starts_with('{') {
        extract_field_from_str(after, "value")?
    } else if after.starts_with('[') {
        let (arr_content, _) = extract_json_array_str(after);
        extract_field_from_str(&arr_content, "value")
            .or_else(|| extract_first_string_in_array(&arr_content))?
    } else {
        return None;
    };

    Some(HoverResult {
        contents: vec![value],
        range: None,
    })
}

fn parse_lsp_locations(json: &str) -> Option<Vec<Location>> {
    let result_start = json.find("\"result\"")?;
    let rest = &json[result_start..];
    let arr_start = rest.find('[')?;
    let chars: Vec<char> = rest.chars().collect();
    let mut pos = arr_start + 1;
    let mut locs = Vec::new();

    while pos < chars.len() {
        skip_ws(&chars, &mut pos);
        if pos >= chars.len() || chars[pos] == ']' {
            break;
        }
        if chars[pos] == '{' {
            let (obj, next) = extract_json_object(&chars, pos);
            if let Some(uri) = extract_field_from_str(&obj, "uri") {
                let (start_line, start_char, end_line, end_char) = extract_range_fields(&obj);
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
            }
            pos = next;
        } else {
            pos += 1;
        }
        skip_comma_ws(&chars, &mut pos);
    }

    if locs.is_empty() {
        None
    } else {
        Some(locs)
    }
}

fn parse_lsp_symbols(json: &str) -> Option<Vec<DocumentSymbol>> {
    let result_start = json.find("\"result\"")?;
    let rest = &json[result_start..];
    let arr_start = rest.find('[')?;
    let chars: Vec<char> = rest.chars().collect();
    let mut pos = arr_start + 1;
    let mut symbols = Vec::new();

    while pos < chars.len() {
        skip_ws(&chars, &mut pos);
        if pos >= chars.len() || chars[pos] == ']' {
            break;
        }
        if chars[pos] == '{' {
            let (obj, next) = extract_json_object(&chars, pos);
            if let Some(name) = extract_field_from_str(&obj, "name") {
                let kind = extract_field_from_str(&obj, "kind")
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(13);
                let (start_line, start_char, end_line, end_char) = extract_range_fields(&obj);
                let range = Range {
                    start: Position {
                        line: start_line,
                        character: start_char,
                    },
                    end: Position {
                        line: end_line,
                        character: end_char,
                    },
                };
                symbols.push(DocumentSymbol {
                    name,
                    detail: extract_field_from_str(&obj, "detail"),
                    kind,
                    range,
                    selection_range: range,
                });
            }
            pos = next;
        } else {
            pos += 1;
        }
        skip_comma_ws(&chars, &mut pos);
    }

    if symbols.is_empty() {
        None
    } else {
        Some(symbols)
    }
}

fn parse_lsp_text_edits(json: &str) -> Option<Vec<TextEdit>> {
    let result_start = json.find("\"result\"")?;
    let rest = &json[result_start..];
    let arr_start = rest.find('[')?;
    let chars: Vec<char> = rest.chars().collect();
    let mut pos = arr_start + 1;
    let mut edits = Vec::new();

    while pos < chars.len() {
        skip_ws(&chars, &mut pos);
        if pos >= chars.len() || chars[pos] == ']' {
            break;
        }
        if chars[pos] == '{' {
            let (obj, next) = extract_json_object(&chars, pos);
            if let Some(new_text) = extract_field_from_str(&obj, "newText") {
                let (start_line, start_char, end_line, end_char) = extract_range_fields(&obj);
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
            }
            pos = next;
        } else {
            pos += 1;
        }
        skip_comma_ws(&chars, &mut pos);
    }

    if edits.is_empty() {
        None
    } else {
        Some(edits)
    }
}

fn word_completions_from_doc(ctx: &DocumentContext, pos: Position) -> Option<CompletionList> {
    let text = host::get_document_text(&ctx.uri).or_else(|| host::read_file(&ctx.uri).ok())?;
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
        if word.len() > prefix.len() && word.starts_with(prefix) && seen.insert(word.to_string()) {
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

fn find_items_array(json: &str) -> Option<usize> {
    let result_pos = json.find("\"result\"")?;
    let rest = &json[result_pos..];
    if let Some(items_pos) = rest.find("\"items\"") {
        let after_items = &rest[items_pos..];
        after_items.find('[').map(|i| result_pos + items_pos + i)
    } else {
        rest.find('[').map(|i| result_pos + i)
    }
}

fn extract_json_object(chars: &[char], start: usize) -> (String, usize) {
    let mut depth = 0i32;
    let mut end = start;
    for i in start..chars.len() {
        match chars[i] {
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
    let s: String = chars[start..end].iter().collect();
    (s, end)
}

fn extract_field_from_str(json: &str, field: &str) -> Option<String> {
    let key = format!("\"{}\"", field);
    let key_pos = json.find(&key)?;
    let after_key = &json[key_pos + key.len()..];
    let colon_pos = after_key.find(':')?;
    let rest = after_key[colon_pos + 1..].trim_start();
    if rest.starts_with('"') {
        extract_string_value(rest)
    } else {
        let end = rest
            .find(|c: char| c == ',' || c == '}' || c == ']' || c == '\n')
            .unwrap_or(rest.len());
        let val = rest[..end].trim();
        if val.is_empty() || val == "null" {
            None
        } else {
            Some(val.to_string())
        }
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

fn extract_text_edit_new_text(obj: &str) -> Option<String> {
    let te_pos = obj.find("\"textEdit\"")?;
    let rest = &obj[te_pos..];
    extract_field_from_str(rest, "newText")
}

fn extract_range_fields(obj: &str) -> (u32, u32, u32, u32) {
    let range_str = obj.find("\"range\"").and_then(|p| {
        let rest = &obj[p..];
        let brace = rest.find('{')?;
        let (inner, _) = extract_json_object(&rest.chars().collect::<Vec<_>>(), brace);
        Some(inner)
    });
    let range_str = range_str.unwrap_or_default();

    let start_pos = range_str.find("\"start\"").and_then(|p| {
        let rest = &range_str[p..];
        let brace = rest.find('{')?;
        let (inner, _) = extract_json_object(&rest.chars().collect::<Vec<_>>(), brace);
        Some(inner)
    });
    let end_pos = range_str.find("\"end\"").and_then(|p| {
        let rest = &range_str[p..];
        let brace = rest.find('{')?;
        let (inner, _) = extract_json_object(&rest.chars().collect::<Vec<_>>(), brace);
        Some(inner)
    });

    let sl = start_pos
        .as_deref()
        .and_then(|s| extract_field_from_str(s, "line"))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let sc = start_pos
        .as_deref()
        .and_then(|s| extract_field_from_str(s, "character"))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let el = end_pos
        .as_deref()
        .and_then(|s| extract_field_from_str(s, "line"))
        .and_then(|s| s.parse().ok())
        .unwrap_or(sl);
    let ec = end_pos
        .as_deref()
        .and_then(|s| extract_field_from_str(s, "character"))
        .and_then(|s| s.parse().ok())
        .unwrap_or(sc);

    (sl, sc, el, ec)
}

fn extract_json_array_str(s: &str) -> (String, usize) {
    let chars: Vec<char> = s.chars().collect();
    let mut depth = 0i32;
    let mut end = 0;
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    let content: String = chars[..end].iter().collect();
    (content, end)
}

fn extract_first_string_in_array(arr: &str) -> Option<String> {
    let start = arr.find('"')?;
    extract_string_value(&arr[start..])
}

fn skip_ws(chars: &[char], pos: &mut usize) {
    while *pos < chars.len() && chars[*pos].is_whitespace() {
        *pos += 1;
    }
}

fn skip_comma_ws(chars: &[char], pos: &mut usize) {
    while *pos < chars.len() && (chars[*pos] == ',' || chars[*pos].is_whitespace()) {
        *pos += 1;
    }
}

fn lsp_completion_kind(kind: u32) -> u32 {
    kind.min(25)
}

sidex_extension_sdk::export_extension!(PythonLanguageExtension);
