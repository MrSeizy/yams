use std::fs;
use std::os::unix::fs::symlink;

use chrono::NaiveDate;
use tempfile::{TempDir, tempdir};
use yams_wiki::{
    HealthError, HealthKind, HealthOptions, capture_wiki, health_snapshot, health_wiki,
};

fn page(slug: &str, title: &str, body: &str) -> String {
    format!(
        "---\nslug: {slug}\ntitle: {title}\ntype: decision\nstatus: current\nowner: shared\nupdated: 2026-01-01\nverified: 2026-01-01\nsummary: A fictional {slug} summary.\n---\n\n{body}\n"
    )
}

fn wiki(pages: &[(&str, String)]) -> TempDir {
    let tmp = tempdir().unwrap();
    fs::create_dir(tmp.path().join("pages")).unwrap();
    fs::write(
        tmp.path().join("INDEX.md"),
        "A deliberately stale catalog.\n",
    )
    .unwrap();
    for (slug, source) in pages {
        fs::write(tmp.path().join("pages").join(format!("{slug}.md")), source).unwrap();
    }
    tmp
}

fn health_options() -> HealthOptions {
    HealthOptions {
        as_of: NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
        stale_days: 180,
    }
}

#[test]
fn advisory_duplicates_are_grouped_and_do_not_claim_contradictions() {
    let tmp = wiki(&[
        (
            "alpha",
            page("alpha", "Same title", "    preserve code indentation"),
        ),
        (
            "beta",
            page("beta", "SAME   TITLE", "    preserve code indentation"),
        ),
        (
            "gamma",
            page("gamma", "Third", "A different claim on the same topic."),
        ),
        (
            "old",
            page("old", "Same title", "    preserve code indentation")
                .replace("status: current", "status: historical"),
        ),
    ]);
    let report = health_wiki(
        tmp.path(),
        &HealthOptions {
            stale_days: 365,
            ..health_options()
        },
    )
    .unwrap();
    assert_eq!(report.page_count, 4);
    assert_eq!(report.current_pages, 3);
    assert_eq!(report.findings.len(), 2);
    assert_eq!(report.findings[0].kind, HealthKind::DuplicateTitle);
    assert_eq!(report.findings[1].kind, HealthKind::DuplicateBody);
    for finding in report.findings {
        assert_eq!(finding.paths, ["pages/alpha.md", "pages/beta.md"]);
    }
}

#[test]
fn body_comparison_does_not_collapse_internal_code_indentation() {
    let tmp = wiki(&[
        (
            "alpha",
            page("alpha", "Alpha", "Example:\n\n    call()\n    return"),
        ),
        ("beta", page("beta", "Beta", "Example:\n\ncall()\nreturn")),
    ]);
    let report = health_wiki(
        tmp.path(),
        &HealthOptions {
            stale_days: 365,
            ..health_options()
        },
    )
    .unwrap();
    assert!(report.findings.is_empty());
}

#[test]
fn health_uses_calendar_age_and_reports_bad_dates_without_invalidating_claims() {
    let tmp = wiki(&[
        (
            "due",
            page("due", "Due", "First fact.")
                .replace("verified: 2026-01-01", "verified: 2026-01-02"),
        ),
        (
            "recent",
            page("recent", "Recent", "Second fact.")
                .replace("verified: 2026-01-01", "verified: 2026-01-03"),
        ),
        (
            "future",
            page("future", "Future", "Third fact.")
                .replace("verified: 2026-01-01", "verified: 2026-07-02"),
        ),
        (
            "invalid",
            page("invalid", "Invalid", "Fourth fact.")
                .replace("verified: 2026-01-01", "verified: 2026-02-30"),
        ),
        (
            "unicode",
            page("unicode", "Unicode", "Fifth fact.")
                .replace("verified: 2026-01-01", "verified: ２０２６-０１-０１"),
        ),
    ]);
    let report = health_wiki(tmp.path(), &health_options()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.kind == HealthKind::VerificationDue && f.paths == ["pages/due.md"])
    );
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.paths == ["pages/recent.md"])
    );
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.kind == HealthKind::FutureVerification)
    );
    assert_eq!(
        report
            .findings
            .iter()
            .filter(|f| f.kind == HealthKind::UninterpretableVerification)
            .count(),
        2
    );
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.message.contains("Age alone does not invalidate"))
    );
}

#[test]
fn views_refuse_incomplete_snapshots_and_preserve_all_source_bytes() {
    let source = page("alpha", "Alpha", "A fictional fact.");
    let tmp = wiki(&[("alpha", source.clone())]);
    health_wiki(tmp.path(), &health_options()).unwrap();
    assert_eq!(
        fs::read_to_string(tmp.path().join("pages/alpha.md")).unwrap(),
        source
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("INDEX.md")).unwrap(),
        "A deliberately stale catalog.\n"
    );
    symlink(
        tmp.path().join("pages/alpha.md"),
        tmp.path().join("pages/unsafe.md"),
    )
    .unwrap();
    assert!(matches!(
        health_wiki(tmp.path(), &health_options()),
        Err(HealthError::InvalidPage(_))
    ));
    fs::remove_file(tmp.path().join("pages/unsafe.md")).unwrap();
    fs::write(tmp.path().join("pages/invalid.md"), "not a canonical page").unwrap();
    let snapshot = capture_wiki(tmp.path()).unwrap();
    assert!(matches!(
        health_snapshot(&snapshot, &health_options()),
        Err(HealthError::InvalidPage(_))
    ));
}

#[test]
fn invalid_options_and_missing_wikis_do_not_create_directories() {
    let tmp = tempdir().unwrap();
    let absent = tmp.path().join("absent");
    assert!(matches!(
        health_wiki(
            &absent,
            &HealthOptions {
                stale_days: 0,
                ..health_options()
            }
        ),
        Err(HealthError::InvalidThreshold)
    ));
    assert!(matches!(
        health_wiki(&absent, &health_options()),
        Err(HealthError::Capture(_))
    ));
    assert!(!absent.exists());
}
