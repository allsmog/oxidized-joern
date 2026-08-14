//! The framework-global source model (`sourceIdents`) end to end on the
//! Flask shape that motivated it: `request.args.get(..)` feeding a sink
//! through an opaque `.format(..)` and a helper call — with shadowing and
//! same-named in-repo methods trying to break it.

use cpg_cli::make_project;

const APP: &str = r#"
from flask import request
import os

def handler():
    cmd = request.args.get('cmd', '')
    full = "prefix {}".format(cmd)
    run_it(full)

def run_it(c):
    os.system(c)

def shadowed():
    request = make_local()
    os.system(request)
"#;

/// A same-named `format` method elsewhere in the module — name-based
/// resolution must NOT let its (empty) summary swallow the literal-receiver
/// `.format(..)` call in `handler`.
const DECOY: &str = r#"
class LogFormatter:
    def format(self, record):
        return "constant"
"#;

fn scan(sources_idents: &[&str]) -> Vec<cpg_analysis::Finding> {
    let (mut project, _) = make_project("python").unwrap();
    project.build(&[("app.py", APP), ("fmt.py", DECOY)]);
    project.find_taint_full(&[], &["system@0"], &[], &[], sources_idents)
}

#[test]
fn request_reaches_system_through_format_and_helper() {
    let findings = scan(&["request"]);
    assert_eq!(
        findings.iter().filter(|f| f.method == "handler").count(),
        1,
        "handler's request->format->run_it->system chain must fire: {findings:?}"
    );
    // The local named `request` shadows the framework global.
    assert!(
        findings.iter().all(|f| f.method != "shadowed"),
        "a local named `request` must not count as the flask global"
    );
}

#[test]
fn no_idents_no_findings() {
    assert!(
        scan(&[]).is_empty(),
        "without sourceIdents nothing is tainted"
    );
}
