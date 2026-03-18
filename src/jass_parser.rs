// Parser for JASS common.j / blizzard.j files.
// Extracts type hierarchy and native function declarations.

#![allow(dead_code)]

use std::collections::HashMap;

/// A JASS type in the handle hierarchy.
#[derive(Debug, Clone)]
pub struct JassType {
    pub name: String,
    pub parent: Option<String>,
}

/// A JASS native function declaration.
#[derive(Debug, Clone)]
pub struct JassNative {
    pub name: String,
    pub params: Vec<JassParam>,
    pub return_type: Option<String>,
    pub is_constant: bool,
}

#[derive(Debug, Clone)]
pub struct JassParam {
    pub type_name: String,
    pub param_name: String,
}

/// Parsed JASS SDK.
#[derive(Debug)]
pub struct JassSdk {
    pub types: HashMap<String, JassType>,
    pub natives: Vec<JassNative>,
}

impl JassSdk {
    pub fn parse(source: &str) -> Self {
        let mut types = HashMap::new();
        let mut natives = Vec::new();

        for line in source.lines() {
            let trimmed = line.trim();

            if let Some(ty) = Self::parse_type_line(trimmed) {
                types.insert(ty.name.clone(), ty);
            } else if let Some(native) = Self::parse_native_line(trimmed) {
                natives.push(native);
            }
        }

        JassSdk { types, natives }
    }

    /// Parse `type NAME extends PARENT`
    fn parse_type_line(line: &str) -> Option<JassType> {
        let line = line.strip_prefix("type ")?;
        // Split on "extends"
        let parts: Vec<&str> = line.splitn(2, "extends").collect();
        if parts.len() != 2 {
            return None;
        }
        let name = parts.first()?.trim().to_string();
        let parent = parts
            .get(1)?
            .split_whitespace()
            .next()?
            .trim_end_matches("//")
            .to_string();

        Some(JassType {
            name,
            parent: Some(parent),
        })
    }

    /// Parse `[constant] native NAME takes PARAMS returns RETURN`
    fn parse_native_line(line: &str) -> Option<JassNative> {
        let is_constant = line.starts_with("constant ");
        let line = if is_constant {
            line.strip_prefix("constant ")?
        } else {
            line
        };

        let line = line.strip_prefix("native ")?;

        // Split: NAME takes PARAMS returns RETURN
        let takes_idx = line.find(" takes ")?;
        let name = line.get(..takes_idx)?.trim().to_string();
        let rest = line.get(takes_idx + 7..)?; // skip " takes "

        let returns_idx = rest.find(" returns ");
        let (params_str, return_type) = match returns_idx {
            Some(idx) => {
                let ret = rest.get(idx + 9..)?.trim().to_string();
                let ret = if ret == "nothing" { None } else { Some(ret) };
                (rest.get(..idx)?.trim(), ret)
            }
            None => (rest.trim(), None),
        };

        let params = if params_str == "nothing" {
            Vec::new()
        } else {
            params_str
                .split(',')
                .filter_map(|p| {
                    let p = p.trim();
                    let mut parts = p.split_whitespace();
                    let type_name = parts.next()?.to_string();
                    let param_name = parts.next()?.to_string();
                    Some(JassParam {
                        type_name,
                        param_name,
                    })
                })
                .collect()
        };

        Some(JassNative {
            name,
            params,
            return_type,
            is_constant,
        })
    }

    /// Map a JASS type name to a Glass type name.
    pub fn jass_to_glass_type(jass_type: &str) -> String {
        match jass_type {
            "integer" => "Int".into(),
            "real" => "Float".into(),
            "boolean" => "Bool".into(),
            "string" => "String".into(),
            "code" => "Code".into(),
            "nothing" => "Void".into(),
            // Handle types: capitalize first letter (most already are)
            other => {
                let mut chars = other.chars();
                match chars.next() {
                    Some(c) => {
                        let upper: String = c.to_uppercase().collect();
                        format!("{}{}", upper, chars.as_str())
                    }
                    None => other.into(),
                }
            }
        }
    }

    /// Check if a JASS type is a handle type (extends handle/agent/widget).
    pub fn is_handle_type(&self, type_name: &str) -> bool {
        if type_name == "handle" {
            return true;
        }
        if let Some(ty) = self.types.get(type_name)
            && let Some(parent) = &ty.parent
        {
            return parent == "handle" || self.is_handle_type(parent);
        }
        false
    }
}

// === Auto-binding generation ===

