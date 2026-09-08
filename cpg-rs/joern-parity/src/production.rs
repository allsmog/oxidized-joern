//! Canonical adapter for the shipped C graph.
//!
//! This deliberately constructs a `cpg_incremental::Project` with
//! `cpg_lang_c::CFrontend` and `cpg_analysis::standard_pipeline`, exactly as
//! the released CLI does. During convergence the historical standalone dump
//! remains the required oracle path; `--migration-report` makes every
//! difference visible without normalising semantic gaps away.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Copy)]
pub enum Mode {
    Production,
    MigrationReport,
}

pub fn dump_paths(paths: &[String]) -> String {
    let sources: Vec<(String, String)> = paths
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("read production parity input {path}: {e}"));
            let name = Path::new(path)
                .file_name()
                .unwrap_or_else(|| panic!("input has no filename: {path}"))
                .to_string_lossy()
                .into_owned();
            (name, source)
        })
        .collect();
    dump_sources(&sources)
}

pub fn dump_sources(sources: &[(String, String)]) -> String {
    canonical_project(sources)
}

fn canonical_project(sources: &[(String, String)]) -> String {
    let refs: Vec<(&str, &str)> = sources
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&refs);
    cpg_lang_c::import::canonical_dump(&project.cpg)
}

/// Exercise the production incremental API on a real source set and compare
/// its complete canonical graph to a clean rebuild of the edited snapshot.
pub fn update_equivalence(paths: &[String]) -> Result<usize, String> {
    let mut sources: Vec<(String, String)> = paths
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(path)
                .map_err(|error| format!("read update-equivalence input {path}: {error}"))?;
            let name = Path::new(path)
                .file_name()
                .ok_or_else(|| format!("input has no filename: {path}"))?
                .to_string_lossy()
                .into_owned();
            Ok((name, source))
        })
        .collect::<Result<_, String>>()?;
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    let refs: Vec<(&str, &str)> = sources
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect();
    let mut incremental = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    incremental.build(&refs);

    sources[0]
        .1
        .push_str("\n/* cpg real-project incremental acceptance */\n");
    let outcome = incremental.update_file(&sources[0].0, &sources[0].1);
    if !matches!(outcome, cpg_incremental::UpdateOutcome::Rebuilt { .. }) {
        return Err(format!("production edit did not rebuild: {outcome:?}"));
    }
    let incremental_dump = cpg_lang_c::import::canonical_dump(&incremental.cpg);
    let clean_dump = canonical_project(&sources);
    if incremental_dump != clean_dump {
        return Err("incremental graph differs from a clean rebuild".to_string());
    }
    Ok(sources.len())
}

pub fn migration_report(standalone: &str, production: &str) -> String {
    let old = sections(standalone);
    let new = sections(production);
    let mut report = String::new();
    let mut total_removed = 0usize;
    let mut total_added = 0usize;
    for name in ["AST", "NODES", "EDGES", "FLOWS"] {
        let a = &old[name];
        let b = &new[name];
        let common = a.intersection(b).count();
        let removed = a.difference(b).count();
        let added = b.difference(a).count();
        total_removed += removed;
        total_added += added;
        report.push_str(&format!(
            "SECTION {name} standalone={} production={} common={common} removed={removed} added={added} exact={}\n",
            a.len(),
            b.len(),
            removed == 0 && added == 0
        ));
    }
    report.push_str(&format!(
        "TOTAL removed={total_removed} added={total_added} exact={}\n",
        total_removed == 0 && total_added == 0
    ));
    report
}

