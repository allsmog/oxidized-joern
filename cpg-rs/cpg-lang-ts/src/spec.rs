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

/// Per-extension grammar overrides: file extension -> grammar constructor.
/// The override's node kinds must be handled by the same spec fields.
type DialectTable = &'static [(&'static str, fn() -> Language)];

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
    /// Per-extension grammar overrides for languages whose dialects need a
    /// different grammar over the same node-kind vocabulary (TypeScript's
    /// `tsx`: JSX syntax is not parseable by the plain grammar, and the TSX
    /// grammar mis-parses `<T>x` casts — so the extension picks the grammar).
    /// The override's node kinds must be handled by the same spec fields.
    pub dialects: DialectTable,

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
    /// Node kinds that define a named type/namespace container (class, object,
    /// trait). The engine tracks the enclosing container name while scanning so
    /// constructor-sugar methods can be qualified. Empty = no tracking.
    pub type_container_kinds: &'static [&'static str],
    /// Field on a function-definition node holding its receiver (Go's
    /// `receiver`). The receiver's type qualifies the method for
    /// type-aware call resolution. None = no explicit receivers.
    pub receiver_field: Option<&'static str>,
    /// Joern models some language-level implicit receivers as a parameter at
    /// index zero (JavaScript/TypeScript `this`). Explicit source parameters
    /// continue at index one.
    pub implicit_receiver: Option<(&'static str, &'static str)>,
    /// A method name the language sugars onto its enclosing container's name at
    /// call sites (Scala: `object Foo { def apply(..) }` is called as
    /// `Foo(..)`). Such a method is registered under the container's name so
    /// the sugar call resolves; `fullName` keeps the `Container.method` form.
    pub ctor_sugar_method: Option<&'static str>,
    /// Field on a function-definition node holding a C-style declarator chain
    /// (C++: `declarator`). The method's name, qualifying scope
    /// (`Foo::bar`), and parameter list all live inside that subtree rather
    /// than in `name`/`parameters` fields. None = names are direct fields.
    pub declarator_field: Option<&'static str>,
    /// Type-container kinds that also emit a TypeDecl node (with base classes
    /// and members). Subset of `type_container_kinds`: namespaces qualify
    /// names but are not type declarations. Empty = no TypeDecl emission.
    pub type_decl_kinds: &'static [&'static str],
    /// Node kinds holding a type declaration's base-class list
    /// (C++: `base_class_clause`). Only read for `type_decl_kinds` nodes.
    pub base_clause_kinds: &'static [&'static str],
    /// Node kinds declaring a data member inside a type body
    /// (C++: `field_declaration`). Members carry declared types for
    /// receiver-hint resolution of member calls.
    pub member_kinds: &'static [&'static str],
    /// Wrapper type names whose single template argument is the type that
    /// matters for call resolution (`shared_ptr<FileServiceIf>` →
    /// `FileServiceIf`). Applied when extracting declared types of
    /// members/params/locals; empty = no unwrapping.
    pub smart_ptr_names: &'static [&'static str],
    /// Constructor-factory helper names (C++ `make_shared`/`make_unique`):
    /// standard-library functions whose constructed type lives in their
    /// template argument, not their own name. Calls to these are lowered
    /// under the constructed type's name so type-named sink specs match
    /// (`make_shared<SimpleRandomAccessFile>(path)` must be visible as a
    /// `SimpleRandomAccessFile` construction); empty = no rewriting.
    pub ctor_factories: &'static [&'static str],
    /// Optional source-level shim applied before parsing. Returns None when
    /// the file needs no transformation. MUST be line/column-preserving so
    /// every location in the CPG still points at the real source.
    pub preprocess: Option<fn(&str) -> Option<String>>,
}

/// C++/CLI (managed C++) uses syntax the standard grammar cannot parse —
/// `String^ s`, `gcnew`, `ref class` — which silently drops whole method
/// bodies from the graph (the Windows agent's PowerShell/AD code is written
/// in it). Rewrite the managed tokens to same-LENGTH standard spellings so
/// the grammar parses and every byte offset is preserved. `^` → `*` is safe
/// even for the XOR operator: both parse as binary expressions.
fn cpp_cli_shim(src: &str) -> Option<String> {
    let managed = src.contains("gcnew")
        || src.contains("ref class")
        || src.contains("ref struct")
        || src.contains("#using")
        || src.contains("msclr");
    if !managed {
        return None;
    }
    let s = src
        .replace('^', "*")
        .replace("gcnew", "new  ")
        .replace("ref class", "    class")
        .replace("ref struct", "    struct");
    Some(s)
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
    "if_statement",
    "if_expression",
    "for_statement",
    "for_expression",
    "while_statement",
    "while_expression",
    "loop_expression",
    "enhanced_for_statement",
    "switch_statement",
    "switch_expression",
    "match_expression",
    "expression_switch_statement",
    "try_statement",
    "with_statement",
    "do_statement",
    "unless",
    "until",
    "case",
    "when",
    "for_in_statement",
    "for_of_statement",
    "labeled_statement",
    "for_range_loop",
];

