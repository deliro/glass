# Editor Integration: Tree-sitter + LSP

## Goal

Full editor support for Glass in Zed and VSCode: syntax highlighting, inline diagnostics, autocomplete, go-to-definition, hover types.

## Architecture

Three deliverables:

1. **Tree-sitter grammar** (`tree-sitter-glass/`) — syntax highlighting and structural queries
2. **LSP server** (`glass lsp` subcommand) — diagnostics, completion, navigation
3. **Editor extensions** — Zed extension + VSCode extension that wire tree-sitter + LSP together

## 1. Tree-sitter Grammar

Directory: `tree-sitter-glass/` in repo root.

```
tree-sitter-glass/
  grammar.js
  src/                    # generated C parser
  queries/
    highlights.scm        # syntax highlighting
    locals.scm            # scope tracking
  package.json
  binding.gyp
```

### Grammar coverage

Top-level definitions:
- `fn`, `pub fn`, `local fn` with params, return type, body
- `pub struct Name { field: Type }` and `pub enum Name { Variant1 Variant2(Type) }`
- `const Name: Type = value`
- `import path`, `import path { items }`, `import path { Item as Alias }`
- `@external("module", "name") pub fn ...`
- `extend Type(params) { methods }`

Expressions:
- Literals: int (decimal, hex), float, string (with escapes), rawcode (`'hfoo'`), bool (`True`/`False`)
- Binary ops: `+`, `-`, `*`, `/`, `%`, `==`, `!=`, `<`, `>`, `<=`, `>=`, `&&`, `||`, `<>`, `|>`
- Unary: `-`, `!`
- Let binding: `let pattern: Type = value`
- Case: `case expr { Pattern -> body ... }` with guards (`if`), or-patterns (`|`), as-binding (`as`)
- Lambda: `fn(params) { body }`
- Constructor: `Type(args)`, `Type::Variant(args)`, `Type { field: val }`
- Record update: `Type(..base, field: val)`
- Field access: `expr.field`
- Method call: `expr.method(args)`
- List: `[a, b]`, `[head | tail]`
- Tuple: `(a, b)`
- Block: `{ expr1 expr2 }`
- Clone: `clone(expr)`
- Todo: `todo()`

Comments: `// line comment`

### highlights.scm mapping

| Node | Scope |
|---|---|
| `fn`, `let`, `case`, `import`, `pub`, `const`, `struct`, `enum`, `extend`, `local`, `clone`, `todo`, `as` | `@keyword` |
| `if` (guard in case arm) | `@keyword` (contextual — only in pattern guard position) |
| `_` (wildcard pattern) | `@variable.builtin` |
| `True`, `False` | `@constant.builtin` |
| Function name in definition | `@function` |
| Function name in call | `@function.call` |
| Type name (UpperIdent in type position) | `@type` |
| Constructor name (UpperIdent in expression) | `@constructor` |
| Field name | `@property` |
| String literal | `@string` |
| Rawcode literal | `@string.special` |
| Int/Float literal | `@number` |
| Comment | `@comment` |
| Operator | `@operator` |
| `@external` | `@attribute` |
| Module name in import | `@module` |
| Parameter name | `@variable.parameter` |

## 2. LSP Server

Subcommand: `glass lsp`. Runs a JSON-RPC stdio LSP server.

Implementation: Rust, using `tower-lsp` crate for the protocol layer.

### LSP capabilities

**Phase 1 (MVP):**
- `textDocument/publishDiagnostics` — parse + type check errors inline
- `textDocument/completion` — local variables, imported functions, keywords, constructors
- `textDocument/hover` — show inferred type of expression under cursor

**Phase 2:**
- `textDocument/definition` — go to function/type/import definition
- `textDocument/references` — find all usages
- `textDocument/formatting` — auto-format (if formatter exists)
- `textDocument/signatureHelp` — function parameter hints

### Internal architecture

```
glass lsp
  ├── LspServer (tower-lsp Backend impl)
  │     ├── on_initialize() → register capabilities
  │     ├── on_did_open/change() → re-parse + type check → publish diagnostics
  │     ├── on_completion() → query scope for completions
  │     ├── on_hover() → lookup type at position from type_map
  │     └── on_goto_definition() → lookup definition span
  ├── DocumentState (per open file)
  │     ├── source: String
  │     ├── tokens: Vec<Token>
  │     ├── module: Module (parsed AST)
  │     ├── type_map: HashMap<(usize,usize), Type>
  │     └── diagnostics: Vec<Diagnostic>
  └── Workspace
        ├── module_resolver: ModuleResolver
        └── documents: HashMap<Url, DocumentState>
```

On every file change:
1. Re-lex + re-parse (fast — <10ms for typical files)
2. Resolve imports (cached)
3. Run type inference
4. Collect errors from parser + inferencer + linearity checker
5. Publish diagnostics

For completion:
- At cursor position, determine scope (which let bindings, function params, imported names are visible)
- Suggest: local variables, function names, type constructors, keywords, field names (after `.`)

For hover:
- Find AST node at cursor position
- Look up its type in `type_map` (keyed by span)
- Format type for display

For go-to-definition:
- If cursor is on a function name → find the FnDef with that name
- If on a type name → find the TypeDef
- If on an import → find the file path
- Return the definition's span as Location

### Reusing existing compiler code

