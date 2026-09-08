//! C format-string findings must reach the format argument, not another value.
use cpg_cli::{rules, scan};
use std::collections::BTreeSet;

fn format_findings(pack: &rules::RulePack) -> BTreeSet<String> {
    let mut project = cpg_incremental::Project::new(
        || Box::new(cpg_lang_c::CFrontend::new()),
        cpg_analysis::standard_pipeline(),
    );
    project.build(&[(
        "positions.c",
        include_str!("fixtures/c-format-positions/positions.c"),
    )]);
    scan::run_pack(&project, pack)
        .into_iter()
        .filter(|result| result.rule.id == "C-FMT-003")
        .flat_map(|result| result.findings.into_iter().map(|finding| finding.method))
        .collect()
}

fn expected_formats() -> BTreeSet<String> {
    [
        "printf_format",
        "fprintf_format",
        "syslog_format",
        "macro_format",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[test]
fn c_sink_selectors_use_zero_based_argument_positions() {
    let pack = rules::RulePack::from_json(
        r#"{"rules":[{"id":"C-FMT-003","sources":["getenv"],
             "sinks":["printf@0","fprintf@1","syslog@1"]}]}"#,
    )
    .unwrap();
    assert_eq!(format_findings(&pack), expected_formats());
}

#[test]
fn builtin_c_format_rule_excludes_data_and_stream_arguments() {
    let pack = rules::builtin_pack("c").unwrap();
    assert_eq!(format_findings(&pack), expected_formats());
}
