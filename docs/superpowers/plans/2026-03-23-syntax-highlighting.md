# Glass Syntax Highlighting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Syntax highlighting for Glass in Zed via tree-sitter grammar + Zed extension.

**Architecture:** Tree-sitter grammar (`tree-sitter-glass/`) defines the parser. Zed extension (`editors/zed-glass/`) bundles the grammar with highlight queries. Install as dev extension in Zed.

**Tech Stack:** JavaScript (tree-sitter grammar.js), TOML (Zed config), Scheme (tree-sitter queries)

**Spec:** `docs/superpowers/specs/2026-03-23-editor-integration-design.md`

---

### Task 1: Tree-sitter Grammar — Scaffold + Literals

**Files:**
- Create: `tree-sitter-glass/grammar.js`
- Create: `tree-sitter-glass/package.json`

- [ ] **Step 1: Create package.json**

```json
{
  "name": "tree-sitter-glass",
  "version": "0.1.0",
  "main": "bindings/node",
  "types": "bindings/node",
  "scripts": {
    "build": "tree-sitter generate",
    "test": "tree-sitter test"
  },
  "tree-sitter": [
    {
      "scope": "source.glass",
      "file-types": ["glass"],
      "highlights": "queries/highlights.scm",
      "locals": "queries/locals.scm"
    }
  ]
}
```

- [ ] **Step 2: Create grammar.js with literals and comments**

Start minimal — just enough to parse literals and comments:

```javascript
module.exports = grammar({
  name: 'glass',

  extras: $ => [/\s/, $.comment],

  word: $ => $.lower_ident,

  rules: {
    source_file: $ => repeat($._definition),

    _definition: $ => choice(
      $.function_def,
      $.struct_def,
      $.enum_def,
      $.const_def,
      $.import_def,
      $.external_def,
    ),

    comment: $ => /\/\/[^\n]*/,

    // Identifiers
    lower_ident: $ => /[a-z_][a-zA-Z0-9_]*/,
    upper_ident: $ => /[A-Z][a-zA-Z0-9_]*/,

    // Literals
    int_literal: $ => choice(
      /0x[0-9a-fA-F]+/,
      /[0-9]+/,
    ),
    float_literal: $ => /[0-9]+\.[0-9]+/,
    string_literal: $ => /"([^"\\]|\\.)*"/,
    rawcode_literal: $ => /'[a-zA-Z0-9]{4}'/,
    bool_literal: $ => choice('True', 'False'),

    // Placeholder definitions — filled in next tasks
    function_def: $ => 'PLACEHOLDER_FN',
    struct_def: $ => 'PLACEHOLDER_STRUCT',
    enum_def: $ => 'PLACEHOLDER_ENUM',
    const_def: $ => 'PLACEHOLDER_CONST',
    import_def: $ => 'PLACEHOLDER_IMPORT',
    external_def: $ => 'PLACEHOLDER_EXTERNAL',
  },
});
```

- [ ] **Step 3: Generate and test**

```bash
cd tree-sitter-glass
npm install
npx tree-sitter generate
echo '// hello\n42\n3.14\n"test"\nTrue' > /tmp/test.glass
npx tree-sitter parse /tmp/test.glass
```

- [ ] **Step 4: Commit**

```bash
git add tree-sitter-glass/
git commit -m "feat: tree-sitter-glass scaffold with literals and comments"
```

---

### Task 2: Tree-sitter Grammar — Top-level Definitions

**Files:**
- Modify: `tree-sitter-glass/grammar.js`

Replace all placeholder definitions with real grammar rules.

- [ ] **Step 1: Implement import_def**

```javascript
import_def: $ => seq(
  'import',
  field('path', $.module_path),
  optional($.import_items),
),
module_path: $ => sep1($.lower_ident, '/'),
import_items: $ => seq('{', sep1($.import_item, ','), '}'),
import_item: $ => seq(
  choice($.upper_ident, $.lower_ident),
  optional(seq('as', choice($.upper_ident, $.lower_ident))),
),
```

- [ ] **Step 2: Implement const_def**

