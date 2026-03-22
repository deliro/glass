use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::closures::LambdaInfo;
use crate::jass_parser::JassSdk;
use crate::types::TypeRegistry;

/// Compiler optimization flags. All optimizations are ON by default (opt-out).
#[derive(Clone, Debug)]
pub struct OptFlags {
    pub mangle: bool,
    pub strip: bool,
    pub tco: bool,
    pub lift: bool,
    pub inline: bool,
    pub beta: bool,
    pub const_prop: bool,
}

impl Default for OptFlags {
    fn default() -> Self {
        Self {
            mangle: true,
            strip: true,
            tco: true,
            lift: true,
            inline: true,
            beta: true,
            const_prop: true,
        }
    }
}

/// Pre-computed name mapping: glass_* identifier → short name.
/// Built from AST frequency analysis, not from output scanning.
pub struct NameTable {
    map: HashMap<String, String>,
}

impl NameTable {
    /// Apply the name table to generated code.
    /// Walks the output, finds glass_-prefixed identifiers, and replaces
    /// those present in the table with their short names.
    /// Works on bytes since JASS/Lua output is guaranteed ASCII.
    pub fn apply(&self, code: &str) -> String {
        let src = code.as_bytes();
        let mut result = Vec::with_capacity(src.len());
        let mut i = 0;

        while let Some(&b) = src.get(i) {
            // Skip double-quoted string literals
            if b == b'"' || b == b'\'' {
                let end = skip_string(src, i, b);
                result.extend_from_slice(src.get(i..end).unwrap_or_default());
                i = end;
                continue;
            }

            // Check for glass_ prefix at identifier start
            if b == b'g'
                && src.get(i..i + 6) == Some(b"glass_")
                && (i == 0 || !src.get(i - 1).is_some_and(|b| is_ident_byte(*b)))
            {
                let start = i;
                i += 6;
                while src.get(i).is_some_and(|b| is_ident_byte(*b)) {
                    i += 1;
                }
                let ident = &code[start..i];
                if let Some(short) = self.map.get(ident) {
                    result.extend_from_slice(short.as_bytes());
                } else {
                    result.extend_from_slice(ident.as_bytes());
                }
                continue;
            }

            result.push(b);
            i += 1;
        }

        // All manipulations are on ASCII bytes, so this is always valid UTF-8
        String::from_utf8(result).unwrap_or_default()
    }
}

/// Remove blank lines, comment-only lines, and collapse multiple blank lines.
/// Also strips trailing whitespace from each line.
pub fn strip_whitespace_and_comments(code: &str) -> String {
    let mut result = String::with_capacity(code.len());
    for line in code.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        result.push_str(line.trim_end());
        result.push('\n');
    }
    result
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Skip a string literal (double or single quoted), returning the index after the closing quote.
fn skip_string(src: &[u8], start: usize, quote: u8) -> usize {
    let mut i = start + 1;
    while let Some(&b) = src.get(i) {
        if b == b'\\' {
            i += 2; // skip escaped char
            continue;
        }
        if b == quote {
            return i + 1;
        }
        i += 1;
    }
    i
}

