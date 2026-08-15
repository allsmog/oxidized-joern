//! Generate the populated sparse-schema graph used by the CPGQL differential.

use cpg_core::{Cpg, EdgeKind, NodeId, NodeKind, PropertyValue};

fn add(cpg: &mut Cpg, file: cpg_core::FileId, kind: NodeKind, label: &str) -> NodeId {
    let node = cpg.add_node(kind, file);
    let code = cpg.intern(&label.to_ascii_lowercase());
    cpg.set_code(node, code);
    cpg.set_line(node, node.0 + 1);
    node
}

fn string_property(cpg: &mut Cpg, node: NodeId, label: &str, value: &str) {
    let label = cpg.intern(label);
    let value = cpg.intern(value);
    cpg.set_passthrough_property(node, label, PropertyValue::Strings(vec![Some(value)]));
}

fn main() {
    let output = std::env::args()
        .nth(1)
        .expect("usage: cpgql_schema_fixture <output.cpg>");
    let mut cpg = Cpg::new();
    let file_id = cpg.file_id("schema-fixture.c");

    let file = add(&mut cpg, file_id, NodeKind::File, "FILE");
    let file_name = cpg.intern("schema-fixture.c");
    cpg.set_name(file, file_name);

    let metadata = add(&mut cpg, file_id, NodeKind::MetaData, "META_DATA");
    string_property(&mut cpg, metadata, "LANGUAGE", "C");
    string_property(&mut cpg, metadata, "VERSION", "0.1");
    let method = add(&mut cpg, file_id, NodeKind::Method, "METHOD");
    let method_name = cpg.intern("schemaMethod");
    let method_full_name = cpg.intern("schemaMethod:void()");
    let signature = cpg.intern("void()");
    cpg.set_name(method, method_name);
    cpg.set_full_name(method, method_full_name);
    cpg.set_signature(method, signature);
    cpg.add_edge(method, file, EdgeKind::SourceFile);

    let base = add(&mut cpg, file_id, NodeKind::TypeDecl, "TYPE_DECL");
    let base_name = cpg.intern("Base");
    let base_full_name = cpg.intern("schema.Base");
    cpg.set_name(base, base_name);
    cpg.set_full_name(base, base_full_name);

    let derived = add(&mut cpg, file_id, NodeKind::TypeDecl, "TYPE_DECL");
    let derived_name = cpg.intern("Derived");
    let derived_full_name = cpg.intern("schema.Derived");
    cpg.set_name(derived, derived_name);
    cpg.set_full_name(derived, derived_full_name);
    string_property(
        &mut cpg,
        derived,
        "INHERITS_FROM_TYPE_FULL_NAME",
        "schema.Base",
    );
    cpg.add_edge(derived, base, EdgeKind::InheritsFrom);

    let member = add(&mut cpg, file_id, NodeKind::Member, "MEMBER");
    let member_name = cpg.intern("value");
    cpg.set_name(member, member_name);
    cpg.add_edge(derived, member, EdgeKind::Ast);

    let typ = add(&mut cpg, file_id, NodeKind::Type, "TYPE");
    let type_name = cpg.intern("Derived");
    cpg.set_name(typ, type_name);
    cpg.set_full_name(typ, derived_full_name);

    let parameter = add(
        &mut cpg,
        file_id,
        NodeKind::MethodParameterIn,
        "METHOD_PARAMETER_IN",
    );
    let parameter_name = cpg.intern("arg");
    cpg.set_name(parameter, parameter_name);
    cpg.set_order(parameter, 1);
    cpg.set_argument_index(parameter, 1);
    let parameter_index = cpg.intern("INDEX");
    cpg.set_passthrough_property(parameter, parameter_index, PropertyValue::Ints(vec![1]));
    string_property(&mut cpg, parameter, "EVALUATION_STRATEGY", "BY_VALUE");
    cpg.add_edge(method, parameter, EdgeKind::Ast);

    let call = add(&mut cpg, file_id, NodeKind::Call, "CALL");
    let call_name = cpg.intern("schemaCall");
    let call_full_name = cpg.intern("schemaCall:void()");
    cpg.set_name(call, call_name);
    cpg.set_full_name(call, call_full_name);
    cpg.set_signature(call, signature);
    string_property(
        &mut cpg,
        call,
        "DYNAMIC_TYPE_HINT_FULL_NAME",
        "schema.Dynamic",
    );
    string_property(&mut cpg, call, "DISPATCH_TYPE", "STATIC_DISPATCH");
    let column = cpg.intern("COLUMN_NUMBER");
    cpg.set_passthrough_property(call, column, PropertyValue::Ints(vec![7]));
    cpg.add_edge(method, call, EdgeKind::Ast);

    let modifier = add(&mut cpg, file_id, NodeKind::Modifier, "MODIFIER");
    let modifier_name = cpg.intern("PUBLIC");
    cpg.set_name(modifier, modifier_name);
    string_property(&mut cpg, modifier, "MODIFIER_TYPE", "PUBLIC");

    let sparse_nodes = [
        (NodeKind::Annotation, "ANNOTATION"),
        (NodeKind::AnnotationLiteral, "ANNOTATION_LITERAL"),
        (NodeKind::AnnotationParameter, "ANNOTATION_PARAMETER"),
        (
            NodeKind::AnnotationParameterAssign,
            "ANNOTATION_PARAMETER_ASSIGN",
        ),
        (NodeKind::ArrayInitializer, "ARRAY_INITIALIZER"),
        (NodeKind::Binding, "BINDING"),
        (NodeKind::ClosureBinding, "CLOSURE_BINDING"),
        (NodeKind::Comment, "COMMENT"),
        (NodeKind::ConfigFile, "CONFIG_FILE"),
        (NodeKind::Dependency, "DEPENDENCY"),
        (NodeKind::Finding, "FINDING"),
        (NodeKind::Import, "IMPORT"),
        (NodeKind::JumpLabel, "JUMP_LABEL"),
        (NodeKind::KeyValuePair, "KEY_VALUE_PAIR"),
        (NodeKind::Tag, "TAG"),
        (NodeKind::TagNodePair, "TAG_NODE_PAIR"),
        (NodeKind::TemplateDom, "TEMPLATE_DOM"),
        (NodeKind::TypeArgument, "TYPE_ARGUMENT"),
    ];
    let mut annotation = None;
    let mut closure_binding = None;
    for (kind, label) in sparse_nodes {
        let node = add(&mut cpg, file_id, kind, label);
        if kind == NodeKind::Annotation {
            annotation = Some(node);
        }
        if kind == NodeKind::ClosureBinding {
            closure_binding = Some(node);
        }
    }
    cpg.add_edge(method, annotation.expect("annotation node"), EdgeKind::Ast);
    cpg.add_edge(
        closure_binding.expect("closure binding node"),
        call,
        EdgeKind::Capture,
    );

    cpg.save(&output).expect("save populated schema fixture");
}