fn sections(text: &str) -> BTreeMap<&'static str, BTreeSet<String>> {
    let mut result: BTreeMap<&'static str, BTreeSet<String>> = [
        ("AST", BTreeSet::new()),
        ("NODES", BTreeSet::new()),
        ("EDGES", BTreeSet::new()),
        ("FLOWS", BTreeSet::new()),
    ]
    .into_iter()
    .collect();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let (section, value) = if let Some(value) = line.strip_prefix("NODES|") {
            ("NODES", value)
        } else if let Some(value) = line.strip_prefix("EDGES|") {
            ("EDGES", value)
        } else if let Some(value) = line.strip_prefix("FLOWS|") {
            ("FLOWS", value)
        } else {
            ("AST", line.strip_prefix("AST|").unwrap_or(line))
        };
        result.get_mut(section).unwrap().insert(value.to_string());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_dump_is_deterministic_and_uses_standard_layers() {
        let sources = vec![
            (
                "b.c".to_string(),
                "int twice(int x) { return x + x; }".to_string(),
            ),
            (
                "a.c".to_string(),
                "int main(void) { return twice(2); }".to_string(),
            ),
        ];
        let first = dump_sources(&sources);
        let second = dump_sources(&sources);
        assert_eq!(first, second);
        assert!(first.contains("METHOD NAME=main"));
        assert!(first.contains("EDGES|CFG "));
        assert!(first.contains("FLOWS|REACHING_DEF["));
    }

    #[test]
    fn migration_report_is_stable_and_sectioned() {
        let report = migration_report(
            "METHOD NAME=a\nNODES|FILE NAME=a.c\n",
            "METHOD NAME=b\nNODES|FILE NAME=a.c\n",
        );
        assert!(report.contains(
            "SECTION AST standalone=1 production=1 common=0 removed=1 added=1 exact=false"
        ));
        assert!(report.contains(
            "SECTION NODES standalone=1 production=1 common=1 removed=0 added=0 exact=true"
        ));
        assert!(report.ends_with("TOTAL removed=1 added=1 exact=false\n"));
    }

    #[test]
    fn production_update_matches_a_clean_rebuild() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.c");
        let b = dir.path().join("b.c");
        std::fs::write(&a, "int id(int x) { return x; }").unwrap();
        std::fs::write(&b, "int main(void) { return id(1); }").unwrap();
        let paths = vec![
            a.to_string_lossy().into_owned(),
            b.to_string_lossy().into_owned(),
        ];
        assert_eq!(update_equivalence(&paths).unwrap(), 2);
    }

    fn braceless_if_dump() -> String {
        let sources = vec![(
            "braceless_if.c".to_string(),
            include_str!("../corpus/braceless_if.c").to_string(),
        )];
        dump_sources(&sources)
    }

    #[test]
    fn production_preserves_braceless_if_call() {
        assert!(braceless_if_dump().contains("CALL NAME=braceless_sink "));
    }

    #[test]
    fn production_preserves_braceless_if_else_calls() {
        let dump = braceless_if_dump();
        for call in ["braceless_true", "braceless_false"] {
            assert!(
                dump.contains(&format!("CALL NAME={call} ")),
                "missing conditional call {call}"
            );
        }
    }

    #[test]
    fn production_preserves_braceless_if_return() {
        let dump = braceless_if_dump();
        let method = dump
            .split("\n\n")
            .find(|block| block.starts_with("METHOD NAME=braceless_if_return "))
            .expect("braceless return method");
        assert!(method.contains("RETURN CODE=return x; "));
        assert!(method.contains("RETURN CODE=return 0; "));
    }

    fn conditional_dump(file: &str, source: &str) -> String {
        dump_sources(&[(file.to_string(), source.to_string())])
    }

    #[test]
    fn production_retains_all_conditional_declarations_and_only_active_bodies() {
        let dump = conditional_dump(
            "conditional_top_level.c",
            include_str!("../corpus/conditional_top_level.c"),
        );
        for (name, active) in [
            ("pp_if", true),
            ("pp_nested_false", false),
            ("pp_nested_elif", true),
            ("pp_nested_else", false),
            ("pp_elif", false),
            ("pp_else", false),
            ("pp_ifndef", true),
            ("pp_false", false),
            ("pp_default", true),
            ("pp_defined", true),
        ] {
            let method = dump
                .split("\n\n")
                .find(|block| block.starts_with(&format!("METHOD NAME={name} ")))
                .unwrap_or_else(|| panic!("missing conditional method {name}"));
            assert!(method.contains("METHOD_PARAMETER_IN NAME=x "));
            assert!(method.contains("BLOCK CODE="));
            assert_eq!(method.contains("RETURN CODE=return"), active, "{name}");
        }
        for declaration in [
            "TYPE_DECL NAME=pp_word ",
            "TYPE_DECL NAME=pp_inactive_word ",
            "LOCAL NAME=pp_global ",
            "LOCAL NAME=pp_inactive_global ",
            "TYPE_DECL NAME=PpBox ",
            "TYPE_DECL NAME=PpInactiveBox ",
        ] {
            assert!(dump.contains(declaration), "missing {declaration}");
        }
        assert!(dump.contains("CALL NAME=<operator>.assignment CODE=pp_inactive_global = 9 "));
        assert!(dump.contains("NODES|TYPE NAME=pp_inactive_word "));
    }

    #[test]
    fn production_selects_conditions_in_source_order_without_losing_inactive_methods() {
        let dump = conditional_dump(
            "conditional_configuration.c",
            include_str!("../corpus/conditional_configuration.c"),
        );
        for (name, active) in [
            ("selected_expression", true),
            ("unselected_expression", false),
            ("after_undef_bad", false),
            ("after_undef_good", true),
            ("before_define_bad", false),
            ("before_define_good", true),
            ("inactive_define_bad", false),
            ("inactive_define_good", true),
        ] {
            let method = dump
                .split("\n\n")
                .find(|block| block.starts_with(&format!("METHOD NAME={name} ")))
                .unwrap_or_else(|| panic!("missing conditional method {name}"));
            assert_eq!(method.contains("RETURN CODE=return"), active, "{name}");
        }
        assert!(dump.contains("CALL NAME=STEP CODE=STEP(x)"));
        assert!(dump.contains("DISPATCH_TYPE=INLINED"));
        assert!(dump.contains("CODE=#define STEP(x) ((x) + 1)"));
        assert!(!dump.contains("CODE=#define STEP(x) ((x) + 2)"));
    }

    #[test]
    fn production_keeps_same_name_conditional_methods_distinct() {
        let dump = conditional_dump(
            "conditional_duplicate.c",
            include_str!("../corpus/conditional_duplicate.c"),
        );
        let methods: Vec<_> = dump
            .split("\n\n")
            .filter(|block| block.starts_with("METHOD NAME=duplicate "))
            .collect();
        assert_eq!(methods.len(), 2);
        assert!(methods[0].contains("FULL_NAME=duplicate SIGNATURE="));
        assert!(!methods[0].contains("RETURN CODE=return"));
        assert!(methods[1].contains("FULL_NAME=duplicate<duplicate>0 SIGNATURE="));
        assert!(methods[1].contains("RETURN CODE=return x + 2;"));
        assert!(dump.contains(
            "CALL NAME=duplicate CODE=duplicate(x) TYPE_FULL_NAME=int METHOD_FULL_NAME=duplicate "
        ));
        assert!(dump.contains("NODES|TYPE_DECL NAME=duplicate FULL_NAME=duplicate<duplicate>0 "));
    }

    #[test]
    fn production_respects_macro_replacement_precedence_and_condition_expressions() {
        let dump = conditional_dump(
            "conditional_evaluation.c",
            include_str!("../corpus/conditional_evaluation.c"),
        );
        for (name, active) in [
            ("precedence_active", true),
            ("precedence_wrong", false),
            ("ternary_active", true),
            ("ternary_wrong", false),
            ("character_active", true),
            ("character_wrong", false),
        ] {
            let method = dump
                .split("\n\n")
                .find(|block| block.starts_with(&format!("METHOD NAME={name} ")))
                .unwrap();
            assert_eq!(method.contains("RETURN CODE=return"), active, "{name}");
        }
    }

    #[test]
    fn production_preserves_macro_expansion_before_a_later_undef() {
        let dump = conditional_dump(
            "conditional_macro_order.c",
            include_str!("../corpus/conditional_macro_order.c"),
        );
        for name in ["before_undef", "<global>"] {
            let method = dump
                .split("\n\n")
                .find(|block| {
                    block.starts_with(&format!("METHOD NAME={name} "))
                        && block
                            .lines()
                            .next()
                            .unwrap()
                            .contains("conditional_macro_order.c")
                            == (name == "<global>")
                })
                .unwrap();
            assert!(method.contains("CALL NAME=ORDER_STEP "), "{name}");
            assert!(method.contains("DISPATCH_TYPE=INLINED"), "{name}");
        }
        assert!(!dump.contains("METHOD NAME=ORDER_STEP FULL_NAME=ORDER_STEP"));
    }

    #[test]
    fn production_counts_imports_in_all_conditional_branches() {
        let dump = conditional_dump(
            "conditional_includes.c",
            include_str!("../corpus/conditional_includes.c"),
        );
        let global_type = dump
            .lines()
            .find(|line| {
                line.starts_with(
                    "NODES|TYPE_DECL NAME=<global> FULL_NAME=conditional_includes.c:<global> ",
                )
            })
            .unwrap();
        assert!(global_type.ends_with("ORDER=3"));
    }

    #[test]
    fn production_removes_comments_before_interpreting_directives() {
        let dump = conditional_dump(
            "conditional_comments.c",
            include_str!("../corpus/conditional_comments.c"),
        );
        for prefix in [
            "defined_comment",
            "defined_space",
            "multiline",
            "macro_comment",
        ] {
            for (suffix, active) in [("active", true), ("wrong", false)] {
                let name = format!("{prefix}_{suffix}");
                let method = dump
                    .split("\n\n")
                    .find(|block| block.starts_with(&format!("METHOD NAME={name} ")))
                    .unwrap();
                assert_eq!(method.contains("RETURN CODE=return"), active, "{name}");
            }
        }
    }
}