/// Build a NameTable by analyzing the AST for name frequencies.
pub fn build_name_table(
    module: &Module,
    type_registry: &TypeRegistry,
    lambdas: &[LambdaInfo],
) -> NameTable {
    let mut freq: HashMap<String, usize> = HashMap::new();

    // --- Collect frequencies from AST ---

    // Build variant → type name mapping
    let mut variant_to_type: HashMap<String, String> = HashMap::new();
    for (type_name, info) in &type_registry.types {
        for v in &info.variants {
            variant_to_type.insert(v.name.clone(), type_name.clone());
        }
    }

    // Walk all definitions
    for def in &module.definitions {
        match def {
            Definition::Function(f) => {
                // The function definition itself appears once
                bump(&mut freq, &format!("glass_{}", f.name), 1);
                // Walk the body for references
                count_expr(&f.body, &mut freq, &variant_to_type, type_registry);
            }
            Definition::Type(t) => {
                // Type infrastructure names — each appears at least once (declaration)
                if t.constructors.len() > 1 || !t.is_struct {
                    bump(&mut freq, &format!("glass_{}_tag", t.name), 1);
                }
                bump(&mut freq, &format!("glass_{}_alloc", t.name), 1);
                bump(&mut freq, &format!("glass_{}_dealloc", t.name), 1);
                bump(&mut freq, &format!("glass_{}_free", t.name), 1);
                bump(&mut freq, &format!("glass_{}_free_top", t.name), 1);
                bump(&mut freq, &format!("glass_{}_count", t.name), 1);

                for v in &t.constructors {
                    bump(&mut freq, &format!("glass_TAG_{}", v.name), 1);
                    bump(&mut freq, &format!("glass_new_{}", v.name), 1);
                    for field in &v.fields {
                        let arr = format!("glass_{}_{}_{}", t.name, v.name, field.name);
                        bump(&mut freq, &arr, 1);
                        let getter = format!("glass_get_{}_{}_{}", t.name, v.name, field.name);
                        bump(&mut freq, &getter, 1);
                    }
                }
            }
            Definition::Const(c) => {
                // Constants are inlined, but the name might still appear
                bump(&mut freq, &format!("glass_{}", c.name), 1);
            }
            _ => {}
        }
    }

    // Closure infrastructure
    for lambda in lambdas {
        let id = lambda.id;
        let arity = lambda.params.len();
        bump(&mut freq, &format!("glass_dispatch_{}", arity), 1);
        if !lambda.captures.is_empty() {
            bump(&mut freq, &format!("glass_clos{}_alloc", id), 1);
            bump(&mut freq, &format!("glass_clos{}_dealloc", id), 1);
            bump(&mut freq, &format!("glass_clos{}_free", id), 1);
            bump(&mut freq, &format!("glass_clos{}_free_top", id), 1);
            bump(&mut freq, &format!("glass_clos{}_count", id), 1);
            for cap in &lambda.captures {
                bump(&mut freq, &format!("glass_clos{}_{}", id, cap.name), 1);
            }
        }
    }

    // Fixed infrastructure names
    bump(&mut freq, "glass_i2b", 1);
    bump(&mut freq, "glass_dispatch_void", 1);
    bump(&mut freq, "glass_panic", 1);

    // List infrastructure
    for lt in &type_registry.list_types {
        let list_name = format!("List_{}", lt);
        bump(&mut freq, &format!("glass_{}_cons", list_name), 1);
        bump(&mut freq, &format!("glass_{}_head", list_name), 1);
        bump(&mut freq, &format!("glass_{}_tail", list_name), 1);
        bump(&mut freq, &format!("glass_{}_alloc", list_name), 1);
        bump(&mut freq, &format!("glass_{}_dealloc", list_name), 1);
        bump(&mut freq, &format!("glass_{}_free", list_name), 1);
        bump(&mut freq, &format!("glass_{}_free_top", list_name), 1);
        bump(&mut freq, &format!("glass_{}_count", list_name), 1);
    }

    // --- Collect reserved names (must not collide) ---
    let reserved = collect_reserved_names(module);

    // --- Sort by frequency descending, assign shortest names ---
    let mut names_by_freq: Vec<(String, usize)> = freq.into_iter().collect();
    names_by_freq.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    let mut map = HashMap::new();
    let mut name_gen = ShortNameGen::new(reserved);
    for (glass_name, _freq) in &names_by_freq {
        let short = name_gen.next();
        map.insert(glass_name.clone(), short);
    }

    NameTable { map }
}

