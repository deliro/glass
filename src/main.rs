mod ast;
mod beta;
mod closures;
mod codegen;
#[cfg(test)]
mod codegen_tests;
mod const_prop;
mod exhaustive;
mod free_vars;
mod infer;
mod inline;
mod jass_parser;
mod lift;
mod linearity;
mod lsp;
mod lua_codegen;
mod lua_runtime;
mod modules;
mod mono;
mod optimize;
mod parser;
mod resolve_const_patterns;
mod runtime;
mod suggest;
mod tco;
mod token;
mod type_env;
mod type_repr;
mod types;
mod unify;

use clap::{Parser as ClapParser, Subcommand, ValueEnum};
use codegen::JassCodegen;
use lua_codegen::LuaCodegen;
use miette::{LabeledSpan, MietteDiagnostic, Report, Severity};
use parser::Parser;
use token::Lexer;
use types::TypeRegistry;

/// Compilation target.
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum Target {
    /// JASS (Warcraft 3 classic scripting language)
    #[default]
    Jass,
    /// Lua (Warcraft 3 Reforged scripting language)
    Lua,
}

#[derive(ClapParser)]
#[command(name = "glass", about = "Glass → JASS/Lua compiler", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Input .glass file to compile
    #[arg(value_name = "INPUT")]
    input: Option<String>,

    /// Output file (defaults to stdout)
    #[arg(short, long, value_name = "OUTPUT")]
    output: Option<String>,

    /// Skip type checking
    #[arg(long)]
    no_check: bool,

    /// Compilation target
    #[arg(long, value_enum, default_value_t = Target::Jass)]
    target: Target,

    /// Disable name mangling (emit readable glass_* names)
    #[arg(long)]
    no_mangle: bool,

    /// Keep blank lines and comments in output
    #[arg(long)]
    no_strip: bool,

    /// Disable tail call optimization
    #[arg(long)]
    no_tco: bool,

    /// Disable lambda lifting
    #[arg(long)]
    no_lift: bool,

    /// Disable function inlining
    #[arg(long)]
    no_inline: bool,

    /// Disable beta reduction (inline immediately-applied lambdas)
    #[arg(long)]
    no_beta: bool,

    /// Disable constant propagation for let bindings
    #[arg(long)]
    no_const_prop: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Generate Glass bindings from a JASS common.j file
    GenBindings {
        /// Path to common.j
        #[arg(value_name = "COMMON_J")]
        jass_file: String,
    },
    /// Type-check a .glass file without compiling
    Check {
        /// Input .glass file
        #[arg(value_name = "INPUT")]
        input: String,
    },
    /// Start LSP server on stdin/stdout
    Lsp,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::GenBindings { jass_file }) => cmd_gen_bindings(&jass_file),
        Some(Command::Check { input }) => cmd_check(&input),
        Some(Command::Lsp) => lsp::run_lsp(),
        None => {
            let Some(input) = cli.input else {
                eprintln!("Usage: glass <INPUT> [-o OUTPUT]");
                eprintln!("       glass gen-bindings <COMMON_J>");
                eprintln!("       glass check <INPUT>");
                std::process::exit(1);
            };
            let opt = optimize::OptFlags {
                mangle: !cli.no_mangle,
                strip: !cli.no_strip,
                tco: !cli.no_tco,
                lift: !cli.no_lift,
                inline: !cli.no_inline,
                beta: !cli.no_beta,
                const_prop: !cli.no_const_prop,
            };
            cmd_compile(
                &input,
                cli.output.as_deref(),
                cli.no_check,
                cli.target,
                &opt,
            );
        }
    }
}

fn cmd_gen_bindings(jass_file: &str) {
    let source = read_file(jass_file);
    let sdk = jass_parser::JassSdk::parse(&source);
    print!("{}", sdk.generate_glass_bindings());
}

