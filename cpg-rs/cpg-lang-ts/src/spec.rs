//! Declarative per-language specifications.
//!
//! Everything that differs between languages is data here: the node-kind names
//! for functions/calls/literals, the field names that hold a callee or a
//! parameter list, and the shapes of assignment/declaration. The mapping engine
//! (`engine.rs`) is identical for all of them. If six grammars as different as
//! Java, Go, JavaScript, Ruby, Rust and Python reduce to six of these structs,
//! the language contract holds — that is the whole point of this crate.

use cpg_frontend::LanguageTraits;
use tree_sitter::Language;

/// One assignment/declaration form: a node kind plus the fields holding the
/// left-hand target and the right-hand value.
#[derive(Clone, Copy)]
pub struct AssignForm {
    pub kind: &'static str,
    pub lhs_field: &'static str,
    pub rhs_field: &'static str,
}

pub struct TsLangSpec {
    pub name: &'static str,
    pub namespace_delim: &'static str,
    pub traits: LanguageTraits,
    pub extensions: &'static [&'static str],
    /// Built tree-sitter grammar.
    pub language: Language,

    /// Node kinds that declare a function/method.
    pub function_kinds: &'static [&'static str],
    /// Node kinds that hold a function's parameters (searched if no `parameters`
    /// field is present, e.g. Ruby's `method_parameters`).
    pub param_container_kinds: &'static [&'static str],
    /// Node kinds for an individual call expression.
    pub call_kinds: &'static [&'static str],
    /// Field on a call node holding the callee (name or member expression).
    pub callee_field: &'static str,
    /// Assignment / declaration forms.
    pub assign_forms: &'static [AssignForm],
    /// Node kinds treated as control structures (branch/loop).
    pub control_kinds: &'static [&'static str],
    /// Node kinds for return statements/expressions.
    pub return_kinds: &'static [&'static str],
    /// True for languages where a function's final expression is its return
    /// value (Rust, Ruby). The engine then wraps that tail expression in a
    /// Return node so the shared dataflow engine sees the param→return flow.
    pub implicit_return: bool,
}

impl TsLangSpec {
    pub fn is_function(&self, k: &str) -> bool {
        self.function_kinds.contains(&k)
    }
    pub fn is_call(&self, k: &str) -> bool {
        self.call_kinds.contains(&k)
    }
    pub fn is_control(&self, k: &str) -> bool {
        self.control_kinds.contains(&k)
    }
    pub fn is_return(&self, k: &str) -> bool {
        self.return_kinds.contains(&k)
    }
    pub fn assign_form(&self, k: &str) -> Option<&AssignForm> {
        self.assign_forms.iter().find(|f| f.kind == k)
    }
}

const CONTROL: &[&str] = &[
    "if_statement", "if_expression", "for_statement", "for_expression",
    "while_statement", "while_expression", "loop_expression", "enhanced_for_statement",
    "switch_statement", "switch_expression", "match_expression", "expression_switch_statement",
    "try_statement", "with_statement", "do_statement", "unless", "until", "case", "when",
    "for_in_statement", "for_of_statement", "labeled_statement",
];

const RETURNS: &[&str] = &["return_statement", "return_expression"];

