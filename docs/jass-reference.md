# JASS Language Reference (Codegen Validation)

Comprehensive reference for validating Glass compiler JASS output.
Sources: jass.sourceforge.net manual, pjass source (github.com/lep/pjass), hiveworkshop.com community.

---

## 1. Variable Declarations

### Globals Block

```jass
globals
    integer myVar = 10
    constant integer MAX = 100
    string array names
endglobals
```

- Only **one** `globals` block per file.
- The `globals` block must appear **before all function declarations**.
- `constant` variables must be initialized at declaration.
- Arrays cannot be initialized (`integer array x = ...` is illegal).
- Arrays cannot be reassigned (`set myArray = otherArray` is illegal; only element assignment `set myArray[i] = v` is valid).
- `code array` is illegal.

### Local Declarations

```jass
function foo takes nothing returns nothing
    local integer x = 5
    local string s
    local unit array units
    // ... statements ...
endfunction
```

- Local declarations **must appear before all statements** in a function body. Declaring a local after any statement is an error.
- Locals cannot be `constant`.
- Local variable names should not collide with parameter names.
- Local arrays are legal.

---

## 2. Function Syntax

```jass
function name takes param_list returns return_type
    // local declarations
    // statements
endfunction
```

- `param_list` is either `nothing` or a comma-separated list of `type name` pairs.
- `return_type` is either `nothing` or a type name.
- Functions may be prefixed with `constant` (prevents calling non-constant functions inside).
- **No forward references**: a function must be declared/defined before it is called. The only exception is self-recursion (a function may call itself).
- Natives must be declared before user-defined functions.
- Type declarations and the globals block must appear before all function declarations.
- Each function signature must be on a **single line** (no line continuation).
- Each statement must be on a **single line**.
- Entry points: map scripts require `main` and `config` (both `takes nothing returns nothing`).

---

## 3. Expression Limitations

### Nested Function Calls

JASS **does** support nested function calls: `f(g(x))` is legal. Expressions can be arbitrarily nested. Function calls are valid expressions as long as the function returns a value (not `nothing`).

### Array Indices

Array indices can be **any expression** that evaluates to an integer:
```jass
set x = myArr[GetIndex() + 1]
set myArr[a * 2 + b] = 5
```

### Function References as Expressions

`function funcName` evaluates to a `code` value. Cannot pass arguments -- the referenced function must take `nothing`. Native functions cannot be used as code references.

### Statement vs Expression

- `call func(args)` is a statement -- the return value is discarded.
- `func(args)` inside an expression uses the return value.
- You cannot call a function returning `nothing` inside an expression.

---

## 4. Type System

### Native Types

| Type      | Description                                           |
|-----------|-------------------------------------------------------|
| `integer` | 32-bit signed, range -2147483647 to 2147483647        |
| `real`    | 32-bit IEEE 754 float                                 |
| `boolean` | `true` or `false`                                     |
| `string`  | Character sequence, can be `null`                     |
| `handle`  | Opaque pointer to engine data, can be `null`          |
| `code`    | Function reference (function pointer)                 |

### User-Defined Types

```jass
type widget extends handle
type unit extends widget
```

Types form a tree rooted at `handle`. A value of type `unit` conforms to `widget` and `handle`. Conformance is used for parameter passing: a function taking `widget` accepts a `unit`.

### Type Conversions

No implicit conversions between `integer`, `real`, `string`. Must use native functions:
- `I2S(integer) -> string`
- `R2S(real) -> string`
- `I2R(integer) -> real`
- `R2I(real) -> integer`
- `S2I(string) -> integer`
- `S2R(string) -> real`

**Exception**: In arithmetic, if any operand is `real`, the result is `real`. An `integer` operand is implicitly promoted to `real` in mixed arithmetic. A `real` may also be compared to an `integer` with `==`/`!=`.

### The `code` Type

- Represents a function pointer.
- Created with `function funcName` expression.
- The referenced function must take `nothing`.
- Can be `null`.
- `code array` is illegal.
- Native functions cannot be used as `code` values (runtime error).

---

## 5. String Limitations

### Length