fn cmd_check(input: &str) {
    let source = read_file(input);
    let module = parse_source(input, &source);
    let (mut module, imports, _imported_count, _) = resolve_imports(input, module);
    resolve_const_patterns::resolve_const_patterns(&mut module);
    let error_count = run_checks(input, &source, &module, &imports);
    if error_count > 0 {
        eprintln!("{} error(s) found", error_count);
        std::process::exit(1);
    }
    eprintln!("No errors found.");
}

fn cmd_compile(
    input: &str,
    output: Option<&str>,
    no_check: bool,
    target: Target,
    opt: &optimize::OptFlags,
) {
    let source = read_file(input);
    let module = parse_source(input, &source);
    let (mut module, imports, imported_count, def_module_map) = resolve_imports(input, module);

    resolve_const_patterns::resolve_const_patterns(&mut module);

    // Always run inference (needed for type_map in codegen)
    let mut inferencer = infer::Inferencer::new();
    let infer_result = inferencer.infer_module_with_imports(&module, &imports, &def_module_map);

    if !no_check {
        let error_count = run_checks_with_result(
            input,
            &source,
            &module,
            &imports,
            &infer_result,
            &inferencer,
            imported_count,
        );
        if error_count > 0 {
            std::process::exit(1);
        }
    }

    // Optimizations
    if opt.tco {
        tco::apply_tco(&mut module);
    }
    if opt.lift {
        lift::apply_lambda_lifting(&mut module);
    }
    if opt.beta {
        beta::apply_beta_reduction(&mut module);
    }
    if opt.const_prop {
        const_prop::apply_const_propagation(&mut module);
    }
    if opt.inline {
        inline::apply_inlining(&mut module);
    }

    // Codegen
    let type_registry = TypeRegistry::from_module(&module);
    let mut lambda_collector = closures::LambdaCollector::new();
    lambda_collector.collect_module(&module);

    // Build name table from AST frequency analysis (before codegen consumes the data)
    let name_table = if opt.mangle {
        Some(optimize::build_name_table(
            &module,
            &type_registry,
            &lambda_collector.lambdas,
        ))
    } else {
        None
    };

    let mut result = match target {
        Target::Jass => JassCodegen::new(
            type_registry,
            lambda_collector.lambdas,
            infer_result.type_map,
            inferencer.type_param_vars.clone(),
        )
        .generate(&module, &imports),
        Target::Lua => LuaCodegen::new(
            type_registry,
            lambda_collector.lambdas,
            infer_result.type_map,
            inferencer.type_param_vars.clone(),
        )
        .generate(&module, &imports),
    };

    if let Some(table) = &name_table {
        result = table.apply(&result);
    }
    if opt.strip {
        result = optimize::strip_whitespace_and_comments(&result);
    }
    result = result.replace('\n', "\r\n");

    match output {
        Some(path) => {
            if let Err(e) = std::fs::write(path, &result) {
                eprintln!("Error writing {}: {}", path, e);
                std::process::exit(1);
            }
        }
        None => print!("{}", result),
    }
}

fn run_checks_with_result(
    filename: &str,
    source: &str,
    module: &ast::Module,
    _imports: &[modules::ResolvedImport],
    infer_result: &infer::InferResult,
    inferencer: &infer::Inferencer,
    imported_count: usize,
) -> usize {
    let named_src = miette::NamedSource::new(filename, source.to_string());
    let mut error_count = 0;

    for e in &infer_result.errors {
        emit_error(&e.message, e.span, &named_src);
        error_count += 1;
    }

    // Exhaustiveness — only check user definitions, not imported ones
    let exhaustiveness_warnings =
        exhaustive::check_exhaustiveness(module, &inferencer.constructors, imported_count);
    for w in &exhaustiveness_warnings {
        emit_warning(&w.message, w.span, "this case expression", &named_src);
    }

    // Monomorphization (collect — no errors, just info)
    let _mono_types = mono::collect_mono_types(module, inferencer);

    // Linearity
    let handle_types = build_handle_types(filename);
    let linearity_result = linearity::LinearityChecker::new(handle_types).check_module(module);
    for w in &linearity_result.warnings {
        emit_warning(&w.message, w.span, "this handle", &named_src);
    }
    for e in &linearity_result.errors {
        emit_error(&e.message, e.span, &named_src);
        error_count += 1;
    }

    // Local fn safety
    let local_fn_errors = linearity::check_local_fns(module);
    for e in &local_fn_errors {
        emit_error(&e.message, e.span, &named_src);
        error_count += 1;
    }

    error_count
}

