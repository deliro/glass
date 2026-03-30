// Module resolution for Glass.
//
// Resolves `import path/to/module` to actual .glass files,
// parses them, and builds a module map for the type checker.

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::{Definition, ImportDef, Module};
use crate::parser::Parser;
use crate::token::Lexer;

/// A resolved import: the module name, its definitions, and what names to expose.
#[derive(Debug, Clone)]
pub struct ResolvedImport {
    /// Module name (last segment of path, e.g. "option" from "import option")
    pub module_name: String,
    /// All definitions from the module
    pub definitions: Vec<Definition>,
    /// Names to expose unqualified (from selective imports)
    pub unqualified: HashSet<String>,
    /// Whether qualified access (module.Name) is available
    pub qualified: bool,
}

/// Module resolver configuration.
pub struct ModuleResolver {
    search_paths: Vec<PathBuf>,
    /// Cache: canonical path → parsed module definitions
    cache: HashMap<PathBuf, Vec<Definition>>,
    resolving: HashSet<PathBuf>,
}

/// Maps def index in merged module → source module name (for qualified access).
pub type DefModuleMap = HashMap<usize, String>;

#[derive(Debug)]
pub struct ModuleError {
    pub message: String,
    pub import: ImportDef,
}

impl ModuleResolver {
    pub fn new(input_file: &Path) -> Self {
        let mut search_paths = Vec::new();

        if let Some(parent) = input_file.parent() {
            search_paths.push(parent.to_path_buf());
            let sdk = parent.join("sdk");
            if sdk.is_dir() {
                search_paths.push(sdk);
            }
        }

        if let Ok(cwd) = std::env::current_dir() {
            let sdk = cwd.join("sdk");
            if sdk.is_dir() {
                search_paths.push(sdk);
            }
        }

        Self {
            search_paths,
            cache: HashMap::new(),
            resolving: HashSet::new(),
        }
    }

