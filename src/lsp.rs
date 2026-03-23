use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

struct DocumentState {
    source: String,
    type_map: HashMap<(usize, usize), crate::type_repr::Type>,
    type_registry: crate::types::TypeRegistry,
    definitions: Vec<crate::ast::Definition>,
    source_len: usize,
}

struct GlassLsp {
    client: Client,
    documents: Arc<RwLock<HashMap<Url, DocumentState>>>,
}

impl GlassLsp {
    async fn analyze(&self, uri: &Url) {
        let source = {
            let docs = self.documents.read().await;
            match docs.get(uri) {
                Some(state) => state.source.clone(),
                None => return,
            }
        };
        let source_len = source.len();
        let tokens = match crate::token::Lexer::tokenize(&source) {
            Ok(t) => t,
            Err(e) => {
                let diag = Diagnostic {
                    range: span_to_range(&source, e.span),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: format!("unexpected character: {:?}", e.text),
                    ..Default::default()
                };
                self.client
                    .publish_diagnostics(uri.clone(), vec![diag], None)
                    .await;
                return;
            }
        };
        let mut parser = crate::parser::Parser::new(tokens);
        let module = match parser.parse_module() {
            Ok(m) => m,
            Err(e) => {
                let diag = Diagnostic {
                    range: span_to_range(&source, e.span),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: e.message,
                    ..Default::default()
                };
                self.client
                    .publish_diagnostics(uri.clone(), vec![diag], None)
                    .await;
                return;
            }
        };

        let input_path = uri.to_file_path().unwrap_or_default();
        let mut resolver = crate::modules::ModuleResolver::new(&input_path);
        let (resolved_module, imports, _imported_count, def_module_map) =
            match resolver.resolve_module(&module) {
                Ok(r) => r,
                Err(_) => {
                    self.client
                        .publish_diagnostics(uri.clone(), vec![], None)
                        .await;
                    return;
                }
            };

        let mut inferencer = crate::infer::Inferencer::new();
        let infer_result =
            inferencer.infer_module_with_imports(&resolved_module, &imports, &def_module_map);

        let mut diagnostics = Vec::new();
        for err in &infer_result.errors {
            if err.span.start < source_len && err.span.end <= source_len {
                diagnostics.push(Diagnostic {
                    range: span_to_range(&source, err.span),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: err.message.clone(),
                    ..Default::default()
                });
            }
        }

        let linearity_result =
            crate::linearity::LinearityChecker::new().check_module(&resolved_module);
        for err in &linearity_result.errors {
            if err.span.start < source_len && err.span.end <= source_len {
                diagnostics.push(Diagnostic {
                    range: span_to_range(&source, err.span),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: err.message.clone(),
                    ..Default::default()
                });
            }
        }
        for warn in &linearity_result.warnings {
            if warn.span.start < source_len && warn.span.end <= source_len {
                diagnostics.push(Diagnostic {
                    range: span_to_range(&source, warn.span),
                    severity: Some(DiagnosticSeverity::WARNING),
                    message: warn.message.clone(),
                    ..Default::default()
                });
            }
        }

        let local_fn_errors = crate::linearity::check_local_fns(&resolved_module);
        for err in &local_fn_errors {
            if err.span.start < source_len && err.span.end <= source_len {
                diagnostics.push(Diagnostic {
                    range: span_to_range(&source, err.span),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: err.message.clone(),
                    ..Default::default()
                });
            }
        }

        let type_registry = crate::types::TypeRegistry::from_module(&resolved_module);

        let local_type_map: HashMap<(usize, usize), crate::type_repr::Type> = infer_result
            .type_map
            .into_iter()
            .filter(|((s, e), _)| *s < source_len && *e <= source_len)
            .collect();

        {
            let mut docs = self.documents.write().await;
            if let Some(doc) = docs.get_mut(uri) {
                doc.type_map = local_type_map;
                doc.type_registry = type_registry;
                doc.definitions = resolved_module.definitions;
                doc.source_len = source_len;
            }
        }

        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for GlassLsp {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions::default()),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Glass LSP initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        {
            let mut docs = self.documents.write().await;
            docs.insert(
                uri.clone(),
                DocumentState {
                    source: params.text_document.text,
                    type_map: HashMap::new(),
                    type_registry: empty_registry(),
                    definitions: Vec::new(),
                    source_len: 0,
                },
            );
        }
        self.analyze(&uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            {
                let mut docs = self.documents.write().await;
                docs.insert(
                    uri.clone(),
                    DocumentState {
                        source: change.text,
                        type_map: HashMap::new(),
                        type_registry: empty_registry(),
                        definitions: Vec::new(),
                        source_len: 0,
                    },
                );
            }
            self.analyze(&uri).await;
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };

        let offset = position_to_offset(&doc.source, pos);

        let mut best: Option<((usize, usize), &crate::type_repr::Type)> = None;
        for (span, ty) in &doc.type_map {
            if span.0 <= offset && offset < span.1 {
                match best {
                    Some((best_span, _)) if (span.1 - span.0) < (best_span.1 - best_span.0) => {
                        best = Some((*span, ty));
                    }
                    None => {
                        best = Some((*span, ty));
                    }
                    _ => {}
                }
            }
        }

        match best {
            Some((span, ty)) => {
                let ty_str = format!("{ty}");
                if ty_str.starts_with('?') {
                    return Ok(None);
                }
                let range = span_to_range(
                    &doc.source,
                    crate::token::Span {
                        start: span.0,
                        end: span.1,
                    },
                );
                Ok(Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::LanguageString(LanguageString {
                        language: "glass".to_string(),
                        value: ty_str,
                    })),
                    range: Some(range),
                }))
            }
            None => Ok(None),
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };

        let offset = position_to_offset(&doc.source, pos);
        let mut items = Vec::new();

        let before = if offset > 0 {
            doc.source.as_bytes().get(offset - 1).copied()
        } else {
            None
        };

        match before {
            Some(b'.') => {
                let mut seen = std::collections::HashSet::new();
                for type_info in doc.type_registry.types.values() {
                    for variant in &type_info.variants {
                        for field in &variant.fields {
                            if seen.insert(field.name.clone()) {
                                items.push(CompletionItem {
                                    label: field.name.clone(),
                                    kind: Some(CompletionItemKind::FIELD),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }
            }
            _ => {
                for kw in &[
                    "fn", "let", "case", "struct", "enum", "pub", "import", "const", "clone",
                    "todo", "True", "False",
                ] {
                    items.push(CompletionItem {
                        label: (*kw).to_string(),
                        kind: Some(CompletionItemKind::KEYWORD),
                        ..Default::default()
                    });
                }

                for def in &doc.definitions {
                    match def {
                        crate::ast::Definition::Function(f)
                            if f.is_pub || f.span.start < doc.source_len =>
                        {
                            items.push(CompletionItem {
                                label: f.name.clone(),
                                kind: Some(CompletionItemKind::FUNCTION),
                                ..Default::default()
                            });
                        }
                        crate::ast::Definition::Const(c) => {
                            items.push(CompletionItem {
                                label: c.name.clone(),
                                kind: Some(CompletionItemKind::CONSTANT),
                                ..Default::default()
                            });
                        }
                        _ => {}
                    }
                }

                for type_info in doc.type_registry.types.values() {
                    items.push(CompletionItem {
                        label: type_info.name.clone(),
                        kind: Some(CompletionItemKind::CLASS),
                        ..Default::default()
                    });
                    if type_info.is_enum {
                        for variant in &type_info.variants {
                            items.push(CompletionItem {
                                label: format!("{}::{}", type_info.name, variant.name),
                                kind: Some(CompletionItemKind::ENUM_MEMBER),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }

        items.sort_by(|a, b| a.label.cmp(&b.label));
        items.dedup_by(|a, b| a.label == b.label);

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;

        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };

        let offset = position_to_offset(&doc.source, pos);
        let word = find_word_at_offset(&doc.source, offset);

        let is_local =
            |span: crate::token::Span| span.start < doc.source_len && span.end <= doc.source_len;

        let mut last_local: Option<crate::token::Span> = None;
        for def in &doc.definitions {
            let span = match def {
                crate::ast::Definition::Function(f) if f.name == word => Some(f.span),
                crate::ast::Definition::Type(t) if t.name == word => Some(t.span),
                crate::ast::Definition::Const(c) if c.name == word => Some(c.span),
                _ => None,
            };
            if let Some(s) = span
                && is_local(s)
            {
                last_local = Some(s);
            }
        }

        if last_local.is_none() {
            for type_info in doc.type_registry.types.values() {
                for variant in &type_info.variants {
                    if variant.fields.iter().any(|f| f.name == word) {
                        for def in &doc.definitions {
                            if let crate::ast::Definition::Type(t) = def
                                && t.name == type_info.name
                                && is_local(t.span)
                            {
                                last_local = Some(t.span);
                            }
                        }
                    }
                }
            }
        }

        match last_local {
            Some(span) => {
                let range = span_to_range(&doc.source, span);
                Ok(Some(GotoDefinitionResponse::Scalar(Location {
                    uri: uri.clone(),
                    range,
                })))
            }
            None => Ok(None),
        }
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let docs = self.documents.read().await;
        let Some(doc) = docs.get(uri) else {
            return Ok(None);
        };

        let offset = position_to_offset(&doc.source, pos);
        let word = find_word_at_offset(&doc.source, offset);

        if word.is_empty() {
            return Ok(None);
        }

        let mut locations = Vec::new();
        let bytes = doc.source.as_bytes();
        let word_bytes = word.as_bytes();
        let wlen = word_bytes.len();

        let mut i = 0;
        while i + wlen <= bytes.len() {
            if doc.source.get(i..i + wlen) == Some(&word) {
                let before_ok = i == 0
                    || bytes
                        .get(i - 1)
                        .is_some_and(|b| !b.is_ascii_alphanumeric() && *b != b'_');
                let after_ok = i + wlen >= bytes.len()
                    || bytes
                        .get(i + wlen)
                        .is_some_and(|b| !b.is_ascii_alphanumeric() && *b != b'_');
                if before_ok && after_ok {
                    let range = span_to_range(
                        &doc.source,
                        crate::token::Span {
                            start: i,
                            end: i + wlen,
                        },
                    );
                    locations.push(Location {
                        uri: uri.clone(),
                        range,
                    });
                }
            }
            i += 1;
        }

        if locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(locations))
        }
    }
}

fn empty_registry() -> crate::types::TypeRegistry {
    crate::types::TypeRegistry {
        types: HashMap::new(),
        list_types: std::collections::HashSet::new(),
    }
}

fn position_to_offset(source: &str, pos: Position) -> usize {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in source.char_indices() {
        if line == pos.line && col == pos.character {
            return i;
        }
        if ch == '\n' {
            if line == pos.line {
                return i;
            }
            line = line.saturating_add(1);
            col = 0;
        } else {
            col = col.saturating_add(1);
        }
    }
    source.len()
}

fn find_word_at_offset(source: &str, offset: usize) -> String {
    let bytes = source.as_bytes();
    let mut start = offset;
    let mut end = offset;
    while start > 0 {
        match bytes.get(start.wrapping_sub(1)) {
            Some(b) if b.is_ascii_alphanumeric() || *b == b'_' => start -= 1,
            _ => break,
        }
    }
    while end < bytes.len() {
        match bytes.get(end) {
            Some(b) if b.is_ascii_alphanumeric() || *b == b'_' => end += 1,
            _ => break,
        }
    }
    source.get(start..end).unwrap_or_default().to_string()
}

fn span_to_range(source: &str, span: crate::token::Span) -> Range {
    let start = offset_to_position(source, span.start);
    let end = offset_to_position(source, span.end);
    Range { start, end }
}

fn offset_to_position(source: &str, offset: usize) -> Position {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line = line.saturating_add(1);
            col = 0;
        } else {
            col = col.saturating_add(1);
        }
    }
    Position {
        line,
        character: col,
    }
}

#[tokio::main]
pub async fn run_lsp() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| GlassLsp {
        client,
        documents: Arc::new(RwLock::new(HashMap::new())),
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