const RETURNS: &[&str] = &["return_statement", "return_expression"];

pub fn java() -> TsLangSpec {
    TsLangSpec {
        name: "Java",
        namespace_delim: ".",
        traits: LanguageTraits::HAS_CLASSES
            | LanguageTraits::HAS_GENERICS
            | LanguageTraits::HAS_OVERLOADING,
        extensions: &["java"],
        language: tree_sitter_java::LANGUAGE.into(),
        function_kinds: &["method_declaration", "constructor_declaration"],
        param_container_kinds: &["formal_parameters"],
        call_kinds: &["method_invocation"],
        callee_field: "name",
        assign_forms: &[
            AssignForm {
                kind: "variable_declarator",
                lhs_field: "name",
                rhs_field: "value",
            },
            AssignForm {
                kind: "assignment_expression",
                lhs_field: "left",
                rhs_field: "right",
            },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: false,
        type_container_kinds: &[],
        receiver_field: None,
        implicit_receiver: None,
        ctor_sugar_method: None,
        declarator_field: None,
        type_decl_kinds: &[],
        base_clause_kinds: &[],
        member_kinds: &[],
        smart_ptr_names: &[],
        ctor_factories: &[],
        preprocess: None,
        dialects: &[],
    }
}

pub fn go() -> TsLangSpec {
    TsLangSpec {
        name: "Go",
        namespace_delim: ".",
        traits: LanguageTraits::HAS_FUNCTION_POINTERS | LanguageTraits::STRUCTURAL_TYPING,
        extensions: &["go"],
        language: tree_sitter_go::LANGUAGE.into(),
        function_kinds: &["function_declaration", "method_declaration", "func_literal"],
        param_container_kinds: &["parameter_list"],
        call_kinds: &["call_expression"],
        callee_field: "function",
        assign_forms: &[
            AssignForm {
                kind: "short_var_declaration",
                lhs_field: "left",
                rhs_field: "right",
            },
            AssignForm {
                kind: "assignment_statement",
                lhs_field: "left",
                rhs_field: "right",
            },
            AssignForm {
                kind: "var_spec",
                lhs_field: "name",
                rhs_field: "value",
            },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: false,
        type_container_kinds: &[],
        receiver_field: Some("receiver"),
        implicit_receiver: None,
        ctor_sugar_method: None,
        declarator_field: None,
        type_decl_kinds: &[],
        base_clause_kinds: &[],
        member_kinds: &[],
        smart_ptr_names: &[],
        ctor_factories: &[],
        preprocess: None,
        dialects: &[],
    }
}

pub fn javascript() -> TsLangSpec {
    TsLangSpec {
        name: "JavaScript",
        namespace_delim: ".",
        traits: LanguageTraits::HAS_CLASSES
            | LanguageTraits::HAS_FUNCTION_POINTERS
            | LanguageTraits::ALLOWS_FORWARD_REFS
            | LanguageTraits::STRUCTURAL_TYPING
            | LanguageTraits::HAS_DEFAULT_ARGS,
        extensions: &["js", "mjs", "cjs"],
        language: tree_sitter_javascript::LANGUAGE.into(),
        function_kinds: &[
            "function_declaration",
            "function_expression",
            "method_definition",
            "arrow_function",
            "generator_function_declaration",
        ],
        param_container_kinds: &["formal_parameters"],
        call_kinds: &["call_expression"],
        callee_field: "function",
        assign_forms: &[
            AssignForm {
                kind: "variable_declarator",
                lhs_field: "name",
                rhs_field: "value",
            },
            AssignForm {
                kind: "assignment_expression",
                lhs_field: "left",
                rhs_field: "right",
            },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: false,
        type_container_kinds: &[],
        receiver_field: None,
        implicit_receiver: Some(("this", "program")),
        ctor_sugar_method: None,
        declarator_field: None,
        type_decl_kinds: &[],
        base_clause_kinds: &[],
        member_kinds: &[],
        smart_ptr_names: &[],
        ctor_factories: &[],
        preprocess: None,
        dialects: &[],
    }
}

pub fn typescript() -> TsLangSpec {
    TsLangSpec {
        name: "TypeScript",
        namespace_delim: ".",
        traits: LanguageTraits::HAS_CLASSES
            | LanguageTraits::HAS_GENERICS
            | LanguageTraits::HAS_FUNCTION_POINTERS
            | LanguageTraits::ALLOWS_FORWARD_REFS
            | LanguageTraits::HAS_DEFAULT_ARGS,
        extensions: &["ts", "tsx", "mts", "cts"],
        language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        dialects: TS_DIALECTS,
        function_kinds: &[
            "function_declaration",
            "function_expression",
            "method_definition",
            "arrow_function",
            "generator_function_declaration",
        ],
        param_container_kinds: &["formal_parameters"],
        call_kinds: &["call_expression"],
        callee_field: "function",
        assign_forms: &[
            AssignForm {
                kind: "variable_declarator",
                lhs_field: "name",
                rhs_field: "value",
            },
            AssignForm {
                kind: "assignment_expression",
                lhs_field: "left",
                rhs_field: "right",
            },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: false,
        type_container_kinds: &["class_declaration"],
        receiver_field: None,
        implicit_receiver: Some(("this", "program")),
        ctor_sugar_method: None,
        declarator_field: None,
        type_decl_kinds: &["class_declaration"],
        base_clause_kinds: &["class_heritage"],
        member_kinds: &["public_field_definition", "property_signature"],
        smart_ptr_names: &[],
        ctor_factories: &[],
        preprocess: None,
    }
}

/// `.tsx` files carry JSX, which the plain TypeScript grammar cannot parse
/// (whole component bodies would silently drop from the graph, exactly the
/// C++/CLI failure mode). The TSX grammar shares its node-kind vocabulary
/// with TypeScript, so only the parser changes.
fn tsx_language() -> Language {
    tree_sitter_typescript::LANGUAGE_TSX.into()
}
const TS_DIALECTS: DialectTable = &[("tsx", tsx_language)];

pub fn ruby() -> TsLangSpec {
    TsLangSpec {
        name: "Ruby",
        namespace_delim: "::",
        traits: LanguageTraits::HAS_CLASSES
            | LanguageTraits::ALLOWS_FORWARD_REFS
            | LanguageTraits::STRUCTURAL_TYPING
            | LanguageTraits::HAS_DEFAULT_ARGS,
        extensions: &["rb"],
        language: tree_sitter_ruby::LANGUAGE.into(),
        function_kinds: &["method", "singleton_method"],
        param_container_kinds: &["method_parameters", "parameters", "bare_parameters"],
        call_kinds: &["call", "command", "command_call"],
        callee_field: "method",
        assign_forms: &[
            AssignForm {
                kind: "assignment",
                lhs_field: "left",
                rhs_field: "right",
            },
            AssignForm {
                kind: "operator_assignment",
                lhs_field: "left",
                rhs_field: "right",
            },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: true,
        type_container_kinds: &[],
        receiver_field: None,
        implicit_receiver: Some(("self", "rb:<main>")),
        ctor_sugar_method: None,
        declarator_field: None,
        type_decl_kinds: &[],
        base_clause_kinds: &[],
        member_kinds: &[],
        smart_ptr_names: &[],
        ctor_factories: &[],
        preprocess: None,
        dialects: &[],
    }
}

pub fn rust() -> TsLangSpec {
    TsLangSpec {
        name: "Rust",
        namespace_delim: "::",
        traits: LanguageTraits::HAS_GENERICS
            | LanguageTraits::HAS_FUNCTION_POINTERS
            | LanguageTraits::STRUCTURAL_TYPING,
        extensions: &["rs"],
        language: tree_sitter_rust::LANGUAGE.into(),
        function_kinds: &["function_item"],
        param_container_kinds: &["parameters"],
        call_kinds: &["call_expression", "macro_invocation"],
        callee_field: "function",
        assign_forms: &[
            AssignForm {
                kind: "let_declaration",
                lhs_field: "pattern",
                rhs_field: "value",
            },
            AssignForm {
                kind: "assignment_expression",
                lhs_field: "left",
                rhs_field: "right",
            },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: true,
        type_container_kinds: &[],
        receiver_field: None,
        implicit_receiver: None,
        ctor_sugar_method: None,
        declarator_field: None,
        type_decl_kinds: &[],
        base_clause_kinds: &[],
        member_kinds: &[],
        smart_ptr_names: &[],
        ctor_factories: &[],
        preprocess: None,
        dialects: &[],
    }
}

pub fn python() -> TsLangSpec {
    TsLangSpec {
        name: "Python",
        namespace_delim: ".",
        traits: LanguageTraits::HAS_CLASSES
            | LanguageTraits::ALLOWS_FORWARD_REFS
            | LanguageTraits::STRUCTURAL_TYPING
            | LanguageTraits::HAS_DEFAULT_ARGS,
        extensions: &["py"],
        language: tree_sitter_python::LANGUAGE.into(),
        function_kinds: &["function_definition"],
        param_container_kinds: &["parameters"],
        call_kinds: &["call"],
        callee_field: "function",
        assign_forms: &[
            AssignForm {
                kind: "assignment",
                lhs_field: "left",
                rhs_field: "right",
            },
            AssignForm {
                kind: "augmented_assignment",
                lhs_field: "left",
                rhs_field: "right",
            },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: false,
        type_container_kinds: &[],
        receiver_field: None,
        implicit_receiver: None,
        ctor_sugar_method: None,
        declarator_field: None,
        type_decl_kinds: &[],
        base_clause_kinds: &[],
        member_kinds: &[],
        smart_ptr_names: &[],
        ctor_factories: &[],
        preprocess: None,
        dialects: &[],
    }
}

pub fn scala() -> TsLangSpec {
    TsLangSpec {
        name: "Scala",
        namespace_delim: ".",
        traits: LanguageTraits::HAS_CLASSES
            | LanguageTraits::HAS_GENERICS
            | LanguageTraits::HAS_OVERLOADING
            | LanguageTraits::HAS_DEFAULT_ARGS,
        extensions: &["scala", "sc"],
        language: tree_sitter_scala::LANGUAGE.into(),
        function_kinds: &["function_definition", "lambda_expression"],
        param_container_kinds: &["parameters"],
        call_kinds: &["call_expression"],
        callee_field: "function",
        assign_forms: &[
            AssignForm {
                kind: "val_definition",
                lhs_field: "pattern",
                rhs_field: "value",
            },
            AssignForm {
                kind: "var_definition",
                lhs_field: "pattern",
                rhs_field: "value",
            },
            AssignForm {
                kind: "assignment_expression",
                lhs_field: "left",
                rhs_field: "right",
            },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: true,
        type_container_kinds: &["object_definition", "class_definition", "trait_definition"],
        receiver_field: None,
        implicit_receiver: None,
        ctor_sugar_method: Some("apply"),
        declarator_field: None,
        type_decl_kinds: &[],
        base_clause_kinds: &[],
        member_kinds: &[],
        smart_ptr_names: &[],
        ctor_factories: &[],
        preprocess: None,
        dialects: &[],
    }
}

pub fn cpp() -> TsLangSpec {
    TsLangSpec {
        name: "C++",
        namespace_delim: "::",
        traits: LanguageTraits::HAS_CLASSES
            | LanguageTraits::HAS_GENERICS
            | LanguageTraits::HAS_OVERLOADING
            | LanguageTraits::HAS_FUNCTION_POINTERS,
        extensions: &["cpp", "cc", "cxx", "hpp", "hxx", "hh", "h", "ipp"],
        language: tree_sitter_cpp::LANGUAGE.into(),
        // Pure declarations (`void f(int);`) are `declaration` nodes, not
        // function_definitions, so header prototypes never become methods —
        // only real bodies (including inline/template ones) do.
        function_kinds: &["function_definition", "lambda_expression"],
        param_container_kinds: &["parameter_list"],
        call_kinds: &["call_expression"],
        callee_field: "function",
        assign_forms: &[
            AssignForm {
                kind: "init_declarator",
                lhs_field: "declarator",
                rhs_field: "value",
            },
            AssignForm {
                kind: "assignment_expression",
                lhs_field: "left",
                rhs_field: "right",
            },
        ],
        control_kinds: CONTROL,
        return_kinds: RETURNS,
        implicit_return: false,
        type_container_kinds: &[
            "class_specifier",
            "struct_specifier",
            "namespace_definition",
        ],
        receiver_field: None,
        implicit_receiver: None,
        ctor_sugar_method: None,
        declarator_field: Some("declarator"),
        type_decl_kinds: &["class_specifier", "struct_specifier"],
        base_clause_kinds: &["base_class_clause"],
        member_kinds: &["field_declaration"],
        smart_ptr_names: &[
            "shared_ptr",
            "unique_ptr",
            "weak_ptr",
            "scoped_ptr",
            "intrusive_ptr",
        ],
        ctor_factories: &[
            "make_shared",
            "make_unique",
            "allocate_shared",
            "make_scoped",
        ],
        preprocess: Some(cpp_cli_shim),
        dialects: &[],
    }
}