/// Walk an expression tree counting identifier usage frequencies.
fn count_expr(
    expr: &Spanned<Expr>,
    freq: &mut HashMap<String, usize>,
    variant_to_type: &HashMap<String, String>,
    type_registry: &TypeRegistry,
) {
    match &expr.node {
        Expr::Call { function, args } => {
            // Count the function reference
            if let Expr::Var(name) = &function.node {
                bump(freq, &format!("glass_{}", name), 1);
            }
            count_expr(function, freq, variant_to_type, type_registry);
            for arg in args {
                count_expr(arg, freq, variant_to_type, type_registry);
            }
        }
        Expr::MethodCall {
            object,
            method,
            args,
        } => {
            bump(freq, &format!("glass_{}", method), 1);
            count_expr(object, freq, variant_to_type, type_registry);
            for arg in args {
                count_expr(arg, freq, variant_to_type, type_registry);
            }
        }
        Expr::Var(name) => {
            bump(freq, &format!("glass_{}", name), 1);
        }
        Expr::Constructor { name, args } => {
            bump(freq, &format!("glass_new_{}", name), 1);
            bump(freq, &format!("glass_TAG_{}", name), 1);
            // Also bump the type's alloc
            if let Some(type_name) = variant_to_type.get(name) {
                bump(freq, &format!("glass_{}_alloc", type_name), 1);
            }
            for arg in args {
                let e = match arg {
                    ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => e,
                };
                count_expr(e, freq, variant_to_type, type_registry);
            }
        }
        Expr::FieldAccess { object, field } => {
            // We don't know the exact type here without type info,
            // but we bump the field name as it'll appear in getters
            count_expr(object, freq, variant_to_type, type_registry);
            // Approximate: bump glass_get_*_field patterns
            // The exact type is resolved in codegen; we bump a generic form
            bump(freq, &format!("glass_get_{}", field), 1);
        }
        Expr::Case { subject, arms } => {
            count_expr(subject, freq, variant_to_type, type_registry);
            for arm in arms {
                count_pattern(&arm.pattern, freq, variant_to_type, type_registry);
                if let Some(guard) = &arm.guard {
                    count_expr(guard, freq, variant_to_type, type_registry);
                }
                count_expr(&arm.body, freq, variant_to_type, type_registry);
            }
        }
        Expr::Let { value, body, .. } => {
            count_expr(value, freq, variant_to_type, type_registry);
            count_expr(body, freq, variant_to_type, type_registry);
        }
        Expr::BinOp { left, right, .. } | Expr::Pipe { left, right } => {
            count_expr(left, freq, variant_to_type, type_registry);
            count_expr(right, freq, variant_to_type, type_registry);
        }
        Expr::UnaryOp { operand, .. } | Expr::Lambda { body: operand, .. } => {
            count_expr(operand, freq, variant_to_type, type_registry);
        }
        Expr::Block(exprs) | Expr::Tuple(exprs) | Expr::List(exprs) => {
            for e in exprs {
                count_expr(e, freq, variant_to_type, type_registry);
            }
        }
        Expr::ListCons { head, tail } => {
            count_expr(head, freq, variant_to_type, type_registry);
            count_expr(tail, freq, variant_to_type, type_registry);
        }
        Expr::RecordUpdate { base, updates, .. } => {
            count_expr(base, freq, variant_to_type, type_registry);
            for (_, e) in updates {
                count_expr(e, freq, variant_to_type, type_registry);
            }
        }
        Expr::Clone(inner) => {
            count_expr(inner, freq, variant_to_type, type_registry);
        }
        Expr::TcoLoop { body } => {
            count_expr(body, freq, variant_to_type, type_registry);
        }
        Expr::TcoContinue { args } => {
            for (_, val) in args {
                count_expr(val, freq, variant_to_type, type_registry);
            }
        }
        _ => {}
    }
}

