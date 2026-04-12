use crate::types::TypeRegistry;

impl super::JassCodegen {
    // === SoA type compilation ===

    pub(super) fn gen_soa_preamble(&mut self) {
        if self.types.types.is_empty() {
            return;
        }

        // Collect all info upfront to avoid borrow conflicts with self.emit
        struct TypeEmitInfo {
            name: String,
            is_enum: bool,
            variants: Vec<VariantEmitInfo>,
        }
        struct VariantEmitInfo {
            name: String,
            tag: i64,
            fields: Vec<(String, String)>, // (field_name, jass_type)
        }

        let type_infos: Vec<TypeEmitInfo> = self
            .types
            .types
            .values()
            .map(|info| TypeEmitInfo {
                name: info.name.clone(),
                is_enum: info.is_enum,
                variants: info
                    .variants
                    .iter()
                    .map(|v| VariantEmitInfo {
                        name: v.name.clone(),
                        tag: v.tag,
                        fields: v
                            .fields
                            .iter()
                            .map(|f| (f.name.clone(), f.jass_type.clone()))
                            .collect(),
                    })
                    .collect(),
            })
            .collect();

        // Collect SoA globals
        for info in &type_infos {
            self.add_global(&format!("// SoA arrays for type {}", info.name));

            if info.is_enum {
                self.add_global(&format!("integer array glass_{}_tag", info.name));
            }

            for variant in &info.variants {
                for (fname, ftype) in &variant.fields {
                    self.add_global(&format!(
                        "{} array glass_{}_{}_{}",
                        ftype, info.name, variant.name, fname
                    ));
                }
            }

            self.add_global(&format!("integer array glass_{}_free", info.name));
            self.add_global(&format!("integer glass_{}_free_top = 0", info.name));
            self.add_global(&format!("integer glass_{}_count = 0", info.name));
        }

        for info in &type_infos {
            if info.is_enum {
                for variant in &info.variants {
                    self.add_global(&format!(
                        "constant integer glass_TAG_{}_{} = {}",
                        info.name, variant.name, variant.tag
                    ));
                }
            }
        }

        // Alloc/dealloc
        for info in &type_infos {
            self.gen_alloc_fn(&info.name);
            self.output.push('\n');
            self.gen_dealloc_fn(&info.name);
            self.output.push('\n');
        }

        // Constructors
        for info in &type_infos {
            for variant in &info.variants {
                self.gen_constructor_fn_from(
                    &info.name,
                    info.is_enum,
                    &variant.name,
                    variant.tag,
                    &variant.fields,
                );
                self.output.push('\n');
            }
        }

        // Field getters — inlined as direct array access at call sites
        for _info in &type_infos {}
    }

    pub(super) fn gen_list_preamble(&mut self) {
        let list_types: Vec<String> = self.types.list_types.iter().cloned().collect();
        if list_types.is_empty() {
            return;
        }

        // Collect list globals
        for elem_type in &list_types {
            let lt = TypeRegistry::list_type_name(elem_type);
            self.add_global(&format!("// Linked list: {}", lt));
            self.add_global(&format!("{} array glass_{}_head", elem_type, lt));
            self.add_global(&format!("integer array glass_{}_tail", lt));
            self.add_global(&format!("integer array glass_{}_free", lt));
            self.add_global(&format!("integer glass_{}_free_top = 0", lt));
            self.add_global(&format!("integer glass_{}_count = 0", lt));
        }

        // Alloc/dealloc + cons for each list type
        for elem_type in &list_types {
            let lt = TypeRegistry::list_type_name(elem_type);
            self.gen_alloc_fn(&lt);
            self.output.push('\n');
            self.gen_dealloc_fn(&lt);
            self.output.push('\n');

            // cons: prepend element to list, return new node ID
            self.emit(&format!(
                "function glass_{}_cons takes {} head, integer tail returns integer",
                lt, elem_type
            ));
            self.indent += 1;
            self.emit(&format!("local integer id = glass_{}_alloc()", lt));
            self.emit(&format!("set glass_{}_head[id] = head", lt));
            self.emit(&format!("set glass_{}_tail[id] = tail", lt));
            self.emit("return id");
            self.indent -= 1;
            self.emit("endfunction");
            self.output.push('\n');
        }
    }

