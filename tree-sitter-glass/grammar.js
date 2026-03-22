module.exports = grammar({
  name: "glass",

  extras: ($) => [/\s/, $.line_comment],

  word: ($) => $.lower_identifier,

  conflicts: ($) => [
    [$.pattern, $._expr],
    [$.record_pattern, $.record_expr],
    [$.record_pattern, $.record_update_brace_expr],
    [$.constructor_pattern, $.qualified_upper],
    [$.record_expr, $.qualified_upper],
    [$.tuple_pattern, $.tuple_expr],
    [$.list_pattern, $.list_expr],
    [$.or_pattern],
    [$._expr],
    [$.field_init],
    [$.named_field_pattern],
  ],

  rules: {
    source_file: ($) => repeat($._definition),

    _definition: ($) =>
      choice(
        $.import_definition,
        $.const_definition,
        $.struct_definition,
        $.enum_definition,
        $.function_definition,
        $.external_definition,
        $.extend_definition,
      ),

    // ── imports ──────────────────────────────────────────────

    import_definition: ($) =>
      seq(
        "import",
        $.import_path,
        optional($.import_exposing),
      ),

    import_path: ($) =>
      seq($.lower_identifier, repeat(seq("/", $.lower_identifier))),

    import_exposing: ($) =>
      seq("{", commaSep1($.import_item), "}"),

    import_item: ($) =>
      seq($.upper_identifier, optional(seq("as", $.upper_identifier))),

    // ── const ───────────────────────────────────────────────

    const_definition: ($) =>
      seq(
        optional($.visibility),
        "const",
        choice($.lower_identifier, $.upper_identifier),
        ":",
        $._type,
        "=",
        $._expr,
      ),

    // ── struct ──────────────────────────────────────────────

    struct_definition: ($) =>
      seq(
        optional($.visibility),
        "struct",
        $.upper_identifier,
        optional($.type_params),
        "{",
        commaSep($.field_declaration),
        optional(","),
        "}",
      ),

    field_declaration: ($) => seq($.lower_identifier, ":", $._type),

    // ── enum ────────────────────────────────────────────────

    enum_definition: ($) =>
      seq(
        optional($.visibility),
        "enum",
        $.upper_identifier,
        optional($.type_params),
        "{",
        repeat1($.variant),
        "}",
      ),

    variant: ($) =>
      seq(
        $.upper_identifier,
        optional(
          choice(
            seq("(", commaSep1($._type), ")"),
            seq("{", commaSep1($.field_declaration), optional(","), "}"),
          ),
        ),
      ),

    // ── function ────────────────────────────────────────────

    function_definition: ($) =>
      seq(
        optional(choice($.visibility, "local")),
        "fn",
        $.lower_identifier,
        $.parameter_list,
        optional(seq("->", $._type)),
        $.block,
      ),

    parameter_list: ($) => seq("(", commaSep($.parameter), optional(","), ")"),

    parameter: ($) => seq($.pattern, ":", $._type),

    // ── external ────────────────────────────────────────────

    external_definition: ($) =>
      seq(
        $.attribute,
        optional($.visibility),
        "fn",
        $.lower_identifier,
        $.parameter_list,
        optional(seq("->", $._type)),
      ),

    attribute: ($) =>
      seq("@", $.lower_identifier, "(", commaSep1($.string_literal), ")"),

    // ── extend ──────────────────────────────────────────────

    extend_definition: ($) =>
      seq(
        "extend",
        $._type,
        "{",
        repeat1($.function_definition),
        "}",
      ),

    // ── type expressions ────────────────────────────────────

    type_params: ($) => seq("(", commaSep1($.upper_identifier), ")"),

    _type: ($) =>
      choice(
        $.type_name,
        $.generic_type,
        $.tuple_type,
        $.function_type,
        $.qualified_type,
      ),

    type_name: ($) => choice($.upper_identifier, $.lower_identifier),

    generic_type: ($) =>
      prec(2, seq(
        choice($.upper_identifier, $.lower_identifier),
        "(",
        commaSep1($._type),
        ")",
      )),

    tuple_type: ($) =>
      seq("(", $._type, ",", commaSep1($._type), ")"),

    function_type: ($) =>
      prec.right(1, seq("fn", "(", commaSep($._type), ")", "->", $._type)),

    qualified_type: ($) =>
      prec(3, seq($.lower_identifier, ".", choice($.upper_identifier, $.generic_type))),

    // ── expressions ─────────────────────────────────────────

    _expr: ($) =>
      choice(
        $.int_literal,
        $.float_literal,
        $.string_literal,
        $.rawcode_literal,
        $.bool_literal,
        $.lower_identifier,
        $.upper_identifier,
        $.binary_expr,
        $.unary_expr,
        $.call_expr,
        $.field_access_expr,
        $.qualified_access_expr,
        $.qualified_upper,
        $.record_expr,
        $.record_update_expr,
        $.record_update_brace_expr,
        $.lambda_expr,
        $.list_expr,
        $.tuple_expr,
        $.paren_expr,
        $.block,
        $.case_expr,
        $.clone_expr,
        $.todo_expr,
      ),

    // ── literals ────────────────────────────────────────────

    int_literal: ($) => token(choice(/[0-9]+/, /0x[0-9a-fA-F]+/)),

    float_literal: ($) => token(/[0-9]+\.[0-9]+/),

    string_literal: ($) =>
      token(seq('"', repeat(choice(/[^"\\]/, /\\./)), '"')),

    rawcode_literal: ($) => token(seq("'", /[^']{4}/, "'")),

    bool_literal: ($) => choice("True", "False"),

    // ── identifiers ─────────────────────────────────────────

    visibility: ($) => "pub",

    lower_identifier: ($) => /[a-z_][a-zA-Z0-9_]*/,

    upper_identifier: ($) => /[A-Z][a-zA-Z0-9_]*/,

    // ── binary expressions ──────────────────────────────────

    binary_expr: ($) =>
      choice(
        prec.left(1, seq($._expr, "|>", $._expr)),
        prec.left(2, seq($._expr, "||", $._expr)),
        prec.left(3, seq($._expr, "&&", $._expr)),
        prec.left(4, seq($._expr, choice("==", "!="), $._expr)),
        prec.left(5, seq($._expr, choice("<", ">", "<=", ">="), $._expr)),
        prec.left(6, seq($._expr, choice("+", "-", "<>"), $._expr)),
        prec.left(7, seq($._expr, choice("*", "/", "%"), $._expr)),
      ),

    unary_expr: ($) =>
      prec(8, seq(choice("-", "!"), $._expr)),

    // ── call / access ───────────────────────────────────────

    call_expr: ($) =>
      prec(10, seq(
        choice($.lower_identifier, $.field_access_expr, $.qualified_access_expr, $.qualified_upper, $.upper_identifier),
        $.argument_list,
      )),

    argument_list: ($) => seq("(", commaSep($._expr), ")"),

    field_access_expr: ($) =>
      prec.left(11, seq($._expr, ".", $.lower_identifier)),

    qualified_access_expr: ($) =>
      prec(12, seq($.lower_identifier, ".", choice($.lower_identifier, $.upper_identifier))),

    qualified_upper: ($) =>
      prec(12, seq(
        choice($.upper_identifier, $.qualified_upper),
        "::",
        $.upper_identifier,
      )),

    // ── record / constructor ────────────────────────────────

    record_expr: ($) =>
      prec(9, seq(
        choice($.upper_identifier, $.qualified_upper),
        "{",
        commaSep($.field_init),
        optional(","),
        "}",
      )),

    field_init: ($) =>
      choice(
        seq($.lower_identifier, ":", $._expr),
        $.lower_identifier,
      ),

    record_update_expr: ($) =>
      prec(9, seq(
        $.upper_identifier,
        "(",
        "..",
        $._expr,
        ",",
        commaSep1($.field_init),
        optional(","),
        ")",
      )),

    record_update_brace_expr: ($) =>
      prec(9, seq(
        $.upper_identifier,
        "{",
        "..",
        $._expr,
        ",",
        commaSep1($.field_init),
        optional(","),
        "}",
      )),

    // ── lambda ──────────────────────────────────────────────

    lambda_expr: ($) =>
      prec.right(0, seq(
        "fn",
        "(",
        commaSep($.parameter),
        ")",
        optional(seq("->", $._type)),
        $.block,
      )),

    // ── list ────────────────────────────────────────────────

    list_expr: ($) =>
      seq(
        "[",
        choice(
          seq(commaSep($._expr), optional(",")),
          seq($._expr, "|", $._expr),
        ),
        "]",
      ),

    // ── tuple / paren ───────────────────────────────────────

    tuple_expr: ($) =>
      seq("(", $._expr, ",", commaSep1($._expr), optional(","), ")"),

    paren_expr: ($) => seq("(", $._expr, ")"),

    // ── block ───────────────────────────────────────────────

    block: ($) =>
      seq("{", repeat($.let_binding), $._expr, "}"),

    // ── let ─────────────────────────────────────────────────

    let_binding: ($) =>
      seq("let", $.pattern, optional(seq(":", $._type)), "=", $._expr),

    // ── case ────────────────────────────────────────────────

    case_expr: ($) =>
      seq("case", $._expr, "{", repeat1($.case_arm), "}"),

    case_arm: ($) =>
      seq(
        $.pattern,
        optional($.guard),
        "->",
        $._expr,
      ),

    guard: ($) => seq("if", $._expr),

    // ── clone / todo ────────────────────────────────────────

    clone_expr: ($) =>
      prec(10, seq("clone", "(", $._expr, ")")),

    todo_expr: ($) =>
      prec(10, seq("todo", "(", optional($.string_literal), ")")),

    // ── patterns ────────────────────────────────────────────

    pattern: ($) =>
      choice(
        $.wildcard_pattern,
        $.lower_identifier,
        $.int_literal,
        $.string_literal,
        $.bool_literal,
        $.constructor_pattern,
        $.record_pattern,
        $.tuple_pattern,
        $.list_pattern,
        $.or_pattern,
        $.as_pattern,
      ),

    wildcard_pattern: ($) => "_",

    constructor_pattern: ($) =>
      prec(2, seq(
        choice($.upper_identifier, $.qualified_upper),
        optional(seq("(", commaSep1($.pattern), ")")),
      )),

    record_pattern: ($) =>
      prec(1, seq(
        choice($.upper_identifier, $.qualified_upper),
        "{",
        choice(
          seq(commaSep1($.named_field_pattern), optional(seq(",", "..")), optional(",")),
          "..",
        ),
        "}",
      )),

    named_field_pattern: ($) =>
      choice(
        seq($.lower_identifier, ":", $.pattern),
        $.lower_identifier,
      ),

    tuple_pattern: ($) =>
      seq("(", $.pattern, ",", commaSep1($.pattern), ")"),

    list_pattern: ($) =>
      seq(
        "[",
        choice(
          seq($.pattern, "|", $.pattern),
          seq(),
        ),
        "]",
      ),

    or_pattern: ($) =>
      prec.left(seq($.pattern, "|", $.pattern)),

    as_pattern: ($) =>
      prec.right(seq($.pattern, "as", $.lower_identifier)),

    // ── comments ────────────────────────────────────────────

    line_comment: ($) => token(seq("//", /[^\n]*/)),
  },
});

function commaSep(rule) {
  return optional(commaSep1(rule));
}

function commaSep1(rule) {
  return seq(rule, repeat(seq(",", rule)));
}
