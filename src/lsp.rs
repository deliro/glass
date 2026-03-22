use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

struct DocumentState {
    source: String,
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
        let tokens = crate::token::Lexer::tokenize(&source);
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

        let input_path = uri
            .to_file_path()
            .unwrap_or_default();
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
            diagnostics.push(Diagnostic {
                range: span_to_range(&source, err.span),
                severity: Some(DiagnosticSeverity::ERROR),
                message: err.message.clone(),
                ..Default::default()
            });
        }

        let linearity_result =
            crate::linearity::LinearityChecker::new().check_module(&resolved_module);
        for err in &linearity_result.errors {
            diagnostics.push(Diagnostic {
                range: span_to_range(&source, err.span),
                severity: Some(DiagnosticSeverity::ERROR),
                message: err.message.clone(),
                ..Default::default()
            });
        }
        for warn in &linearity_result.warnings {
            diagnostics.push(Diagnostic {
                range: span_to_range(&source, warn.span),
                severity: Some(DiagnosticSeverity::WARNING),
                message: warn.message.clone(),
                ..Default::default()
            });
        }

        let local_fn_errors = crate::linearity::check_local_fns(&resolved_module);
        for err in &local_fn_errors {
            diagnostics.push(Diagnostic {
                range: span_to_range(&source, err.span),
                severity: Some(DiagnosticSeverity::ERROR),
                message: err.message.clone(),
                ..Default::default()
            });
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
                    },
                );
            }
            self.analyze(&uri).await;
        }
    }

    async fn hover(&self, _params: HoverParams) -> Result<Option<Hover>> {
        Ok(None)
    }

    async fn completion(&self, _params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let keywords = [
            "fn", "let", "case", "struct", "enum", "pub", "import", "const", "clone", "todo",
            "True", "False",
        ];
        let items: Vec<CompletionItem> = keywords
            .iter()
            .map(|kw| CompletionItem {
                label: (*kw).to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            })
            .collect();
        Ok(Some(CompletionResponse::Array(items)))
    }
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