/// Classification of a native function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeKind {
    /// Pure query — safe to call directly (Get*, Is*, constant natives, math)
    Pure,
    /// Effectful — creates/destroys/modifies game state
    Effectful,
}

impl JassNative {
    pub fn classify(&self) -> NativeKind {
        if self.is_constant {
            return NativeKind::Pure;
        }
        let name = &self.name;
        // Pure: getters, queries, math, conversion
        if name.starts_with("Get")
            || name.starts_with("Is")
            || name.starts_with("I2")
            || name.starts_with("R2")
            || name.starts_with("S2")
            || name.starts_with("Deg2")
            || name.starts_with("Rad2")
            || matches!(
                name.as_str(),
                "Sin"
                    | "Cos"
                    | "Tan"
                    | "Asin"
                    | "Acos"
                    | "Atan"
                    | "Atan2"
                    | "SquareRoot"
                    | "Pow"
                    | "StringLength"
                    | "SubString"
                    | "StringCase"
                    | "StringHash"
                    | "Player"
                    | "OrderId"
                    | "OrderId2String"
                    | "AbilityId"
                    | "AbilityId2String"
            )
        {
            NativeKind::Pure
        } else {
            NativeKind::Effectful
        }
    }
}

impl JassSdk {
    /// Generate Glass source for all natives, grouped by domain.
    pub fn generate_glass_bindings(&self) -> String {
        let mut output = String::new();
        output.push_str("// Auto-generated Glass bindings for JASS SDK (common.j)\n");
        output.push_str("// Do not edit — regenerate with `glass --gen-bindings`\n\n");

        for native in &self.natives {
            // Skip natives with `code` params (can't represent in Glass yet)
            if native.params.iter().any(|p| p.type_name == "code") {
                output.push_str(&format!("// skipped: {} (takes code param)\n", native.name));
                continue;
            }

            let kind = native.classify();
            let kind_comment = match kind {
                NativeKind::Pure => "pure",
                NativeKind::Effectful => "effect",
            };

            let params: Vec<String> = native
                .params
                .iter()
                .map(|p| {
                    format!(
                        "{}: {}",
                        to_snake_case(&p.param_name),
                        Self::jass_to_glass_type(&p.type_name)
                    )
                })
                .collect();

            let ret = match &native.return_type {
                Some(t) => format!(" -> {}", Self::jass_to_glass_type(t)),
                None => String::new(),
            };

            let glass_name = to_snake_case(&native.name);

            output.push_str(&format!(
                "// {}\n@external(\"jass\", \"{}\") pub fn {}({}){}\n\n",
                kind_comment,
                native.name,
                glass_name,
                params.join(", "),
                ret
            ));
        }

        output
    }
}