```javascript
const_def: $ => seq(
  optional('pub'),
  'const',
  field('name', choice($.lower_ident, $.upper_ident)),
  optional(seq(':', $._type_expr)),
  '=',
  field('value', $._expression),
),
```

- [ ] **Step 3: Implement struct_def**

```javascript
struct_def: $ => seq(
  optional('pub'),
  'struct',
  field('name', $.upper_ident),
  optional($.type_params),
  '{',
  sep($.field_def, ','),
  '}',
),
field_def: $ => seq(
  field('name', $.lower_ident),
  ':',
  field('type', $._type_expr),
),
type_params: $ => seq('(', sep1($._type_expr, ','), ')'),
```

- [ ] **Step 4: Implement enum_def**

```javascript
enum_def: $ => seq(
  optional('pub'),
  'enum',
  field('name', $.upper_ident),
  optional($.type_params),
  '{',
  repeat1($.variant_def),
  '}',
),
variant_def: $ => seq(
  field('name', $.upper_ident),
  optional(choice(
    seq('(', sep1($._type_expr, ','), ')'),
    seq('{', sep($.field_def, ','), '}'),
  )),
),
```

- [ ] **Step 5: Implement function_def**

```javascript
function_def: $ => seq(
  optional('pub'),
  optional('local'),
  'fn',
  field('name', $.lower_ident),
  '(',
  sep($.param, ','),
  ')',
  optional(seq('->', $._type_expr)),
  field('body', $.block),
),
param: $ => seq(
  field('pattern', $._pattern),
  optional(seq(':', $._type_expr)),
),
block: $ => seq('{', repeat($._expression), '}'),
```

- [ ] **Step 6: Implement external_def**

```javascript
external_def: $ => seq(
  '@',
  'external',
  '(',
  $.string_literal,
  ',',
  $.string_literal,
  ')',
  optional('pub'),
  'fn',
  field('name', $.lower_ident),
  '(',
  sep($.param, ','),
  ')',
  optional(seq('->', $._type_expr)),
),
```

- [ ] **Step 7: Implement type expressions**

```javascript
_type_expr: $ => choice(
  $.type_name,
  $.type_application,
  $.function_type,
  $.tuple_type,
),
type_name: $ => choice($.upper_ident, $.lower_ident),
type_application: $ => seq(
  $.upper_ident,
  '(',
  sep1($._type_expr, ','),
  ')',
),
function_type: $ => seq(
  'fn',
  '(',
  sep($._type_expr, ','),
  ')',
  '->',
  $._type_expr,
),
tuple_type: $ => seq('(', sep1($._type_expr, ','), ')'),
```

- [ ] **Step 8: Generate and test against examples**

```bash
cd tree-sitter-glass
npx tree-sitter generate
npx tree-sitter parse ../examples/elm_counter.glass
```

Should parse without ERROR nodes for top-level structure.

- [ ] **Step 9: Commit**

```bash
git add tree-sitter-glass/
git commit -m "feat: tree-sitter-glass top-level definitions"
```

---

### Task 3: Tree-sitter Grammar — Expressions and Patterns

**Files:**
- Modify: `tree-sitter-glass/grammar.js`

- [ ] **Step 1: Implement expression rules**