/// Count pattern-match references (tag checks, field accesses).
fn count_pattern(
    pat: &Spanned<Pattern>,
    freq: &mut HashMap<String, usize>,
    variant_to_type: &HashMap<String, String>,
    type_registry: &TypeRegistry,
) {
    match &pat.node {
        Pattern::Constructor { name, args } => {
            bump(freq, &format!("glass_TAG_{}", name), 1);
            if let Some(type_name) = variant_to_type.get(name) {
                bump(freq, &format!("glass_{}_tag", type_name), 1);
                // Bump field arrays for positional fields
                if let Some(type_info) = type_registry.types.get(type_name) {
                    for v in &type_info.variants {
                        if v.name == *name {
                            for (i, field) in v.fields.iter().enumerate() {
                                if i < args.len() {
                                    let arr =
                                        format!("glass_{}_{}_{}", type_name, name, field.name);
                                    bump(freq, &arr, 1);
                                }
                            }
                        }
                    }
                }
            }
            for arg in args {
                count_pattern(arg, freq, variant_to_type, type_registry);
            }
        }
        Pattern::ConstructorNamed { name, fields, .. } => {
            bump(freq, &format!("glass_TAG_{}", name), 1);
            if let Some(type_name) = variant_to_type.get(name) {
                bump(freq, &format!("glass_{}_tag", type_name), 1);
                for fp in fields {
                    let getter = format!("glass_get_{}_{}_{}", type_name, name, fp.field_name);
                    bump(freq, &getter, 1);
                }
            }
        }
        Pattern::ListCons { head, tail } => {
            count_pattern(head, freq, variant_to_type, type_registry);
            count_pattern(tail, freq, variant_to_type, type_registry);
        }
        Pattern::Tuple(pats) | Pattern::List(pats) | Pattern::Or(pats) => {
            for p in pats {
                count_pattern(p, freq, variant_to_type, type_registry);
            }
        }
        Pattern::As { pattern, .. } => {
            count_pattern(pattern, freq, variant_to_type, type_registry);
        }
        _ => {}
    }
}

fn bump(freq: &mut HashMap<String, usize>, name: &str, count: usize) {
    *freq.entry(name.to_string()).or_insert(0) += count;
}

/// Collect all names that must be excluded from mangling:
/// JASS keywords + user-defined variable names (params, let bindings, pattern bindings).
fn collect_reserved_names(module: &Module) -> HashSet<String> {
    let mut reserved = HashSet::new();

    // JASS keywords
    for kw in &[
        "function",
        "endfunction",
        "local",
        "set",
        "call",
        "if",
        "then",
        "else",
        "elseif",
        "endif",
        "loop",
        "endloop",
        "exitwhen",
        "return",
        "returns",
        "takes",
        "nothing",
        "globals",
        "endglobals",
        "constant",
        "type",
        "extends",
        "array",
        "true",
        "false",
        "null",
        "and",
        "or",
        "not",
        "debug",
        "native",
        "integer",
        "real",
        "boolean",
        "string",
        "handle",
        "code",
        "do",
        "end",
        "in",
        "for",
        "while",
        "repeat",
        "until",
        "break",
        "nil",
        // Lua keywords too
        "local",
        "function",
        "end",
        "if",
        "then",
        "else",
        "elseif",
        "return",
        "true",
        "false",
        "nil",
        "and",
        "or",
        "not",
        "do",
        "while",
        "for",
        "repeat",
        "until",
        "break",
        "in",
    ] {
        reserved.insert(kw.to_string());
    }

    // Codegen / runtime internal variable names (hardcoded in codegen.rs, runtime.rs, lua_runtime.rs)
    for name in &[
        "i",
        "id",
        "t",
        "u",
        "tt",
        "sfx",
        "hid",
        "current",
        "fx_id",
        "fx_tag",
        "closure_id",
        "msg_result",
        "cb",
        "expired",
        "handler",
        "msg",
        "result",
        "sub",
        "subs",
        "target",
        "name",
        "tag",
        "cid",
    ] {
        reserved.insert(name.to_string());
    }

    let stub = include_str!("../tests/common_stub.j");
    let sdk = JassSdk::parse(stub);
    for native in &sdk.natives {
        reserved.insert(native.name.clone());
    }
    for ty in sdk.types.keys() {
        reserved.insert(ty.clone());
    }

    for def in &module.definitions {
        match def {
            Definition::Function(f) => {
                for p in &f.params {
                    reserved.insert(p.name.clone());
                }
                collect_vars_from_expr(&f.body, &mut reserved);
            }
            Definition::External(e) => {
                for p in &e.params {
                    reserved.insert(p.name.clone());
                }
            }
            _ => {}
        }
    }

    reserved
}