- String **literals** over 1023 characters crash the game when loading a saved game (pjass warns about this).
- Concatenated strings at runtime can reach up to ~4099 characters.
- For save/load compatibility, keep strings under 1013 characters.

### Concatenation

The `+` operator concatenates two `string` values. Both operands must be `string` type.

### Escaping

Valid escape sequences in strings: `\b` (backspace), `\t` (tab), `\n` (newline), `\f` (form feed), `\r` (carriage return), `\"` (double quote), `\\` (backslash).

Any other `\x` sequence is an **error** (pjass rejects it).

### Null Strings

A `string` variable may be `null`. Using a `null` string (except to assign to it) is illegal/undefined behavior.

---

## 6. Array Limitations

- **Max size**: 8192 elements (indices 0 to 8191). Using index 8192 may corrupt save files.
- Arrays are **sparse** -- you can use any index from 0 to 8191, but the total element count is capped at 8192.
- **No multi-dimensional arrays**. `integer array array x` is not valid syntax.
- Arrays **cannot be passed to functions** as arguments.
- Arrays **cannot be returned** from functions.
- Array variables **cannot be reassigned** (no `set arr = otherArr`), only individual elements can be set.
- Array elements are initialized to zero-values (`0` for integer, `null` for handle types, etc.).
- **Local arrays** are legal.
- `code array` is illegal.
- All arrays are effectively global-scoped in behavior: local arrays of handle types can leak if not cleaned up.

---

## 7. pjass vs World Editor Differences

### What pjass Accepts That WE Might Not

- pjass is generally **stricter** than the World Editor's built-in checker in some areas (e.g., Filter/Condition return type checking).
- pjass supports the `%` (modulo) operator; this was only added to WC3 in patch 1.29+. Use `+nomodulooperator` flag for older patches.
- pjass checks for uninitialized variables (with flags).

### What WE Accepts That Is Technically Wrong

- The WE type checker has a bug: `return` values only need to conform to the **base native type**, not the declared return type. E.g., returning a `handle` from a function declared to return `unit` passes the WE checker. pjass replicates this bug by default for compatibility.
- Missing `return` statements in functions returning values: WE may accept this, but it leads to undefined behavior at runtime.

### Common Gotchas

- `return` vs `returns`: in function signatures, you must write `returns` (with 's'). pjass helpfully suggests this if you write `return`.
- `set` keyword is mandatory for all assignments. Writing `x = 5` without `set` is a syntax error.
- `call` keyword is mandatory for standalone function calls. Writing `MyFunc()` without `call` is a syntax error.
- A statement on the same line as the previous statement (without a newline) is a syntax error. pjass checks for "Missing linebreak before X" errors.
- Declaring a `local` after any statement in a function body is an error.
- Declaring types or natives after the first function definition is an error.
- `alias` is a reserved word and cannot be used as an identifier.

---

## 8. CRLF / Line Endings / BOM

- pjass accepts both `\n` (LF) and `\r\n` (CRLF) line endings. It also handles bare `\r` (CR).
- pjass silently consumes a UTF-8 BOM (`\xEF\xBB\xBF`) at the start of the file.
- The World Editor generates `\r\n` (CRLF) on Windows, but the game engine accepts both.
- **Recommendation**: Use `\r\n` for maximum compatibility if targeting all environments, but LF works in practice.

---

## 9. Operator Precedence

From the pjass bison grammar, **lowest to highest** precedence:

| Precedence | Operators           | Associativity | Notes                                        |
|------------|---------------------|---------------|----------------------------------------------|
| 1 (lowest) | `=`                 | right         | Assignment (only in `set` statements)        |
| 2          | `and`               | left          |                                              |
| 3          | `or`                | left          | **`or` has HIGHER precedence than `and`!**   |
| 4          | `==`, `!=`          | left          |                                              |
| 5          | `<`, `>`, `<=`, `>=`| left          |                                              |
| 6          | `not`               | left          |                                              |
| 7          | `+`, `-` (binary)   | left          | Also string concatenation for `+`            |
| 8 (highest)| `*`, `/`, `%`       | left          | `%` only in patch 1.29+                      |

**CRITICAL**: `or` binds **tighter** than `and` in JASS. This is the **opposite** of most languages (C, Java, Python, etc.).

