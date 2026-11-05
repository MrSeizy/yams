//! Advisory knowledge maintenance, separate from structural validation.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::NaiveDate;
use serde::Serialize;
use thiserror::Error;

use crate::check::capture_existing_wiki;
use crate::read::read_pages;
use crate::{CheckError, PageReadError, Status, WikiSnapshot};

pub const DEFAULT_STALE_DAYS: u32 = 180;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HealthOptions {
    pub as_of: NaiveDate,
    /// Flag current pages whose verification is at least this many days old.
    pub stale_days: u32,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthKind {
    DuplicateTitle,
    DuplicateBody,
    VerificationDue,
    FutureVerification,
    UninterpretableVerification,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HealthFinding {
    pub kind: HealthKind,
    /// Relative wiki paths, sorted and deduplicated. Findings are advisory only.
    pub paths: Vec<String>,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HealthReport {
    pub as_of: String,
    pub stale_days: u32,
    pub page_count: usize,
    pub current_pages: usize,
    pub findings: Vec<HealthFinding>,
    pub isolated: bool,
    pub isolation_note: Option<String>,
}

#[derive(Debug, Error)]
pub enum HealthError {
    #[error("stale-days must be between 1 and 36500")]
    InvalidThreshold,
    #[error(transparent)]
    Capture(#[from] CheckError),
    #[error(transparent)]
    InvalidPage(#[from] PageReadError),
}

fn validate_options(options: &HealthOptions) -> Result<(), HealthError> {
    if !(1..=36_500).contains(&options.stale_days) {
        return Err(HealthError::InvalidThreshold);
    }
    Ok(())
}

/// Read-only advisory checks over current canonical pages. Findings never mutate
/// pages, alter search ranking, or imply that an old claim is false.
pub fn health_wiki(corpus: &Path, options: &HealthOptions) -> Result<HealthReport, HealthError> {
    validate_options(options)?;
    health_snapshot(&capture_existing_wiki(corpus)?, options)
}

pub fn health_snapshot(
    snapshot: &WikiSnapshot,
    options: &HealthOptions,
) -> Result<HealthReport, HealthError> {
    validate_options(options)?;
    let pages = read_pages(snapshot)?;
    let mut titles: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut bodies: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut findings = Vec::new();
    let mut current_pages = 0;
    for (slug, page) in &pages {
        if page.metadata.status != Status::Current {
            continue;
        }
        current_pages += 1;
        let path = format!("pages/{slug}.md");
        titles
            .entry(
                page.metadata
                    .title
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase(),
            )
            .or_default()
            .push(path.clone());
        // Preserve internal whitespace and code indentation. Only CRLF and outer
        // blank lines are normalized; similarity is not proof of duplication.
        let body = yams_core::parse_frontmatter(page.source)
            .body
            .replace("\r\n", "\n");
        let body = body.trim_matches('\n');
        if !body.trim().is_empty() {
            bodies
                .entry(body.to_owned())
                .or_default()
                .push(path.clone());
        }
        let verified = &page.metadata.verified;
        let date = verified
            .is_ascii()
            .then(|| NaiveDate::parse_from_str(verified, "%Y-%m-%d").ok())
            .flatten();
        let finding = match date {
            None => Some((
                HealthKind::UninterpretableVerification,
                format!(
                    "Cannot interpret verification date {verified:?} as an ASCII calendar date; review it before assessing age."
                ),
            )),
            Some(date) if date > options.as_of => Some((
                HealthKind::FutureVerification,
                format!("Verification date {verified} is after {}.", options.as_of),
            )),
            Some(date) => {
                let age = (options.as_of - date).num_days();
                (age >= i64::from(options.stale_days)).then(|| (HealthKind::VerificationDue,
                    format!("Last verified {age} days ago ({verified}); recheck the evidence. Age alone does not invalidate this claim.")))
            }
        };
        if let Some((kind, message)) = finding {
            findings.push(HealthFinding {
                kind,
                paths: vec![path],
                message,
            });
        }
    }
    for (groups, kind, message) in [
        (
            titles,
            HealthKind::DuplicateTitle,
            "Current pages share a title after case and whitespace normalization; review whether their scopes overlap.",
        ),
        (
            bodies,
            HealthKind::DuplicateBody,
            "Current pages have identical bodies after newline and outer-blank-line normalization; review whether one should supersede the other.",
        ),
    ] {
        for paths in groups.into_values().filter(|paths| paths.len() > 1) {
            findings.push(HealthFinding {
                kind,
                paths,
                message: message.into(),
            });
        }
    }
    findings.sort_by(|a, b| (a.kind, &a.paths).cmp(&(b.kind, &b.paths)));
    Ok(HealthReport {
        as_of: options.as_of.to_string(),
        stale_days: options.stale_days,
        page_count: pages.len(),
        current_pages,
        findings,
        isolated: snapshot.isolated,
        isolation_note: snapshot.isolation_note.clone(),
    })
}