/// Convert PascalCase/camelCase to snake_case.
fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            // Don't add underscore between consecutive uppercase (e.g. "GetUnitAI" → "get_unit_ai")
            let prev_upper = s
                .as_bytes()
                .get(i.wrapping_sub(1))
                .is_some_and(|b| b.is_ascii_uppercase());
            if !prev_upper {
                result.push('_');
            }
        }
        result.push(c.to_lowercase().next().unwrap_or(c));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_type_line() {
        let ty = JassSdk::parse_type_line(
            "type unit               extends     widget  // a single unit reference",
        );
        let ty = ty.expect("should parse");
        assert_eq!(ty.name, "unit");
        assert_eq!(ty.parent.as_deref(), Some("widget"));
    }

    #[test]
    fn parse_native_simple() {
        let n = JassSdk::parse_native_line("native Sin      takes real radians returns real");
        let n = n.expect("should parse");
        assert_eq!(n.name, "Sin");
        assert_eq!(n.params.len(), 1);
        assert_eq!(n.params[0].type_name, "real");
        assert_eq!(n.params[0].param_name, "radians");
        assert_eq!(n.return_type.as_deref(), Some("real"));
        assert!(!n.is_constant);
    }

    #[test]
    fn parse_native_nothing() {
        let n = JassSdk::parse_native_line(
            "native DestroyTimer takes timer whichTimer returns nothing",
        );
        let n = n.expect("should parse");
        assert_eq!(n.name, "DestroyTimer");
        assert_eq!(n.params.len(), 1);
        assert!(n.return_type.is_none());
    }

    #[test]
    fn parse_native_multi_params() {
        let n = JassSdk::parse_native_line(
            "native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit",
        );
        let n = n.expect("should parse");
        assert_eq!(n.name, "CreateUnit");
        assert_eq!(n.params.len(), 5);
        assert_eq!(n.return_type.as_deref(), Some("unit"));
    }

    #[test]
    fn parse_constant_native() {
        let n = JassSdk::parse_native_line(
            "constant native GetPlayerId takes player whichPlayer returns integer",
        );
        let n = n.expect("should parse");
        assert_eq!(n.name, "GetPlayerId");
        assert!(n.is_constant);
    }

    #[test]
    fn parse_native_takes_nothing() {
        let n = JassSdk::parse_native_line("native GetLocalPlayer takes nothing returns player");
        let n = n.expect("should parse");
        assert_eq!(n.name, "GetLocalPlayer");
        assert!(n.params.is_empty());
        assert_eq!(n.return_type.as_deref(), Some("player"));
    }

    #[test]
    fn parse_full_common_j() {
        let source = std::fs::read_to_string("sdk/common.j").expect("need sdk/common.j");
        let sdk = JassSdk::parse(&source);

        // Should have many types and natives
        assert!(
            sdk.types.len() > 30,
            "expected >30 types, got {}",
            sdk.types.len()
        );
        assert!(
            sdk.natives.len() > 1000,
            "expected >1000 natives, got {}",
            sdk.natives.len()
        );

        // Check specific types
        assert!(sdk.types.contains_key("unit"));
        assert!(sdk.types.contains_key("timer"));
        assert!(sdk.types.contains_key("player"));

        // Check type hierarchy
        assert!(sdk.is_handle_type("unit"));
        assert!(sdk.is_handle_type("timer"));
        assert!(sdk.is_handle_type("player"));
        assert!(!sdk.is_handle_type("integer"));

        // Check specific natives exist
        let create_unit = sdk.natives.iter().find(|n| n.name == "CreateUnit");
        assert!(create_unit.is_some());

        let get_unit_x = sdk.natives.iter().find(|n| n.name == "GetUnitX");
        assert!(get_unit_x.is_some());

        // Snapshot: type count and native count
        insta::assert_snapshot!(
            "sdk_stats",
            format!("types: {}\nnatives: {}", sdk.types.len(), sdk.natives.len())
        );
    }

    #[test]
    fn classify_natives() {
        let source = std::fs::read_to_string("sdk/common.j").expect("need sdk/common.j");
        let sdk = JassSdk::parse(&source);

        let pure_count = sdk
            .natives
            .iter()
            .filter(|n| n.classify() == NativeKind::Pure)
            .count();
        let effect_count = sdk
            .natives
            .iter()
            .filter(|n| n.classify() == NativeKind::Effectful)
            .count();

        insta::assert_snapshot!(
            "native_classification",
            format!(
                "pure: {}\neffectful: {}\ntotal: {}",
                pure_count,
                effect_count,
                sdk.natives.len()
            )
        );
    }

    #[test]
    fn generate_bindings_sample() {
        let sdk = JassSdk::parse(
            "native GetUnitX takes unit whichUnit returns real\n\
             native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit\n\
             native DestroyTimer takes timer whichTimer returns nothing\n\
             constant native GetPlayerId takes player whichPlayer returns integer\n\
             native TimerStart takes timer whichTimer, real timeout, boolean periodic, code handlerFunc returns nothing\n",
        );
        insta::assert_snapshot!(sdk.generate_glass_bindings());
    }

    #[test]
    fn snake_case_conversion() {
        assert_eq!(to_snake_case("GetUnitX"), "get_unit_x");
        assert_eq!(to_snake_case("CreateUnit"), "create_unit");
        assert_eq!(to_snake_case("DestroyTimer"), "destroy_timer");
        assert_eq!(to_snake_case("Sin"), "sin");
        assert_eq!(to_snake_case("Atan2"), "atan2");
    }

    #[test]
    fn jass_to_glass_type_mapping() {
        assert_eq!(JassSdk::jass_to_glass_type("integer"), "Int");
        assert_eq!(JassSdk::jass_to_glass_type("real"), "Float");
        assert_eq!(JassSdk::jass_to_glass_type("boolean"), "Bool");
        assert_eq!(JassSdk::jass_to_glass_type("string"), "String");
        assert_eq!(JassSdk::jass_to_glass_type("unit"), "Unit");
        assert_eq!(JassSdk::jass_to_glass_type("timer"), "Timer");
        assert_eq!(JassSdk::jass_to_glass_type("player"), "Player");
    }
}