/// Collect all variable names introduced in an expression (let, case, lambda params).
fn collect_vars_from_expr(expr: &Spanned<Expr>, vars: &mut HashSet<String>) {
    match &expr.node {
        Expr::Let {
            pattern,
            value,
            body,
            ..
        } => {
            collect_vars_from_pattern(&pattern.node, vars);
            collect_vars_from_expr(value, vars);
            collect_vars_from_expr(body, vars);
        }
        Expr::Case { subject, arms } => {
            collect_vars_from_expr(subject, vars);
            for arm in arms {
                collect_vars_from_pattern(&arm.pattern.node, vars);
                if let Some(guard) = &arm.guard {
                    collect_vars_from_expr(guard, vars);
                }
                collect_vars_from_expr(&arm.body, vars);
            }
        }
        Expr::Lambda { params, body, .. } => {
            for p in params {
                vars.insert(p.name.clone());
            }
            collect_vars_from_expr(body, vars);
        }
        Expr::Call { function, args } => {
            collect_vars_from_expr(function, vars);
            for a in args {
                collect_vars_from_expr(a, vars);
            }
        }
        Expr::MethodCall { object, args, .. } => {
            collect_vars_from_expr(object, vars);
            for a in args {
                collect_vars_from_expr(a, vars);
            }
        }
        Expr::BinOp { left, right, .. }
        | Expr::Pipe { left, right }
        | Expr::ListCons {
            head: left,
            tail: right,
        } => {
            collect_vars_from_expr(left, vars);
            collect_vars_from_expr(right, vars);
        }
        Expr::UnaryOp { operand, .. } | Expr::Clone(operand) => {
            collect_vars_from_expr(operand, vars);
        }
        Expr::Block(exprs) | Expr::List(exprs) | Expr::Tuple(exprs) => {
            for e in exprs {
                collect_vars_from_expr(e, vars);
            }
        }
        Expr::FieldAccess { object, .. } => {
            collect_vars_from_expr(object, vars);
        }
        Expr::RecordUpdate { base, updates, .. } => {
            collect_vars_from_expr(base, vars);
            for (_, e) in updates {
                collect_vars_from_expr(e, vars);
            }
        }
        Expr::Constructor { args, .. } => {
            for arg in args {
                let e = match arg {
                    ConstructorArg::Positional(e) | ConstructorArg::Named(_, e) => e,
                };
                collect_vars_from_expr(e, vars);
            }
        }
        Expr::TcoLoop { body } => {
            collect_vars_from_expr(body, vars);
        }
        Expr::TcoContinue { args } => {
            for (_, val) in args {
                collect_vars_from_expr(val, vars);
            }
        }
        _ => {}
    }
}

fn collect_vars_from_pattern(pat: &Pattern, vars: &mut HashSet<String>) {
    match pat {
        Pattern::Var(name) => {
            vars.insert(name.clone());
        }
        Pattern::Constructor { args, .. } => {
            for a in args {
                collect_vars_from_pattern(&a.node, vars);
            }
        }
        Pattern::ConstructorNamed { fields, .. } => {
            for fp in fields {
                if let Some(p) = &fp.pattern {
                    collect_vars_from_pattern(&p.node, vars);
                } else {
                    vars.insert(fp.field_name.clone());
                }
            }
        }
        Pattern::Tuple(pats) | Pattern::List(pats) | Pattern::Or(pats) => {
            for p in pats {
                collect_vars_from_pattern(&p.node, vars);
            }
        }
        Pattern::ListCons { head, tail } => {
            collect_vars_from_pattern(&head.node, vars);
            collect_vars_from_pattern(&tail.node, vars);
        }
        Pattern::As { pattern, name } => {
            vars.insert(name.clone());
            collect_vars_from_pattern(&pattern.node, vars);
        }
        _ => {}
    }
}

/// Generates short names (a, b, ..., z, A, ..., Z, aa, ab, ...) skipping reserved names.
struct ShortNameGen {
    reserved: HashSet<String>,
    counter: usize,
}

