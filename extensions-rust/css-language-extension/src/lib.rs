use sidex_extension_sdk::prelude::*;

pub struct CssLanguageExtension;

const SERVER_SCRIPT: &str = "extension-host/css-language-server/out/node/cssServerMain.js";

impl SidexExtension for CssLanguageExtension {
    fn activate() -> Result<(), String> {
        Ok(())
    }
    fn deactivate() {}
    fn get_name() -> String {
        "CSS Language Features".to_string()
    }
    fn get_activation_events() -> Vec<String> {
        vec![
            "onLanguage:css".into(),
            "onLanguage:less".into(),
            "onLanguage:scss".into(),
        ]
    }
    fn get_commands() -> Vec<CommandDefinition> {
        vec![]
    }

    fn provide_completion(ctx: DocumentContext, pos: Position) -> Option<CompletionList> {
        if !is_css_lang(&ctx.language_id) {
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
        if !is_css_lang(&ctx.language_id) {
            return None;
        }
        let resp = lsp_request("textDocument/hover", &ctx, Some(pos), "")?;
        parse_lsp_hover(&resp)
    }

    fn provide_definition(ctx: DocumentContext, pos: Position) -> Vec<Location> {
        if !is_css_lang(&ctx.language_id) {
            return vec![];
        }
        lsp_request("textDocument/definition", &ctx, Some(pos), "")
            .and_then(|r| parse_lsp_locations(&r))
            .unwrap_or_default()
    }

    fn provide_references(ctx: DocumentContext, pos: Position) -> Vec<Location> {
        if !is_css_lang(&ctx.language_id) {
            return vec![];
        }
        let extra = r#","context":{"includeDeclaration":true}"#;
        lsp_request("textDocument/references", &ctx, Some(pos), extra)
            .and_then(|r| parse_lsp_locations(&r))
            .unwrap_or_default()
    }

    fn provide_document_symbols(ctx: DocumentContext) -> Vec<DocumentSymbol> {
        if !is_css_lang(&ctx.language_id) {
            return vec![];
        }
        lsp_request("textDocument/documentSymbol", &ctx, None, "")
            .and_then(|r| parse_lsp_symbols(&r))
            .unwrap_or_default()
    }

    fn provide_document_highlights(ctx: DocumentContext, pos: Position) -> Vec<DocumentHighlight> {
        if !is_css_lang(&ctx.language_id) {
            return vec![];
        }
        lsp_request("textDocument/documentHighlight", &ctx, Some(pos), "")
            .and_then(|r| parse_lsp_highlights(&r))
            .unwrap_or_default()
    }

    fn provide_document_colors(ctx: DocumentContext) -> Vec<ColorInfo> {
        if !is_css_lang(&ctx.language_id) {
            return vec![];
        }
        lsp_request("textDocument/documentColor", &ctx, None, "")
            .and_then(|r| parse_lsp_colors(&r))
            .unwrap_or_default()
    }

    fn provide_formatting(
        ctx: DocumentContext,
        tab_size: u32,
        insert_spaces: bool,
    ) -> Vec<TextEdit> {
        if !is_css_lang(&ctx.language_id) {
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

    fn provide_range_formatting(
        ctx: DocumentContext,
        range: Range,
        tab_size: u32,
        insert_spaces: bool,
    ) -> Vec<TextEdit> {
        if !is_css_lang(&ctx.language_id) {
            return vec![];
        }
        let extra = format!(
            r#","range":{{"start":{{"line":{},"character":{}}},"end":{{"line":{},"character":{}}}}},"options":{{"tabSize":{},"insertSpaces":{}}}"#,
            range.start.line,
            range.start.character,
            range.end.line,
            range.end.character,
            tab_size,
            insert_spaces
        );
        lsp_request("textDocument/rangeFormatting", &ctx, None, &extra)
            .and_then(|r| parse_lsp_text_edits(&r))
            .unwrap_or_default()
    }

    fn provide_folding_ranges(ctx: DocumentContext) -> Vec<FoldingRange> {
        if !is_css_lang(&ctx.language_id) {
            return vec![];
        }
        lsp_request("textDocument/foldingRange", &ctx, None, "")
            .and_then(|r| parse_lsp_folding(&r))
            .unwrap_or_default()
    }

    fn provide_selection_ranges(
        ctx: DocumentContext,
        positions: Vec<Position>,
    ) -> Vec<SelectionRange> {
        if !is_css_lang(&ctx.language_id) {
            return vec![];
        }
        let pos_arr: Vec<String> = positions
            .iter()
            .map(|p| format!(r#"{{"line":{},"character":{}}}"#, p.line, p.character))
            .collect();
        let extra = format!(r#","positions":[{}]"#, pos_arr.join(","));
        lsp_request("textDocument/selectionRange", &ctx, None, &extra)
            .and_then(|r| parse_lsp_selection_ranges(&r))
            .unwrap_or_default()
    }

    fn provide_code_actions(
        ctx: DocumentContext,
        range: Range,
        diagnostics: Vec<Diagnostic>,
    ) -> Vec<CodeAction> {
        if !is_css_lang(&ctx.language_id) {
            return vec![];
        }
        let diag_json: Vec<String> = diagnostics.iter().map(|d| format!(
            r#"{{"range":{{"start":{{"line":{},"character":{}}},"end":{{"line":{},"character":{}}}}},"message":"{}","severity":{}}}"#,
            d.range.start.line, d.range.start.character, d.range.end.line, d.range.end.character,
            d.message.replace('"', "\\\""),
            match d.severity { DiagnosticSeverity::Error => 1, DiagnosticSeverity::Warning => 2, DiagnosticSeverity::Information => 3, _ => 4 }
        )).collect();
        let extra = format!(
            r#","range":{{"start":{{"line":{},"character":{}}},"end":{{"line":{},"character":{}}}}},"context":{{"diagnostics":[{}]}}"#,
            range.start.line,
            range.start.character,
            range.end.line,
            range.end.character,
            diag_json.join(",")
        );
        lsp_request("textDocument/codeAction", &ctx, None, &extra)
            .and_then(|r| parse_lsp_code_actions(&r))
            .unwrap_or_default()
    }

    fn provide_rename(
        ctx: DocumentContext,
        pos: Position,
        new_name: String,
    ) -> Option<RenameResult> {
        if !is_css_lang(&ctx.language_id) {
            return None;
        }
        let extra = format!(r#","newName":"{}""#, new_name.replace('"', "\\\""));
        let resp = lsp_request("textDocument/rename", &ctx, Some(pos), &extra)?;
        parse_lsp_rename(&resp)
    }

    fn provide_document_links(ctx: DocumentContext) -> Vec<DocumentLink> {
        if !is_css_lang(&ctx.language_id) {
            return vec![];
        }
        lsp_request("textDocument/documentLink", &ctx, None, "")
            .and_then(|r| parse_lsp_doc_links(&r))
            .unwrap_or_default()
    }

    fn on_file_event(events: Vec<FileEvent>) {
        for event in events {
            if event.uri.ends_with(".css")
                || event.uri.ends_with(".scss")
                || event.uri.ends_with(".less")
            {
                let ctx = DocumentContext {
                    uri: event.uri.clone(),
                    language_id: "css".into(),
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
    fn provide_code_lenses(_: DocumentContext) -> Vec<CodeLens> {
        vec![]
    }
    fn provide_signature_help(_: DocumentContext, _: Position) -> Option<SignatureHelpResult> {
        None
    }
    fn prepare_rename(_: DocumentContext, _: Position) -> Option<RenameLocation> {
        None
    }
    fn provide_inlay_hints(_: DocumentContext, _: Range) -> Vec<InlayHint> {
        vec![]
    }
    fn provide_semantic_tokens(_: DocumentContext) -> Option<SemanticTokens> {
        None
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

fn is_css_lang(lang: &str) -> bool {
    matches!(lang, "css" | "scss" | "less")
}

fn server_path() -> String {
    if let Ok(p) = host::execute_command("__sidex.resolve_path", SERVER_SCRIPT) {
        if !p.is_empty() {
            return p;
        }
    }
    SERVER_SCRIPT.to_string()
}

fn lsp_request(
    method: &str,
    ctx: &DocumentContext,
    pos: Option<Position>,
    extra: &str,
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
    let sp = server_path();
    let payload = format!(
        r#"{{"server":"css-language-server","cmd":"node","args":["{sp}","--stdio"],"method":"{method}","params":{{"textDocument":{{"uri":"{uri}"}}{pos_json}{extra}}}}}"#
    );
    host::execute_command("__sidex.lsp", &payload).ok()
}

fn parse_lsp_completions(json: &str) -> Option<CompletionList> {
    let items_start = json.find("\"items\"")?;
    let arr_start = json[items_start..].find('[')? + items_start;
    let mut items = Vec::new();
    let chars: Vec<char> = json.chars().collect();
    let mut pos = arr_start + 1;
    while pos < chars.len() && items.len() < 200 {
        while pos < chars.len() && chars[pos].is_whitespace() {
            pos += 1;
        }
        if pos >= chars.len() || chars[pos] == ']' {
            break;
        }
        if chars[pos] == '{' {
            let (obj, next) = extract_json_obj_chars(&chars, pos);
            if let Some(label) = extract_field(&obj, "label") {
                let kind_num = extract_field(&obj, "kind")
                    .and_then(|s| s.parse::<u32>().ok())
                    .map(lsp_kind);
                items.push(CompletionItem {
                    label: label.clone(),
                    kind: kind_num,
                    detail: extract_field(&obj, "detail"),
                    documentation: extract_field(&obj, "documentation")
                        .or_else(|| extract_nested_field(&obj, "documentation", "value")),
                    insert_text: extract_field(&obj, "insertText")
                        .or_else(|| extract_field(&obj, "filterText"))
                        .unwrap_or_else(|| label.clone())
                        .into(),
                    sort_text: extract_field(&obj, "sortText"),
                    filter_text: extract_field(&obj, "filterText"),
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
        None
    } else {
        Some(CompletionList {
            items,
            is_incomplete: json.contains("\"isIncomplete\":true"),
        })
    }
}

fn parse_lsp_hover(json: &str) -> Option<HoverResult> {
    let search = "\"value\":";
    let val_pos = json.find(search)?;
    let rest = json[val_pos + search.len()..].trim_start();

    if !rest.starts_with('"') {
        return None;
    }

    let mut chars = rest[1..].chars();
    let mut value = String::new();
    loop {
        match chars.next() {
            None => break,
            Some('\\') => match chars.next() {
                Some('n') => value.push('\n'),
                Some('r') => value.push('\r'),
                Some('t') => value.push('\t'),
                Some('"') => value.push('"'),
                Some('\\') => value.push('\\'),
                Some('/') => value.push('/'),
                Some('u') => {
                    let mut hex = String::new();
                    for _ in 0..4 {
                        if let Some(c) = chars.next() {
                            hex.push(c);
                        }
                    }
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(code) {
                            value.push(ch);
                        }
                    }
                }
                Some(c) => {
                    value.push('\\');
                    value.push(c);
                }
                None => break,
            },
            Some('"') => break,
            Some(c) => value.push(c),
        }
    }

    if value.is_empty() {
        return None;
    }

    let range = if let Some(rp) = json.find("\"range\":") {
        let chunk = &json[rp..];
        let sl = nested_num(chunk, "start", "line");
        let sc = nested_num(chunk, "start", "character");
        let el = nested_num(chunk, "end", "line");
        let ec = nested_num(chunk, "end", "character");
        if let (Some(sl), Some(sc), Some(el), Some(ec)) = (sl, sc, el, ec) {
            Some(Range {
                start: Position {
                    line: sl,
                    character: sc,
                },
                end: Position {
                    line: el,
                    character: ec,
                },
            })
        } else {
            None
        }
    } else {
        None
    };

    Some(HoverResult {
        contents: vec![value],
        range,
    })
}

fn parse_lsp_locations(json: &str) -> Option<Vec<Location>> {
    let mut locs = Vec::new();
    let mut s = json;
    while let Some(up) = s.find("\"uri\":") {
        let after = &s[up + 6..];
        if let Some(uri) = extract_str_val(after) {
            let chunk = &s[up..];
            locs.push(Location {
                uri,
                range: Range {
                    start: Position {
                        line: nested_num(chunk, "start", "line").unwrap_or(0),
                        character: nested_num(chunk, "start", "character").unwrap_or(0),
                    },
                    end: Position {
                        line: nested_num(chunk, "end", "line").unwrap_or(0),
                        character: nested_num(chunk, "end", "character").unwrap_or(0),
                    },
                },
            });
            s = &s[up + 6..];
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
    let mut syms = Vec::new();
    let mut s = json;
    while let Some(np) = s.find("\"name\":") {
        if let Some(name) = extract_str_val(&s[np + 7..]) {
            let chunk = &s[np..];
            let kind = extract_field(chunk, "kind")
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(12);
            let line = nested_num(chunk, "start", "line").unwrap_or(0);
            syms.push(DocumentSymbol {
                name,
                detail: extract_field(chunk, "detail"),
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
            s = &s[np + 7..];
        } else {
            break;
        }
    }
    if syms.is_empty() {
        None
    } else {
        Some(syms)
    }
}

fn parse_lsp_text_edits(json: &str) -> Option<Vec<TextEdit>> {
    let mut edits = Vec::new();
    let mut s = json;
    while let Some(np) = s.find("\"newText\":") {
        if let Some(new_text) = extract_str_val(&s[np + 10..]) {
            let chunk = &s[np..];
            edits.push(TextEdit {
                range: Range {
                    start: Position {
                        line: nested_num(chunk, "start", "line").unwrap_or(0),
                        character: nested_num(chunk, "start", "character").unwrap_or(0),
                    },
                    end: Position {
                        line: nested_num(chunk, "end", "line").unwrap_or(0),
                        character: nested_num(chunk, "end", "character").unwrap_or(0),
                    },
                },
                new_text,
            });
            s = &s[np + 10..];
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
    let mut s = json;
    while let Some(mp) = s.find("\"message\":") {
        if let Some(message) = extract_str_val(&s[mp + 10..]) {
            let chunk = &s[mp..];
            let sev = extract_field(chunk, "severity")
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(1);
            diags.push(Diagnostic {
                range: Range {
                    start: Position {
                        line: nested_num(chunk, "start", "line").unwrap_or(0),
                        character: nested_num(chunk, "start", "character").unwrap_or(0),
                    },
                    end: Position {
                        line: nested_num(chunk, "end", "line").unwrap_or(0),
                        character: nested_num(chunk, "end", "character").unwrap_or(0),
                    },
                },
                message,
                code: extract_field(chunk, "code"),
                severity: match sev {
                    1 => DiagnosticSeverity::Error,
                    2 => DiagnosticSeverity::Warning,
                    3 => DiagnosticSeverity::Information,
                    _ => DiagnosticSeverity::Hint,
                },
                source: Some("css-language-server".into()),
            });
            s = &s[mp + 10..];
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

fn parse_lsp_highlights(json: &str) -> Option<Vec<DocumentHighlight>> {
    let mut hl = Vec::new();
    let mut s = json;
    while let Some(rp) = s.find("\"range\":") {
        let chunk = &s[rp..];
        let kind = extract_field(chunk, "kind")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        hl.push(DocumentHighlight {
            range: Range {
                start: Position {
                    line: nested_num(chunk, "start", "line").unwrap_or(0),
                    character: nested_num(chunk, "start", "character").unwrap_or(0),
                },
                end: Position {
                    line: nested_num(chunk, "end", "line").unwrap_or(0),
                    character: nested_num(chunk, "end", "character").unwrap_or(0),
                },
            },
            kind,
        });
        s = &s[rp + 8..];
    }
    if hl.is_empty() {
        None
    } else {
        Some(hl)
    }
}

fn parse_lsp_colors(json: &str) -> Option<Vec<ColorInfo>> {
    let mut colors = Vec::new();
    let mut s = json;
    while let Some(cp) = s.find("\"color\":") {
        let chunk = &s[cp..];
        let red = extract_field(chunk, "red")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        let green = extract_field(chunk, "green")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        let blue = extract_field(chunk, "blue")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0);
        let alpha = extract_field(chunk, "alpha")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(1.0);
        let rng_chunk = &s[s[..cp + 8].rfind("\"range\":").unwrap_or(cp)..];
        colors.push(ColorInfo {
            range: Range {
                start: Position {
                    line: nested_num(rng_chunk, "start", "line").unwrap_or(0),
                    character: nested_num(rng_chunk, "start", "character").unwrap_or(0),
                },
                end: Position {
                    line: nested_num(rng_chunk, "end", "line").unwrap_or(0),
                    character: nested_num(rng_chunk, "end", "character").unwrap_or(0),
                },
            },
            red,
            green,
            blue,
            alpha,
        });
        s = &s[cp + 8..];
    }
    if colors.is_empty() {
        None
    } else {
        Some(colors)
    }
}

fn parse_lsp_folding(json: &str) -> Option<Vec<FoldingRange>> {
    let mut ranges = Vec::new();
    let mut s = json;
    while let Some(sp) = s.find("\"startLine\":") {
        let chunk = &s[sp..];
        let start_line = extract_field(chunk, "startLine")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(0);
        let end_line = extract_field(chunk, "endLine")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(start_line);
        ranges.push(FoldingRange {
            start_line,
            end_line,
            kind: None,
        });
        s = &s[sp + 12..];
    }
    if ranges.is_empty() {
        None
    } else {
        Some(ranges)
    }
}

fn parse_lsp_selection_ranges(_json: &str) -> Option<Vec<SelectionRange>> {
    None
}
fn parse_lsp_code_actions(_json: &str) -> Option<Vec<CodeAction>> {
    None
}
fn parse_lsp_rename(_json: &str) -> Option<RenameResult> {
    None
}
fn parse_lsp_doc_links(_json: &str) -> Option<Vec<DocumentLink>> {
    None
}

fn word_completions_from_doc(ctx: &DocumentContext, pos: Position) -> Option<CompletionList> {
    let text = host::get_document_text(&ctx.uri).or_else(|| host::read_file(&ctx.uri).ok())?;
    let lines: Vec<&str> = text.lines().collect();
    let line = lines.get(pos.line as usize)?;
    let col = (pos.character as usize).min(line.len());
    let before = &line[..col];
    let ps = before
        .rfind(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .map(|i| i + 1)
        .unwrap_or(0);
    let prefix = &before[ps..];
    if prefix.len() < 2 {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();
    for word in text.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_') {
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

fn extract_field(json: &str, field: &str) -> Option<String> {
    let key = format!("\"{}\":", field);
    let start = json.find(&key)? + key.len();
    let rest = json[start..].trim_start();
    if rest.starts_with('"') {
        extract_str_val(rest)
    } else {
        let end = rest
            .find(|c: char| c == ',' || c == '}' || c == ']' || c == '\n')
            .unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    }
}

fn extract_nested_field(json: &str, outer: &str, inner: &str) -> Option<String> {
    let key = format!("\"{}\":", outer);
    let start = json.find(&key)? + key.len();
    let rest = json[start..].trim_start();
    if rest.starts_with('{') {
        let close = rest.find('}')?;
        extract_field(&rest[..close + 1], inner)
    } else {
        None
    }
}

fn extract_str_val(s: &str) -> Option<String> {
    let s = s.trim_start();
    if !s.starts_with('"') {
        return None;
    }
    let mut chars = s[1..].chars();
    let mut out = String::new();
    loop {
        match chars.next()? {
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                c => {
                    out.push('\\');
                    out.push(c);
                }
            },
            '"' => break,
            c => out.push(c),
        }
    }
    Some(out)
}

fn extract_json_obj_chars(chars: &[char], start: usize) -> (String, usize) {
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
    (chars[start..end].iter().collect(), end)
}

fn nested_num(json: &str, obj: &str, field: &str) -> Option<u32> {
    let os = json.find(&format!("\"{}\"", obj))?;
    let b = json[os..].find('{')?;
    let sub = &json[os + b..];
    let c = sub.find('}')?;
    extract_field(&sub[..c + 1], field)?.parse::<u32>().ok()
}

fn lsp_kind(kind: u32) -> u32 {
    match kind {
        1 => 0,
        2 => 1,
        3 => 2,
        4 => 3,
        5 => 4,
        6 => 5,
        7 => 6,
        8 => 7,
        9 => 8,
        10 => 9,
        11 => 10,
        12 => 11,
        13 => 12,
        14 => 13,
        15 => 14,
        16 => 15,
        22 => 24,
        _ => 0,
    }
}

sidex_extension_sdk::export_extension!(CssLanguageExtension);
