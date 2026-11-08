//! Prompts that hand a scan's problems to an agent: every failing or warning check that a filter
//! keeps, worst first, in one prompt.

use super::{CheckResult, Report, Status};

/// A prompt for every failing or warning check that `keep` keeps, worst first. A check the
/// external scan answered (a directory listing, an encyclopedia entry, a published package) is
/// mostly not fixed in the project's code: left out, or with `outside`, listed apart, for the agent
/// to do what the project can and report back the rest.
pub fn fix_prompt_where(
    report: &Report,
    outside: bool,
    keep: impl Fn(&CheckResult) -> bool,
) -> String {
    let mut list: Vec<&CheckResult> = report
        .checks
        .iter()
        .filter(|c| keep(c) && matches!(c.status, Status::Fail | Status::Warn))
        .filter(|c| outside || !c.external)
        .collect();
    list.sort_by_key(|c| (c.status != Status::Fail, c.tier as u8));
    let (code, beyond): (Vec<&CheckResult>, Vec<&CheckResult>) =
        list.into_iter().partition(|c| !c.external);
    let line = |c: &CheckResult| {
        format!(
            "- [{}] {}: {} Fix: {}\n",
            c.status.as_str(),
            c.name,
            c.details,
            c.fix
        )
    };
    let mut out = format!(
        "Fix these SEO and agent-readiness problems on {}:\n\n",
        report.url
    );
    for c in &code {
        out.push_str(&line(c));
    }
    if !beyond.is_empty() {
        out.push_str(if code.is_empty() {
            "These come from the outside scan and mostly need work outside this project's \
             code (listings, records, packages). Do what the project can for each, and tell me \
             what is left for me to do:\n"
        } else {
            "\nThese come from the outside scan and mostly need work outside this project's \
             code (listings, records, packages). Do what the project can for each, and tell me \
             what is left for me to do:\n"
        });
        for c in &beyond {
            out.push_str(&line(c));
        }
    }
    out.push_str("\nMake the changes in this project's code, then check again with browser_seo.");
    out
}

#[cfg(test)]
mod tests {
    use super::super::{CheckResult, Layer, Report, Status, Tier};
    use super::fix_prompt_where;

    fn check(id: &str, layer: Layer, status: Status, external: bool) -> CheckResult {
        CheckResult {
            id: id.into(),
            name: format!("Check {id}"),
            layer,
            tier: Tier::Required,
            status,
            score: 0.,
            max: 1.,
            details: format!("{id} found."),
            evidence: Vec::new(),
            fix: format!("Fix {id}."),
            spec: String::new(),
            seo: layer == Layer::Seo,
            external,
        }
    }

    /// One prompt holds every problem the filter keeps, failures first, passes left out; the
    /// checks that need outside work are listed apart, or left out.
    #[test]
    fn one_prompt_holds_every_problem_kept() {
        let report = Report {
            url: "https://example.com/".into(),
            checks: vec![
                check("warn", Layer::Discovery, Status::Warn, false),
                check("fail", Layer::Discovery, Status::Fail, false),
                check("pass", Layer::Discovery, Status::Pass, false),
                check("wiki", Layer::Discovery, Status::Fail, true),
                check("title", Layer::Seo, Status::Fail, false),
            ],
            ..Default::default()
        };
        let agents = fix_prompt_where(&report, true, |c| c.layer != Layer::Seo);
        let (fail, warn) = (
            agents.find("Check fail").unwrap(),
            agents.find("Check warn").unwrap(),
        );
        assert!(fail < warn, "{agents}");
        assert!(!agents.contains("Check pass") && !agents.contains("Check title"));
        let apart = agents.find("outside scan").unwrap();
        assert!(agents.find("Check wiki").unwrap() > apart, "{agents}");
        let code_only = fix_prompt_where(&report, false, |_| true);
        assert!(!code_only.contains("Check wiki") && code_only.contains("Check title"));
    }
}