Example: `false and true or true` parses as `false and (true or true)` = `false`, NOT `(false and true) or true` = `true`.

### Unary Operators

- Unary `-` and `+` apply to integer and real values.
- `not` applies to boolean values.

### Short-Circuit Evaluation

JASS **does** short-circuit `and` and `or`:
- `false and f()` -- `f()` is never called.
- `true or f()` -- `f()` is never called.

---

## 10. Common Compilation Errors

1. **"Missing linebreak before X"** -- Two statements on the same line.
2. **"Local declaration after first statement"** -- Local declared after a `set`, `call`, `if`, etc.
3. **"Undefined function X"** -- Forward reference (function used before declaration).
4. **"Undefined type X"** -- Using a type that hasn't been declared yet.
5. **"Missing return"** -- Function declared to return a value but not all paths return.
6. **"Cannot assign to constant X"** -- Using `set` on a constant variable.
7. **"X not an array" / "Index missing for array variable X"** -- Indexing a non-array or using array without index.
8. **"Arrays cannot be directly initialized"** -- `integer array x = ...` in declaration.
9. **"Function X must not take any arguments when used as code"** -- Passing a function with params as `code`.
10. **"Constants must be initialized"** -- `constant integer X` without `= value`.
11. **"Missing 'call'" / "Missing 'set'"** -- Bare function call or assignment without keyword.
12. **"Native declared after functions"** -- Native declaration after first function definition.
13. **"Types can not be extended inside functions"** -- Type declaration inside a function body.
14. **"Local constants are not allowed"** -- `constant local integer x = 5`.
15. **"String literals over 1023 chars long crash the game"** -- pjass warning.
16. **"Rawcodes must consist of 1 or 4 characters"** -- Invalid fourcc literal.
17. **"Invalid escape character sequence"** -- Unknown `\x` escape in string or rawcode.

---

## 11. Handle Type Rules

### Comparison

- Handles can be compared with `==` and `!=`. This tests **pointer equality** (whether both refer to the same object), not value equality.
- Handle subtypes can be compared across the hierarchy (e.g., `unit == widget` is legal if they share a common ancestor).

### Null