```javascript
_expression: $ => choice(
  $.int_literal,
  $.float_literal,
  $.string_literal,
  $.rawcode_literal,
  $.bool_literal,
  $.variable,
  $.binary_expr,
  $.unary_expr,
  $.call_expr,
  $.field_access,
  $.method_call,
  $.constructor,
  $.record_update,
  $.let_expr,
  $.case_expr,
  $.lambda_expr,
  $.list_literal,
  $.list_cons,
  $.tuple_literal,
  $.pipe_expr,
  $.clone_expr,
  $.todo_expr,
  $.block,
  $.qualified_access,
),

variable: $ => $.lower_ident,

qualified_access: $ => seq(
  field('module', $.lower_ident),
  '.',
  field('member', choice($.lower_ident, $.upper_ident)),
),

binary_expr: $ => choice(
  ...['||', '&&'].map(op =>
    prec.left(1, seq($._expression, op, $._expression))
  ),
  ...['==', '!=', '<', '>', '<=', '>='].map(op =>
    prec.left(2, seq($._expression, op, $._expression))
  ),
  ...['+', '-', '<>'].map(op =>
    prec.left(3, seq($._expression, op, $._expression))
  ),
  ...['*', '/', '%'].map(op =>
    prec.left(4, seq($._expression, op, $._expression))
  ),
),

unary_expr: $ => prec(5, seq(choice('-', '!'), $._expression)),

pipe_expr: $ => prec.left(0, seq($._expression, '|>', $._expression)),

call_expr: $ => prec(6, seq(
  field('function', $._expression),
  '(',
  sep($._expression, ','),
  ')',
)),

field_access: $ => prec(7, seq($._expression, '.', $.lower_ident)),

method_call: $ => prec(7, seq(
  $._expression,
  '.',
  field('method', $.lower_ident),
  '(',
  sep($._expression, ','),
  ')',
)),

constructor: $ => prec(6, seq(
  field('name', choice(
    seq($.upper_ident, '::', $.upper_ident),
    $.upper_ident,
  )),
  optional(choice(
    seq('(', sep($._constructor_arg, ','), ')'),
    seq('{', sep($._constructor_arg, ','), '}'),
  )),
)),

_constructor_arg: $ => choice(
  seq($.lower_ident, ':', $._expression),
  seq('..', $._expression),
  $._expression,
),

record_update: $ => seq(
  $.upper_ident,
  '(',
  '..',
  $._expression,
  ',',
  sep1(seq($.lower_ident, ':', $._expression), ','),
  ')',
),

let_expr: $ => seq(
  'let',
  field('pattern', $._pattern),
  optional(seq(':', $._type_expr)),
  '=',
  field('value', $._expression),
  optional($._expression),
),

case_expr: $ => seq(
  'case',
  field('subject', $._expression),
  '{',
  repeat1($.case_arm),
  '}',
),

case_arm: $ => seq(
  field('pattern', $._pattern),
  optional(seq('as', $.lower_ident)),
  optional(seq('if', $._expression)),
  '->',
  field('body', $._expression),
),

lambda_expr: $ => seq(
  'fn',
  '(',
  sep($.param, ','),
  ')',
  optional(seq('->', $._type_expr)),
  $.block,
),

list_literal: $ => seq('[', sep($._expression, ','), ']'),

list_cons: $ => seq('[', $._expression, '|', $._expression, ']'),

tuple_literal: $ => prec(8, seq('(', $._expression, ',', sep($._expression, ','), ')')),

clone_expr: $ => seq('clone', '(', $._expression, ')'),

todo_expr: $ => seq('todo', '(', optional($.string_literal), ')'),
```

- [ ] **Step 2: Implement pattern rules**

```javascript
_pattern: $ => choice(
  $.pat_wildcard,
  $.pat_variable,
  $.pat_literal,
  $.pat_constructor,
  $.pat_constructor_named,
  $.pat_tuple,
  $.pat_list,
  $.pat_list_cons,
  $.pat_or,
),

pat_wildcard: $ => '_',
pat_variable: $ => $.lower_ident,
pat_literal: $ => choice($.int_literal, $.string_literal, $.bool_literal, $.rawcode_literal),
pat_constructor: $ => seq(
  optional(seq($.upper_ident, '::')),
  $.upper_ident,
  optional(seq('(', sep1($._pattern, ','), ')')),
),
pat_constructor_named: $ => seq(
  $.upper_ident,
  '{',
  sep($.field_pattern, ','),
  optional('..'),
  '}',
),
field_pattern: $ => seq(
  $.lower_ident,
  optional(choice(
    seq('as', $.lower_ident),
    seq(':', $._pattern),
  )),
),
pat_tuple: $ => seq('(', $._pattern, ',', sep($._pattern, ','), ')'),
pat_list: $ => seq('[', sep($._pattern, ','), ']'),
pat_list_cons: $ => seq('[', $._pattern, '|', $._pattern, ']'),
pat_or: $ => prec.left(seq($._pattern, '|', $._pattern)),
```