The LSP reuses these modules directly:
- `token.rs` — lexer
- `parser.rs` — parser
- `infer.rs` — type inference
- `modules.rs` — import resolution
- `linearity.rs` — linearity checking
- `exhaustive.rs` — exhaustiveness checking
- `types.rs` — type registry

No new parsing or type checking code. The LSP is a thin wrapper that calls the same functions as `main.rs` but keeps results in memory and maps them to LSP protocol types.

### Position mapping

LSP uses line:column positions. The compiler uses byte offsets (Span { start, end }). Need a `PositionMapper` that converts between the two using the source text. This is a simple utility: scan the source for `\n` to build a line offset table.

## 3. Editor Extensions

### Zed extension

Directory: `editors/zed/` or published as a Zed extension.

Zed extensions are TOML-configured:
```toml
# extension.toml
[extension]
name = "glass"
version = "0.1.0"

[grammars.glass]
repository = "https://github.com/user/tree-sitter-glass"

[language.glass]
path = "languages/glass"
grammar = "glass"

[language_servers.glass-lsp]
language = "glass"
command = "glass"
args = ["lsp"]
```

Plus `languages/glass/config.toml`:
```toml
name = "Glass"
grammar = "glass"
path_suffixes = ["glass"]
line_comments = ["//"]
block_comments = []
brackets = [
  { start = "{", end = "}", close = true },
  { start = "(", end = ")", close = true },
  { start = "[", end = "]", close = true },
]
```

And symlink or copy `queries/highlights.scm` from tree-sitter-glass.

### VSCode extension

Directory: `editors/vscode/`.

```
editors/vscode/
  package.json          # extension manifest
  syntaxes/
    glass.tmLanguage.json   # TextMate grammar (fallback for basic highlighting)
  language-configuration.json
```

VSCode doesn't use tree-sitter natively. Two options:
- **TextMate grammar** for syntax highlighting (simpler, standard VSCode approach)
- **vscode-anycode** for tree-sitter (experimental)

Recommended: TextMate grammar for VSCode highlighting + LSP client for everything else. The LSP client is a tiny `extension.js` that spawns `glass lsp` and connects via stdio.

```json
// package.json (key parts)
{
  "contributes": {
    "languages": [{
      "id": "glass",
      "extensions": [".glass"],
      "configuration": "./language-configuration.json"
    }],
    "grammars": [{
      "language": "glass",
      "scopeName": "source.glass",
      "path": "./syntaxes/glass.tmLanguage.json"
    }]
  }
}
```

The LSP client activates on `.glass` files and starts `glass lsp` as a child process.

## Error Recovery

The current parser stops at the first error. For LSP, partial parse results are essential — users are always mid-edit.

Strategy: on parse error, skip to the next top-level definition (`fn`, `pub`, `struct`, `enum`, `const`, `import`, `@external`) and continue parsing. Return a partial `Module` with successfully parsed definitions plus error diagnostics for the skipped regions. This gives diagnostics for the error site AND working completions/hover for the rest of the file.

Implementation: catch parse errors at the top-level `parse_definition` loop. On error, consume tokens until the next definition-starting token, record the error, and continue.

Fallback: if the parser produces no usable result (catastrophic syntax error), use the last successful parse for completions/hover and only update diagnostics.

## Position Mapping (UTF-16)

LSP positions use UTF-16 code units by default, not bytes. Glass source is UTF-8. The `PositionMapper` must:
1. Build a line offset table (scan for `\n`)
2. Convert byte offsets to (line, UTF-16 column) for LSP responses
3. Convert (line, UTF-16 column) from LSP requests to byte offsets

The `on_initialize` handler advertises `utf-8` as preferred `PositionEncodingKind` (LSP 3.17+). If the client supports it, UTF-16 conversion is unnecessary. Fall back to UTF-16 if the client doesn't negotiate.

## Phases

1. **Tree-sitter grammar** — standalone, testable with `tree-sitter parse` and corpus tests from `examples/`
2. **`glass lsp` subcommand** — diagnostics only (parse + type check errors inline)
3. **Zed extension** — Rust/WASM extension with `language_server_command()` returning `glass lsp`
4. **VSCode extension** — TextMate grammar + `vscode-languageclient` LSP client
5. **LSP: hover + completion** — type info and autocomplete
6. **LSP: go-to-definition** — navigate to definitions
7. **LSP: Phase 2 features** — references, signature help

Phases 1-4 are the MVP. Both editors get syntax highlighting + inline error diagnostics.

## Binary Discovery

Editor extensions find the `glass` binary via:
1. `$PATH` lookup (default)
2. Extension setting `glass.path` for explicit override
3. Zed/VSCode show an error if `glass` is not found with install instructions

## Import Invalidation

Single-file analysis by default. When file A imports file B and file B changes:
- If both are open, both get re-checked
- If only A is open, A uses cached B from disk (re-read on `didSave` of B if B is also open)
- SDK modules are read once and cached for the session

## Dependencies

- `tower-lsp` crate — LSP protocol, async via tokio. Compiler passes are sync; use `spawn_blocking` for heavy passes.
- `tree-sitter-cli` — for generating the C parser from grammar.js
- Node.js — for tree-sitter grammar development
- `vscode-languageclient` — VSCode LSP client library

## Non-Goals

- Debugger integration (DAP)
- Semantic highlighting via LSP (tree-sitter handles highlighting)
- Auto-formatting (no formatter exists yet)
- Refactoring actions (rename, extract function)