    /// Resolve all imports in a module.
    /// Returns:
    /// - A new Module with all imported definitions merged in (for codegen)
    /// - A list of ResolvedImports (for type checker namespacing)
    /// - The count of imported definitions (for skipping in exhaustiveness checks)
    pub fn resolve_module(
        &mut self,
        module: &Module,
    ) -> Result<(Module, Vec<ResolvedImport>, usize, DefModuleMap), Vec<ModuleError>> {
        let mut errors = Vec::new();
        let mut resolved_imports = Vec::new();
        let mut all_imported_defs: Vec<Definition> = Vec::new();
        let mut seen_defs: HashSet<String> = HashSet::new();

        // Collect imports
        let imports: Vec<ImportDef> = module
            .definitions
            .iter()
            .filter_map(|d| {
                if let Definition::Import(imp) = d {
                    Some(imp.clone())
                } else {
                    None
                }
            })
            .collect();

        // Track which module each imported def comes from (by index in all_defs)
        let mut def_module_map: HashMap<usize, String> = HashMap::new();

        for imp in &imports {
            match self.resolve_single_import(imp) {
                Ok(resolved) => {
                    let module_name = &resolved.module_name;
                    for def in &resolved.definitions {
                        let name = def_name(def);
                        let dedup_key = match name {
                            Some(n) => format!("{}.{}", module_name, n),
                            None => format!("{}.__anon_{}", module_name, all_imported_defs.len()),
                        };
                        let should_add = if seen_defs.insert(dedup_key) {
                            true
                        } else if let Definition::External(e) = def {
                            if let Some(ref src) = e.source_module {
                                let src_key = name.map(|n| format!("{}.{}", src, n));
                                src_key.map_or(false, |k| seen_defs.insert(k))
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        if should_add {
                            if resolved.qualified {
                                def_module_map.insert(all_imported_defs.len(), module_name.clone());
                            }
                            let mut def = def.clone();
                            if resolved.qualified {
                                if let Definition::External(ref mut e) = def {
                                    if e.source_module.is_none() {
                                        e.source_module = Some(module_name.clone());
                                    }
                                }
                            }
                            all_imported_defs.push(def);
                        }
                    }
                    resolved_imports.push(resolved);
                }
                Err(e) => errors.push(e),
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        // Build merged module: imported defs first, then user defs
        let mut all_defs = all_imported_defs;
        let imported_count = all_defs.len();
        for def in &module.definitions {
            if !matches!(def, Definition::Import(_)) {
                all_defs.push(def.clone());
            }
        }

        Ok((
            Module {
                definitions: all_defs,
            },
            resolved_imports,
            imported_count,
            def_module_map,
        ))
    }

    fn resolve_single_import(&mut self, imp: &ImportDef) -> Result<ResolvedImport, ModuleError> {
        let module_name = imp
            .alias
            .clone()
            .unwrap_or_else(|| imp.path.last().cloned().unwrap_or_default());

        let defs = self.load_module(imp)?;

        // Determine what to expose
        let (unqualified, qualified) = match &imp.items {
            None => {
                // `import option` → qualified only (option.X)
                (HashSet::new(), true)
            }
            Some(items) => {
                let mut unqual = HashSet::new();
                let mut has_self = false;

                for item in items {
                    if item.name == "self" {
                        has_self = true;
                    } else {
                        // Use alias if provided, otherwise original name
                        let exposed_name = item.alias.as_ref().unwrap_or(&item.name);
                        unqual.insert(exposed_name.clone());
                    }
                }

                (unqual, has_self)
            }
        };

        Ok(ResolvedImport {
            module_name,
            definitions: defs,
            unqualified,
            qualified,
        })
    }

    fn load_module(&mut self, imp: &ImportDef) -> Result<Vec<Definition>, ModuleError> {
        let rel_path = imp.path.join("/") + ".glass";

        let file_path = self
            .search_paths
            .iter()
            .map(|dir| dir.join(&rel_path))
            .find(|p| p.is_file())
            .ok_or_else(|| ModuleError {
                message: format!(
                    "cannot find module '{}' (searched: {})",
                    imp.path.join("/"),
                    self.search_paths
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                import: imp.clone(),
            })?;

        let canonical = file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.clone());

        // Cycle detection
        if self.resolving.contains(&canonical) {
            return Err(ModuleError {
                message: format!("circular import detected: '{}'", imp.path.join("/")),
                import: imp.clone(),
            });
        }

        // Cache hit
        if let Some(defs) = self.cache.get(&canonical) {
            return Ok(defs.clone());
        }

        // Parse
        self.resolving.insert(canonical.clone());
        let source = std::fs::read_to_string(&file_path).map_err(|e| ModuleError {
            message: format!("cannot read '{}': {}", file_path.display(), e),
            import: imp.clone(),
        })?;

        let tokens = Lexer::tokenize(&source).map_err(|e| ModuleError {
            message: format!("lex error in '{}': {}", file_path.display(), e),
            import: imp.clone(),
        })?;
        let mut parser = Parser::new(tokens);
        let output = parser.parse_module();
        if let Some(e) = output.errors.first() {
            return Err(ModuleError {
                message: format!("parse error in '{}': {}", file_path.display(), e.message),
                import: imp.clone(),
            });
        }
        let parsed = output.module;

        // Recursively resolve imports in the imported module
        let (resolved_module, _sub_imports, _, _) =
            self.resolve_module(&parsed).map_err(|errs| ModuleError {
                message: format!(
                    "errors in '{}': {}",
                    imp.path.join("/"),
                    errs.iter()
                        .map(|e| e.message.clone())
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
                import: imp.clone(),
            })?;

        self.resolving.remove(&canonical);

        // Filter out Import definitions, keep everything else
        let defs: Vec<Definition> = resolved_module
            .definitions
            .iter()
            .filter(|d| !matches!(d, Definition::Import(_)))
            .cloned()
            .collect();

        self.cache.insert(canonical, defs.clone());
        Ok(defs)
    }
}

/// Get the name of a definition (for deduplication).
pub fn def_name_pub(def: &Definition) -> Option<&str> {
    def_name(def)
}

pub fn def_name(def: &Definition) -> Option<&str> {
    match def {
        Definition::Function(f) => Some(&f.name),
        Definition::Type(t) => Some(&t.name),
        Definition::Const(c) => Some(&c.name),
        Definition::External(e) => Some(&e.fn_name),
        Definition::Import(_) | Definition::Extend(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_sdk_option() {
        let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("x.glass");
        let mut resolver = ModuleResolver::new(&input);

        let source = "import option";
        let tokens = Lexer::tokenize(source).expect("lex failed");
        let mut parser = Parser::new(tokens);
        let module = {
            let _o = parser.parse_module();
            assert!(_o.errors.is_empty(), "parse errors: {:?}", _o.errors);
            _o.module
        };

        let (resolved, imports, _, _) = resolver.resolve_module(&module).unwrap();
        // Should contain Option type from sdk/option.glass
        let type_names: Vec<&str> = resolved
            .definitions
            .iter()
            .filter_map(|d| {
                if let Definition::Type(t) = d {
                    Some(t.name.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(type_names.contains(&"Option"), "got: {:?}", type_names);

        // Should be qualified-only (no selective items)
        assert_eq!(imports.len(), 1);
        assert!(imports[0].qualified);
        assert!(imports[0].unqualified.is_empty());
    }

    #[test]
    fn resolve_selective_import() {
        let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("x.glass");
        let mut resolver = ModuleResolver::new(&input);

        let source = "import option { Option, option_map }";
        let tokens = Lexer::tokenize(source).expect("lex failed");
        let mut parser = Parser::new(tokens);
        let module = {
            let _o = parser.parse_module();
            assert!(_o.errors.is_empty(), "parse errors: {:?}", _o.errors);
            _o.module
        };

        let (_resolved, imports, _, _) = resolver.resolve_module(&module).unwrap();
        assert_eq!(imports.len(), 1);
        assert!(!imports[0].qualified); // no `self`
        assert!(imports[0].unqualified.contains("Option"));
        assert!(imports[0].unqualified.contains("option_map"));
    }

    #[test]
    fn resolve_self_import() {
        let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("x.glass");
        let mut resolver = ModuleResolver::new(&input);

        let source = "import option { Option, self }";
        let tokens = Lexer::tokenize(source).expect("lex failed");
        let mut parser = Parser::new(tokens);
        let module = {
            let _o = parser.parse_module();
            assert!(_o.errors.is_empty(), "parse errors: {:?}", _o.errors);
            _o.module
        };

        let (_resolved, imports, _, _) = resolver.resolve_module(&module).unwrap();
        assert_eq!(imports.len(), 1);
        assert!(imports[0].qualified); // self → qualified access
        assert!(imports[0].unqualified.contains("Option"));
    }

    #[test]
    fn circular_import_detected() {
        let dir = std::env::temp_dir().join("glass_test_circular");
        let _ = std::fs::create_dir_all(&dir);

        std::fs::write(dir.join("a.glass"), "import b\npub fn fa() -> Int { 1 }").unwrap();
        std::fs::write(dir.join("b.glass"), "import a\npub fn fb() -> Int { 2 }").unwrap();

        let input = dir.join("a.glass");
        let mut resolver = ModuleResolver::new(&input);

        let source = std::fs::read_to_string(&input).unwrap();
        let tokens = Lexer::tokenize(&source).expect("lex failed");
        let mut parser = Parser::new(tokens);
        let module = {
            let _o = parser.parse_module();
            assert!(_o.errors.is_empty(), "parse errors: {:?}", _o.errors);
            _o.module
        };

        let result = resolver.resolve_module(&module);
        assert!(result.is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_module_error() {
        let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("x.glass");
        let mut resolver = ModuleResolver::new(&input);

        let source = "import nonexistent_module";
        let tokens = Lexer::tokenize(source).expect("lex failed");
        let mut parser = Parser::new(tokens);
        let module = {
            let _o = parser.parse_module();
            assert!(_o.errors.is_empty(), "parse errors: {:?}", _o.errors);
            _o.module
        };

        let result = resolver.resolve_module(&module);
        assert!(result.is_err());
    }
}