- [ ] **Step 3: Add helper function**

At the top of grammar.js, before `module.exports`:

```javascript
function sep(rule, separator) {
  return optional(sep1(rule, separator));
}

function sep1(rule, separator) {
  return seq(rule, repeat(seq(separator, rule)));
}
```

- [ ] **Step 4: Generate and test against multiple examples**

```bash
cd tree-sitter-glass
npx tree-sitter generate
for f in ../examples/elm_counter.glass ../examples/elm_timer.glass ../examples/hook_demo.glass ../examples/invoker.glass; do
  echo "=== $(basename $f) ==="
  npx tree-sitter parse "$f" 2>&1 | tail -1
done
```

Goal: no ERROR nodes for the core syntax. Some ambiguity warnings are OK at this stage.

- [ ] **Step 5: Create test corpus**

Create `tree-sitter-glass/test/corpus/basics.txt`:

```
================
Simple function
================

fn add(a: Int, b: Int) -> Int { a + b }

---

(source_file
  (function_def
    name: (lower_ident)
    (param (pat_variable (lower_ident)) (type_name (upper_ident)))
    (param (pat_variable (lower_ident)) (type_name (upper_ident)))
    (type_name (upper_ident))
    body: (block (binary_expr (variable (lower_ident)) (variable (lower_ident))))))

================
Import
================

import wc3/unit

---

(source_file
  (import_def path: (module_path (lower_ident) (lower_ident))))

================
Enum
================

pub enum Msg { Tick Reset }

---

(source_file
  (enum_def
    name: (upper_ident)
    (variant_def name: (upper_ident))
    (variant_def name: (upper_ident))))

================
Const with rawcode
================

const hero_id: Int = 'Otch'

---

(source_file
  (const_def
    name: (lower_ident)
    (type_name (upper_ident))
    value: (rawcode_literal)))
```

Run: `cd tree-sitter-glass && npx tree-sitter test`

- [ ] **Step 6: Iterate until tests pass**

Fix grammar conflicts. Tree-sitter will report conflicts — resolve with `prec`, `prec.left`, `prec.right`, or `prec.dynamic`. Common issues:
- `call_expr` vs `constructor` (both start with ident + `(`)
- `field_access` vs `qualified_access` (both use `.`)
- `let_expr` body ambiguity (let without explicit separator)

- [ ] **Step 7: Commit**

```bash
git add tree-sitter-glass/
git commit -m "feat: tree-sitter-glass full expression and pattern grammar"
```

---

### Task 4: Highlight Queries

**Files:**
- Create: `tree-sitter-glass/queries/highlights.scm`

- [ ] **Step 1: Write highlights.scm**

```scheme
; Keywords
["fn" "let" "case" "import" "pub" "const" "struct" "enum"
 "extend" "local" "clone" "todo" "as"] @keyword

; Guard keyword (contextual)
(case_arm "if" @keyword)

; Arrows and special syntax
["->" "=>" "|>"] @operator
["=" ":" "::" ".." "@"] @punctuation.delimiter
["(" ")" "{" "}" "[" "]"] @punctuation.bracket
["," "|"] @punctuation.delimiter

; Operators
["+" "-" "*" "/" "%" "==" "!=" "<" ">" "<=" ">=" "&&" "||" "<>" "!"] @operator

; Literals
(int_literal) @number
(float_literal) @number.float
(string_literal) @string
(rawcode_literal) @string.special
(bool_literal) @constant.builtin

; Comments
(comment) @comment

; Identifiers
(function_def name: (lower_ident) @function)
(external_def name: (lower_ident) @function)
(call_expr function: (variable (lower_ident) @function.call))
(method_call method: (lower_ident) @function.method)
(param (pat_variable (lower_ident) @variable.parameter))

; Types
(type_name (upper_ident) @type)
(type_application (upper_ident) @type)
(struct_def name: (upper_ident) @type)
(enum_def name: (upper_ident) @type)
(variant_def name: (upper_ident) @constructor)

; Constructor usage
(constructor name: (upper_ident) @constructor)
(pat_constructor (upper_ident) @constructor)

; Field names
(field_def name: (lower_ident) @property)
(field_access (lower_ident) @property)

; Module
(import_def path: (module_path (lower_ident) @module))
(qualified_access module: (lower_ident) @module)

; Wildcard pattern
(pat_wildcard) @variable.builtin

; External attribute
(external_def "@" @attribute "external" @attribute)
```

