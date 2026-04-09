use sidex_extension_sdk::prelude::*;

struct HelloExtension;

impl SidexExtension for HelloExtension {
    fn activate() -> Result<(), String> {
        host::log_info("Hello extension activated!");
        host::show_info_message("Hello from a native Rust extension!");
        Ok(())
    }

    fn deactivate() {
        host::log_info("Hello extension deactivated.");
    }

    fn get_name() -> String {
        "Hello Extension".to_string()
    }

    fn get_activation_events() -> Vec<String> {
        vec!["*".to_string()]
    }

    fn get_commands() -> Vec<CommandDefinition> {
        vec![CommandDefinition {
            id: "hello.sayHello".to_string(),
            title: "Say Hello".to_string(),
        }]
    }

    fn get_semantic_tokens_legend() -> Option<SemanticTokensLegend> {
        None
    }

    fn provide_completion(
        _ctx: DocumentContext,
        _pos: Position,
    ) -> Option<CompletionList> {
        Some(CompletionList {
            items: vec![
                CompletionItem {
                    label: "hello_world".to_string(),
                    kind: Some(2),
                    detail: Some("A greeting from Rust".to_string()),
                    documentation: Some("This completion item comes from a native WASM extension written in Rust.".to_string()),
                    insert_text: Some("hello_world()".to_string()),
                    sort_text: None,
                    filter_text: None,
                },
            ],
            is_incomplete: false,
        })
    }

    fn provide_hover(_ctx: DocumentContext, _pos: Position) -> Option<HoverResult> {
        Some(HoverResult {
            contents: vec!["**Hello Extension**\n\nThis hover comes from a native Rust WASM extension.".to_string()],
            range: None,
        })
    }

    fn provide_definition(_ctx: DocumentContext, _pos: Position) -> Vec<Location> {
        vec![]
    }

    fn provide_type_definition(_ctx: DocumentContext, _pos: Position) -> Vec<Location> {
        vec![]
    }

    fn provide_implementation(_ctx: DocumentContext, _pos: Position) -> Vec<Location> {
        vec![]
    }

    fn provide_declaration(_ctx: DocumentContext, _pos: Position) -> Vec<Location> {
        vec![]
    }

    fn provide_references(_ctx: DocumentContext, _pos: Position) -> Vec<Location> {
        vec![]
    }

    fn provide_document_symbols(_ctx: DocumentContext) -> Vec<DocumentSymbol> {
        vec![]
    }

    fn provide_code_actions(
        _ctx: DocumentContext,
        _range: Range,
        _diagnostics: Vec<Diagnostic>,
    ) -> Vec<CodeAction> {
        vec![]
    }

    fn provide_code_lenses(_ctx: DocumentContext) -> Vec<CodeLens> {
        vec![]
    }

    fn provide_formatting(
        _ctx: DocumentContext,
        _tab_size: u32,
        _insert_spaces: bool,
    ) -> Vec<TextEdit> {
        vec![]
    }

    fn provide_range_formatting(
        _ctx: DocumentContext,
        _range: Range,
        _tab_size: u32,
        _insert_spaces: bool,
    ) -> Vec<TextEdit> {
        vec![]
    }

    fn provide_signature_help(
        _ctx: DocumentContext,
        _pos: Position,
    ) -> Option<SignatureHelpResult> {
        None
    }

    fn provide_document_highlights(
        _ctx: DocumentContext,
        _pos: Position,
    ) -> Vec<DocumentHighlight> {
        vec![]
    }

    fn provide_rename(
        _ctx: DocumentContext,
        _pos: Position,
        _new_name: String,
    ) -> Option<RenameResult> {
        None
    }

    fn prepare_rename(_ctx: DocumentContext, _pos: Position) -> Option<RenameLocation> {
        None
    }

    fn provide_folding_ranges(_ctx: DocumentContext) -> Vec<FoldingRange> {
        vec![]
    }

    fn provide_inlay_hints(_ctx: DocumentContext, _range: Range) -> Vec<InlayHint> {
        vec![]
    }

    fn provide_document_links(_ctx: DocumentContext) -> Vec<DocumentLink> {
        vec![]
    }

    fn provide_selection_ranges(
        _ctx: DocumentContext,
        _positions: Vec<Position>,
    ) -> Vec<SelectionRange> {
        vec![]
    }

    fn provide_semantic_tokens(_ctx: DocumentContext) -> Option<SemanticTokens> {
        None
    }

    fn provide_document_colors(_ctx: DocumentContext) -> Vec<ColorInfo> {
        vec![]
    }

    fn provide_workspace_symbols(_query: String) -> Vec<DocumentSymbol> {
        vec![]
    }

    fn execute_command(command_id: String, _args: String) -> Result<String, String> {
        if command_id == "hello.sayHello" {
            host::show_info_message("Hello from a native Rust extension command!");
            Ok("hello!".to_string())
        } else {
            Err(format!("unknown command: {command_id}"))
        }
    }

    fn on_file_event(_events: Vec<FileEvent>) {}

    fn on_configuration_changed(_section: String) {}

    fn get_tree_children(_view_id: String, _element_id: Option<String>) -> Vec<TreeItem> {
        vec![]
    }
}

sidex_extension_sdk::export_extension!(HelloExtension);