impl ShortNameGen {
    fn new(reserved: HashSet<String>) -> Self {
        Self {
            reserved,
            counter: 0,
        }
    }

    fn next(&mut self) -> String {
        loop {
            let name = encode_name(self.counter);
            self.counter += 1;
            if !self.reserved.contains(&name) {
                return name;
            }
        }
    }
}

/// Encode a counter into a short name: a, b, ..., z, A, ..., Z, aa, ab, ...
fn encode_name(mut n: usize) -> String {
    const ALPHABET: &[u8; 52] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut chars = Vec::new();
    loop {
        let c = n % 52;
        chars.push(*ALPHABET.get(c).unwrap_or(&b'a') as char);
        n /= 52;
        if n == 0 {
            break;
        }
        n -= 1; // bijective base-52
    }
    chars.reverse();
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_name() {
        assert_eq!(encode_name(0), "a");
        assert_eq!(encode_name(1), "b");
        assert_eq!(encode_name(25), "z");
        assert_eq!(encode_name(26), "A");
        assert_eq!(encode_name(51), "Z");
        assert_eq!(encode_name(52), "aa");
        assert_eq!(encode_name(53), "ab");
        assert_eq!(encode_name(103), "aZ");
        assert_eq!(encode_name(104), "ba");
    }

    #[test]
    fn test_name_table_apply() {
        let mut map = HashMap::new();
        map.insert("glass_foo".to_string(), "a".to_string());
        map.insert("glass_bar".to_string(), "b".to_string());
        let table = NameTable { map };

        let input = "set glass_foo = glass_bar(glass_foo)\n";
        let output = table.apply(input);
        assert_eq!(output, "set a = b(a)\n");
    }

    #[test]
    fn test_name_table_skips_unknown() {
        let map = HashMap::new();
        let table = NameTable { map };

        let input = "glass_unknown stays";
        let output = table.apply(input);
        assert_eq!(output, "glass_unknown stays");
    }

    #[test]
    fn test_name_table_skips_strings() {
        let mut map = HashMap::new();
        map.insert("glass_foo".to_string(), "a".to_string());
        let table = NameTable { map };

        let input = r#"glass_foo("glass_foo")"#;
        let output = table.apply(input);
        assert_eq!(output, r#"a("glass_foo")"#);
    }

    #[test]
    fn test_name_table_no_mid_identifier() {
        let mut map = HashMap::new();
        map.insert("glass_foo".to_string(), "a".to_string());
        let table = NameTable { map };

        let input = "xglass_foo";
        let output = table.apply(input);
        assert_eq!(output, "xglass_foo");
    }

    #[test]
    fn test_reserved_names_skipped() {
        let mut reserved = HashSet::new();
        reserved.insert("a".to_string());
        reserved.insert("b".to_string());
        let mut ngen = ShortNameGen::new(reserved);
        assert_eq!(ngen.next(), "c"); // skips a, b
        assert_eq!(ngen.next(), "d");
    }

    #[test]
    fn test_frequency_ordering() {
        // Higher frequency names should get shorter identifiers
        let mut freq = vec![
            ("glass_rare".to_string(), 1usize),
            ("glass_common".to_string(), 100),
            ("glass_medium".to_string(), 10),
        ];
        freq.sort_by(|a, b| b.1.cmp(&a.1));

        let reserved = HashSet::new();
        let mut ngen = ShortNameGen::new(reserved);
        let mut map = HashMap::new();
        for (name, _) in &freq {
            map.insert(name.clone(), ngen.next());
        }

        assert_eq!(map["glass_common"], "a"); // most frequent → shortest
        assert_eq!(map["glass_medium"], "b");
        assert_eq!(map["glass_rare"], "c");
    }

    #[test]
    fn test_mangled_names_never_collide_with_reserved() {
        // Simulate: module has params a, b, c, x, y, data, p, val
        let mut reserved = HashSet::new();
        for name in &["a", "b", "c", "x", "y", "data", "p", "val", "id"] {
            reserved.insert(name.to_string());
        }
        // Also JASS keywords
        for kw in &[
            "if", "set", "call", "local", "return", "function", "integer", "in", "do",
        ] {
            reserved.insert(kw.to_string());
        }

        let mut ngen = ShortNameGen::new(reserved.clone());
        // Generate 200 names — none should be in the reserved set
        for _ in 0..200 {
            let name = ngen.next();
            assert!(
                !reserved.contains(&name),
                "mangled name '{}' collides with reserved name",
                name
            );
        }
    }

    /// Parse a Glass module and verify that every mangled name is absent
    /// from the set of local variable names used anywhere in that module.
    #[test]
    fn test_build_name_table_no_conflict_with_locals() {
        use crate::closures::LambdaCollector;
        use crate::parser::Parser;
        use crate::token::Lexer;
        use crate::types::TypeRegistry;

        // Module with many short variable names to stress the reserved set.
        // `a` through `h` are all used as params/let-bindings/pattern-bindings.
        let source = r#"
pub struct Pair { a: Int, b: Int }

pub fn f(a: Int, b: Int) -> Pair {
    Pair { a: a, b: b }
}

pub fn g(c: Int, d: Int) -> Int {
    let e = c + d
    e
}

pub fn h(p: Pair) -> Int {
    p.a + p.b
}
"#;
        let tokens = Lexer::tokenize(source);
        let module = Parser::new(tokens).parse_module().unwrap();
        let type_registry = TypeRegistry::from_module(&module);
        let mut lc = LambdaCollector::new();
        lc.collect_module(&module);

        let table = build_name_table(&module, &type_registry, &lc.lambdas);
        let reserved = collect_reserved_names(&module);

        // Every mangled name must be outside the reserved set
        for (glass_name, short_name) in &table.map {
            assert!(
                !reserved.contains(short_name),
                "mangled name '{}' (from '{}') collides with a local variable or keyword",
                short_name,
                glass_name,
            );
        }

        // Specifically: 'a'..'e' and 'p' are used as local vars, so no
        // mangled name should be any of them.
        let local_vars: HashSet<&str> = ["a", "b", "c", "d", "e", "p"].iter().copied().collect();
        for short_name in table.map.values() {
            assert!(
                !local_vars.contains(short_name.as_str()),
                "mangled name '{}' collides with a local variable name",
                short_name,
            );
        }
    }

    #[test]
    fn test_global_array_no_conflict_with_local_param() {
        // If a function has param 'x' and a global is mangled to some name,
        // the global's mangled name must not be 'x'
        let mut map = HashMap::new();
        map.insert("glass_Data_val".to_string(), "q".to_string());
        let table = NameTable { map };

        // JASS code where 'x' is a local param and glass_Data_val is a global array
        let input = "function glass_foo takes integer x returns integer\n    \
                     return glass_Data_val[x]\nendfunction\n";
        let output = table.apply(input);
        // glass_Data_val should become 'q', not 'x'
        assert!(output.contains("return q[x]"), "got: {}", output);
        // 'x' as param should stay untouched
        assert!(output.contains("takes integer x"), "got: {}", output);
    }

    #[test]
    fn test_jass_natives_in_reserved() {
        use crate::parser::Parser;
        use crate::token::Lexer;

        let source = "fn test(x: Int) -> Int { x }";
        let tokens = Lexer::tokenize(source);
        let module = Parser::new(tokens).parse_module().unwrap();
        let reserved = collect_reserved_names(&module);

        for native_name in &[
            "CreateUnit",
            "RemoveUnit",
            "ShowUnit",
            "KillUnit",
            "CreateTimer",
            "DestroyTimer",
            "TimerStart",
            "CreateGroup",
            "DestroyGroup",
            "CreateTrigger",
            "DestroyTrigger",
            "DisplayTimedTextToPlayer",
            "GetHandleId",
            "I2S",
            "R2S",
            "S2I",
            "Player",
        ] {
            assert!(
                reserved.contains(*native_name),
                "JASS native '{}' missing from reserved set",
                native_name,
            );
        }
    }
}