- [ ] **Step 2: Test highlighting**

```bash
cd tree-sitter-glass
npx tree-sitter highlight ../examples/elm_counter.glass
npx tree-sitter highlight ../examples/hook_demo.glass
```

Visually inspect the colored output. Keywords should be one color, types another, strings another, etc.

- [ ] **Step 3: Commit**

```bash
git add tree-sitter-glass/queries/
git commit -m "feat: tree-sitter-glass highlight queries"
```

---

### Task 5: Zed Extension

**Files:**
- Create: `editors/zed-glass/extension.toml`
- Create: `editors/zed-glass/Cargo.toml`
- Create: `editors/zed-glass/src/lib.rs`
- Create: `editors/zed-glass/languages/glass/config.toml`
- Copy: `editors/zed-glass/languages/glass/highlights.scm` (from tree-sitter-glass)

- [ ] **Step 1: Create extension.toml**

```toml
id = "glass"
name = "Glass"
version = "0.1.0"
schema_version = 1
authors = ["Glass Contributors"]
description = "Glass language support — syntax highlighting"

[grammars.glass]
repository = "https://github.com/user/tree-sitter-glass"
commit = ""
```

Note: for local dev, we'll use `Install Dev Extension` which builds from local tree-sitter-glass. The `repository` field is for published extensions. During dev, the grammar is resolved from the local path.

- [ ] **Step 2: Create Cargo.toml**

```toml
[package]
name = "zed-glass"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
zed_extension_api = "0.1.0"
```

- [ ] **Step 3: Create src/lib.rs**

```rust
use zed_extension_api as zed;

struct GlassExtension;

impl zed::Extension for GlassExtension {
    fn new() -> Self {
        GlassExtension
    }
}

zed::register_extension!(GlassExtension);
```

- [ ] **Step 4: Create languages/glass/config.toml**

```toml
name = "Glass"
grammar = "glass"
path_suffixes = ["glass"]
line_comments = ["// "]
block_comment = []
brackets = [
  { start = "{", end = "}", close = true, newline = true },
  { start = "(", end = ")", close = true, newline = false },
  { start = "[", end = "]", close = true, newline = false },
]
word_characters = ["_"]
```

- [ ] **Step 5: Copy highlights.scm**

```bash
cp tree-sitter-glass/queries/highlights.scm editors/zed-glass/languages/glass/highlights.scm
```

- [ ] **Step 6: Test in Zed**

1. Open Zed
2. Extensions → Install Dev Extension
3. Select `editors/zed-glass/` directory
4. Open any `.glass` file
5. Verify syntax highlighting works

- [ ] **Step 7: Commit**

```bash
git add editors/zed-glass/
git commit -m "feat: Zed extension for Glass syntax highlighting"
```

---

### Task 6: Polish and Validate

- [ ] **Step 1: Test all example files parse**

```bash
cd tree-sitter-glass
for f in ../examples/*.glass ../examples/game/main.glass ../examples/game/heroes/*.glass ../sdk/*.glass ../sdk/wc3/*.glass; do
  errors=$(npx tree-sitter parse "$f" 2>&1 | grep -c ERROR)
  echo "$(basename $f): $errors errors"
done
```

- [ ] **Step 2: Fix any grammar issues found**

Iterate on grammar.js to reduce ERROR nodes. Priority: examples/ and sdk/ files should parse cleanly.

- [ ] **Step 3: Add more test corpus cases**

Add tests for: case expressions with guards, or-patterns, as-bindings, lambdas, pipe chains, record updates, list cons, qualified access, external annotations.

- [ ] **Step 4: Final commit**

```bash
git add tree-sitter-glass/ editors/zed-glass/
git commit -m "fix: tree-sitter-glass grammar polish, all examples parse"
```