pub fn java() -> TsLangSpec {
    TsLangSpec {
        name: "Java",
        namespace_delim: ".",
        traits: LanguageTraits::HAS_CLASSES | LanguageTraits::HAS_GENERICS
            | LanguageTraits::HAS_OVERLOADING,
        extensions: &["java"],
        language: tree_sitter_java::LANGUAGE.into(),
        function_kinds: &["method_declaration", "constructor_declaration"],
        param_container_kinds: &["formal_parameters"],
        call_kinds: &["method_invocation"],
        callee_field: "name",
        assign_forms: &[
            AssignForm { kind: "variable_declarator", lhs_field: "name", rhs_field: "value" },
            AssignForm { kind: "assignment_expression", lhs_field: "left", rhs_field: "right" },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: false,
    }
}

pub fn go() -> TsLangSpec {
    TsLangSpec {
        name: "Go",
        namespace_delim: ".",
        traits: LanguageTraits::HAS_FUNCTION_POINTERS | LanguageTraits::STRUCTURAL_TYPING,
        extensions: &["go"],
        language: tree_sitter_go::LANGUAGE.into(),
        function_kinds: &["function_declaration", "method_declaration"],
        param_container_kinds: &["parameter_list"],
        call_kinds: &["call_expression"],
        callee_field: "function",
        assign_forms: &[
            AssignForm { kind: "short_var_declaration", lhs_field: "left", rhs_field: "right" },
            AssignForm { kind: "assignment_statement", lhs_field: "left", rhs_field: "right" },
            AssignForm { kind: "var_spec", lhs_field: "name", rhs_field: "value" },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: false,
    }
}

pub fn javascript() -> TsLangSpec {
    TsLangSpec {
        name: "JavaScript",
        namespace_delim: ".",
        traits: LanguageTraits::HAS_CLASSES | LanguageTraits::HAS_FUNCTION_POINTERS
            | LanguageTraits::ALLOWS_FORWARD_REFS | LanguageTraits::STRUCTURAL_TYPING
            | LanguageTraits::HAS_DEFAULT_ARGS,
        extensions: &["js", "mjs", "cjs"],
        language: tree_sitter_javascript::LANGUAGE.into(),
        function_kinds: &[
            "function_declaration", "function_expression", "method_definition",
            "arrow_function", "generator_function_declaration",
        ],
        param_container_kinds: &["formal_parameters"],
        call_kinds: &["call_expression"],
        callee_field: "function",
        assign_forms: &[
            AssignForm { kind: "variable_declarator", lhs_field: "name", rhs_field: "value" },
            AssignForm { kind: "assignment_expression", lhs_field: "left", rhs_field: "right" },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: false,
    }
}

pub fn ruby() -> TsLangSpec {
    TsLangSpec {
        name: "Ruby",
        namespace_delim: "::",
        traits: LanguageTraits::HAS_CLASSES | LanguageTraits::ALLOWS_FORWARD_REFS
            | LanguageTraits::STRUCTURAL_TYPING | LanguageTraits::HAS_DEFAULT_ARGS,
        extensions: &["rb"],
        language: tree_sitter_ruby::LANGUAGE.into(),
        function_kinds: &["method", "singleton_method"],
        param_container_kinds: &["method_parameters", "parameters", "bare_parameters"],
        call_kinds: &["call", "command", "command_call"],
        callee_field: "method",
        assign_forms: &[
            AssignForm { kind: "assignment", lhs_field: "left", rhs_field: "right" },
            AssignForm { kind: "operator_assignment", lhs_field: "left", rhs_field: "right" },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: true,
    }
}

pub fn rust() -> TsLangSpec {
    TsLangSpec {
        name: "Rust",
        namespace_delim: "::",
        traits: LanguageTraits::HAS_GENERICS | LanguageTraits::HAS_FUNCTION_POINTERS
            | LanguageTraits::STRUCTURAL_TYPING,
        extensions: &["rs"],
        language: tree_sitter_rust::LANGUAGE.into(),
        function_kinds: &["function_item"],
        param_container_kinds: &["parameters"],
        call_kinds: &["call_expression", "macro_invocation"],
        callee_field: "function",
        assign_forms: &[
            AssignForm { kind: "let_declaration", lhs_field: "pattern", rhs_field: "value" },
            AssignForm { kind: "assignment_expression", lhs_field: "left", rhs_field: "right" },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: true,
    }
}

pub fn python() -> TsLangSpec {
    TsLangSpec {
        name: "Python",
        namespace_delim: ".",
        traits: LanguageTraits::HAS_CLASSES | LanguageTraits::ALLOWS_FORWARD_REFS
            | LanguageTraits::STRUCTURAL_TYPING | LanguageTraits::HAS_DEFAULT_ARGS,
        extensions: &["py"],
        language: tree_sitter_python::LANGUAGE.into(),
        function_kinds: &["function_definition"],
        param_container_kinds: &["parameters"],
        call_kinds: &["call"],
        callee_field: "function",
        assign_forms: &[
            AssignForm { kind: "assignment", lhs_field: "left", rhs_field: "right" },
            AssignForm { kind: "augmented_assignment", lhs_field: "left", rhs_field: "right" },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: false,
    }
}