fn run_checks(
    filename: &str,
    source: &str,
    module: &ast::Module,
    imports: &[modules::ResolvedImport],
) -> usize {
    let mut inferencer = infer::Inferencer::new();
    let empty_map = std::collections::HashMap::new();
    let infer_result = inferencer.infer_module_with_imports(module, imports, &empty_map);
    let imported_count: usize = imports.iter().map(|i| i.definitions.len()).sum();
    run_checks_with_result(
        filename,
        source,
        module,
        imports,
        &infer_result,
        &inferencer,
        imported_count,
    )
}

fn emit_error(message: &str, span: crate::token::Span, src: &miette::NamedSource<String>) {
    let diag =
        MietteDiagnostic::new(message).with_label(LabeledSpan::at(span.start..span.end, "here"));
    eprintln!("{:?}", Report::new(diag).with_source_code(src.clone()));
}

fn emit_warning(
    message: &str,
    span: crate::token::Span,
    label: &str,
    src: &miette::NamedSource<String>,
) {
    let diag = MietteDiagnostic::new(message)
        .with_severity(Severity::Warning)
        .with_label(LabeledSpan::at(span.start..span.end, label));
    eprintln!("{:?}", Report::new(diag).with_source_code(src.clone()));
}

fn build_handle_types(input: &str) -> std::collections::HashSet<String> {
    let input_path = std::path::Path::new(input);
    let mut candidates = Vec::new();
    if let Some(parent) = input_path.parent() {
        candidates.push(parent.join("sdk").join("common.j"));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("sdk").join("common.j"));
    }
    for path in candidates {
        if let Ok(source) = std::fs::read_to_string(&path) {
            return jass_parser::JassSdk::parse(&source).handle_type_names();
        }
    }
    std::collections::HashSet::new()
}

fn resolve_imports(
    input: &str,
    module: ast::Module,
) -> (
    ast::Module,
    Vec<modules::ResolvedImport>,
    usize,
    std::collections::HashMap<usize, String>,
) {
    let input_path = std::path::Path::new(input);
    let mut resolver = modules::ModuleResolver::new(input_path);
    match resolver.resolve_module(&module) {
        Ok((resolved, imports, imported_count, def_module_map)) => {
            (resolved, imports, imported_count, def_module_map)
        }
        Err(errors) => {
            for e in &errors {
                eprintln!("  × {}", e.message);
            }
            std::process::exit(1);
        }
    }
}

fn read_file(path: &str) -> String {
    match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", path, e);
            std::process::exit(1);
        }
    }
}

fn parse_source(filename: &str, source: &str) -> ast::Module {
    let tokens = match Lexer::tokenize(source) {
        Ok(t) => t,
        Err(e) => {
            let named_src = miette::NamedSource::new(filename, source.to_string());
            let diag = MietteDiagnostic::new(format!("unexpected character: {:?}", e.text))
                .with_label(LabeledSpan::at(e.span.start..e.span.end, "here"));
            eprintln!("{:?}", Report::new(diag).with_source_code(named_src));
            std::process::exit(1);
        }
    };
    let mut parser = Parser::new(tokens);
    let output = parser.parse_module();
    if !output.errors.is_empty() {
        let named_src = miette::NamedSource::new(filename, source.to_string());
        for e in &output.errors {
            let diag = MietteDiagnostic::new(&e.message)
                .with_label(LabeledSpan::at(e.span.start..e.span.end, "here"));
            eprintln!(
                "{:?}",
                Report::new(diag).with_source_code(named_src.clone())
            );
        }
        std::process::exit(1);
    }
    output.module
}
