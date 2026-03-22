; Keywords
["fn" "let" "case" "import" "const" "struct" "enum" "extend" "local" "clone" "todo" "as"] @keyword

; Guard keyword
(guard "if" @keyword)

; Visibility keyword
(visibility) @keyword

; Boolean literals
(bool_literal) @constant.builtin

; Operators
["+" "-" "*" "/" "%" "==" "!=" "<" ">" "<=" ">=" "&&" "||" "<>" "|>" "!" "->"] @operator

; Punctuation
["(" ")" "{" "}" "[" "]"] @punctuation.bracket
["," ":" "::" ".." "." "|" "=" "@"] @punctuation.delimiter

; Literals
(int_literal) @number
(float_literal) @number.float
(string_literal) @string
(rawcode_literal) @string.special

; Comments
(line_comment) @comment

; Function definition name: first lower_identifier child after optional visibility
(function_definition (visibility) . (lower_identifier) @function)
(function_definition . (lower_identifier) @function)

; External definition name
(external_definition (attribute) (visibility) . (lower_identifier) @function)
(external_definition (attribute) . (lower_identifier) @function)

; Function calls: first child of call_expr that is lower_identifier
(call_expr . (lower_identifier) @function.call)

; Field access: last child of field_access_expr
(field_access_expr _ . (lower_identifier) @property)

; Field declarations: first child
(field_declaration . (lower_identifier) @property)

; Field init: first child
(field_init . (lower_identifier) @property)

; Named field pattern: first child
(named_field_pattern . (lower_identifier) @property)

; Type definitions: upper_identifier after optional visibility
(struct_definition (visibility) . (upper_identifier) @type)
(struct_definition . (upper_identifier) @type)
(enum_definition (visibility) . (upper_identifier) @type)
(enum_definition . (upper_identifier) @type)

; Type references
(type_name (upper_identifier) @type)
(generic_type . (upper_identifier) @type)
(qualified_type _ . (upper_identifier) @type)

; Constructors
(variant . (upper_identifier) @constructor)
(constructor_pattern . (upper_identifier) @constructor)
(record_pattern . (upper_identifier) @constructor)
(record_expr . (upper_identifier) @constructor)
(record_update_expr . (upper_identifier) @constructor)
(qualified_upper (upper_identifier) @constructor)

; Module paths
(import_path (lower_identifier) @module)
(qualified_access_expr . (lower_identifier) @module)
(qualified_type . (lower_identifier) @module)

; Parameters
(parameter (pattern (lower_identifier) @variable.parameter))

; Wildcard
(wildcard_pattern) @variable.builtin

; Attribute
(attribute) @attribute