    pub(super) fn gen_alloc_fn(&mut self, type_name: &str) {
        self.emit(&format!(
            "function glass_{}_alloc takes nothing returns integer",
            type_name
        ));
        self.indent += 1;
        self.emit("local integer id");
        self.emit(&format!("if glass_{}_free_top > 0 then", type_name));
        self.indent += 1;
        self.emit(&format!(
            "set glass_{}_free_top = glass_{}_free_top - 1",
            type_name, type_name
        ));
        self.emit(&format!(
            "set id = glass_{}_free[glass_{}_free_top]",
            type_name, type_name
        ));
        self.indent -= 1;
        self.emit("else");
        self.indent += 1;
        self.emit(&format!(
            "set glass_{}_count = glass_{}_count + 1",
            type_name, type_name
        ));
        self.emit(&format!("set id = glass_{}_count", type_name));
        self.indent -= 1;
        self.emit("endif");
        self.emit("return id");
        self.indent -= 1;
        self.emit("endfunction");
    }

    pub(super) fn gen_dealloc_fn(&mut self, type_name: &str) {
        self.emit(&format!(
            "function glass_{}_dealloc takes integer id returns nothing",
            type_name
        ));
        self.indent += 1;
        if let Some(info) = self.types.types.get(type_name) {
            let nulls: Vec<String> = info
                .variants
                .iter()
                .flat_map(|variant| {
                    variant
                        .fields
                        .iter()
                        .filter_map(|field| match field.jass_type.as_str() {
                            "unit" | "player" | "timer" | "group" | "trigger" | "effect"
                            | "force" | "sound" | "location" | "rect" | "region" | "dialog"
                            | "quest" | "multiboard" | "leaderboard" | "texttag" | "lightning"
                            | "image" | "ubersplat" | "trackable" | "timerdialog"
                            | "fogmodifier" | "hashtable" => Some(format!(
                                "set glass_{}_{}_{} [id] = null",
                                type_name, variant.name, field.name
                            )),
                            _ => None,
                        })
                })
                .collect();
            for stmt in nulls {
                self.emit(&stmt);
            }
        }
        self.emit(&format!(
            "set glass_{}_free[glass_{}_free_top] = id",
            type_name, type_name
        ));
        self.emit(&format!(
            "set glass_{}_free_top = glass_{}_free_top + 1",
            type_name, type_name
        ));
        self.indent -= 1;
        self.emit("endfunction");
    }

    pub(super) fn gen_constructor_fn_from(
        &mut self,
        type_name: &str,
        is_enum: bool,
        variant_name: &str,
        variant_tag: i64,
        fields: &[(String, String)],
    ) {
        let params: Vec<String> = fields
            .iter()
            .map(|(fname, ftype)| format!("{} p_{}", ftype, fname))
            .collect();

        let takes = if params.is_empty() {
            "nothing".to_string()
        } else {
            params.join(", ")
        };

        self.emit(&format!(
            "function glass_new_{}_{} takes {} returns integer",
            type_name, variant_name, takes
        ));
        self.indent += 1;
        self.emit(&format!("local integer id = glass_{}_alloc()", type_name));

        if is_enum {
            self.emit(&format!(
                "set glass_{}_tag[id] = {}",
                type_name, variant_tag
            ));
        }

        for (fname, _ftype) in fields {
            self.emit(&format!(
                "set glass_{}_{}_{} [id] = p_{}",
                type_name, variant_name, fname, fname
            ));
        }

        self.emit("return id");
        self.indent -= 1;
        self.emit("endfunction");
    }
}