- Any handle variable can be `null`.
- `null` conforms to any handle type.
- Using a `null` handle in native functions is typically undefined behavior / runtime error.
- Local handle variables should be set to `null` before the function returns to avoid handle leaks (the engine's reference counting requires this).

### Arrays

- Handle types **can** be stored in arrays: `unit array myUnits` is legal.
- Array elements are initialized to `null`.

### Agent Type (Patch 1.24b+)

- `agent` extends `handle`. Types that extend `agent` (like `unit`, `rect`, `destructable`) must be manually destroyed.
- Types that extend `handle` directly (like `race`, `alliancetype`) are managed by the engine.

---

## 12. Execution Limits

| Limit                    | Value                  | Notes                                                |
|--------------------------|------------------------|------------------------------------------------------|
| **Op limit**             | ~300,000 operations    | Per thread/trigger execution. Exceeding crashes/halts the thread. |
| **Array size**           | 8192 elements          | Per array. Index 0-8191.                             |
| **String literal**       | 1023 chars             | Longer literals crash on save game load.             |
| **Runtime string**       | ~4099 chars            | Via concatenation.                                   |
| **Thread limit**         | Unlimited (in theory)  | Common myth says 72; actually unlimited, but deep nesting causes stack overflow. |
| **Memory (strings)**     | ~712 MB                | WC3 crashes after accumulating ~712 MB of string data. |
| **Identifier length**    | 3958 chars             | pjass `+checklongnames` warns about this.            |
| **Local variable count** | No hard syntax limit   | But excessive locals may hit memory/stack limits at runtime. |

### Op Limit Details

- The op limit counts bytecode operations, not source-level statements.
- `ExecuteFunc("name")` starts a **new thread** with a fresh op limit, allowing long computations to be split.
- `TriggerExecute` and timer callbacks also get fresh threads.

---

## 13. Reserved Words / Keywords

Complete list extracted from the pjass lexer:

**Statement/Block Keywords:**
`if`, `then`, `else`, `elseif`, `endif`, `loop`, `endloop`, `exitwhen`, `return`, `set`, `call`, `debug`

**Declaration Keywords:**
`function`, `endfunction`, `takes`, `returns`, `native`, `constant`, `local`, `type`, `extends`, `globals`, `endglobals`, `array`

**Type Keywords (built-in types):**
`integer`, `real`, `boolean`, `string`, `handle`, `code`, `nothing`

**Literal Keywords:**
`true`, `false`, `null`

**Operator Keywords:**
`and`, `or`, `not`

**Other Reserved:**
`alias`

**Total: 33 reserved words.**

All keywords are **case-sensitive**. `If`, `IF`, `TRUE` are not keywords -- they are valid identifiers.

---

## 14. The `globals` Block

```jass
globals
    integer x = 0
    constant real PI = 3.14159
    string array names
endglobals
```

- **Only one** `globals` block per file (confirmed by the JASS manual and pjass grammar).
- Must appear **before all function declarations**.
- Can contain:
  - Variable declarations: `type name` or `type name = expr`
  - Constant declarations: `constant type name = expr`
  - Array declarations: `type array name`
- Cannot contain:
  - Function calls
  - Statements
  - Type definitions (those go outside the globals block)
- In the pjass grammar, the globals block is defined as: `GLOBALS newline vardecls ENDGLOBALS`
- If there are no globals, the block can be omitted entirely.

---

## 15. The `set` Statement

### Syntax

```jass
set variable = expression
set arrayVar[indexExpr] = expression
```

### Rules

- `set` is **required** for all variable assignments. Bare `x = 5` is a syntax error.
- `set` is a **statement**, not an expression. You cannot do `set x = set y = 5` or use `set` inside an expression.
- You cannot use `set` on:
  - `constant` variables (error: "Cannot assign to constant")
  - Array variables themselves (error: only element access is valid)
- The right-hand side expression must conform to the variable's type.
- Array index must be an integer expression.
- `set` must appear at the start of a line (after newline).

---

## Appendix: BNF Grammar (Authoritative from pjass)

Key productions from the pjass bison grammar:

```
program         := topscopes globdefs topscopes funcdefns
globals         := 'globals' newline vardecls 'endglobals'
func            := funcbegin localblock codeblock 'endfunction'
funcbegin       := ['constant'] 'function' id 'takes' optparams 'returns' opttype
localblock      := (lvardecl | newline)*
lvardecl        := 'local' vardecl
vardecl         := type id ['=' expr] | type 'array' id
statement       := set | call | ifthenelse | loop | exitwhen | return | debug
set             := 'set' id '=' expr | 'set' id '[' expr ']' '=' expr
call            := 'call' id '(' args? ')'
ifthenelse      := 'if' expr 'then' newline codeblock elsifseq elseseq 'endif'
loop            := 'loop' newline codeblock 'endloop'
exitwhen        := 'exitwhen' expr
return          := 'return' expr?
debug           := 'debug' (set | call | ifthenelse | loop)
```

---

## Codegen Checklist for Glass Compiler

When generating JASS from Glass, ensure:

1. [ ] All locals are emitted before any statements in each function
2. [ ] Every assignment uses `set` keyword
3. [ ] Every standalone function call uses `call` keyword
4. [ ] Functions are emitted in dependency order (callee before caller)
5. [ ] One statement per line; no line continuations
6. [ ] Exactly one `globals` block (or none)
7. [ ] Globals block appears before all functions
8. [ ] Type declarations appear before globals and functions
9. [ ] String literals do not exceed 1023 characters
10. [ ] Array indices are within 0-8191
11. [ ] No `code array` declarations
12. [ ] Identifier names do not collide with the 33 reserved words
13. [ ] No forward function references (except self-recursion)
14. [ ] Handle locals are set to `null` before function return
15. [ ] Operator precedence: remember `or` binds tighter than `and`
16. [ ] All expressions in `if`/`elseif`/`exitwhen` are boolean-typed
17. [ ] Functions used as `code` values take `nothing`
18. [ ] `constant` globals are initialized
19. [ ] No bare `\x` escapes in strings except `\b`, `\t`, `\n`, `\f`, `\r`, `\"`, `\\`
20. [ ] Each line ends with newline (including last line of file)
