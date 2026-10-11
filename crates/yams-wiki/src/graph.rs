//! One-hop graph retrieval over the same captured pages and links as `check`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use thiserror::Error;

use crate::check::{capture_existing_wiki, wikilink_targets};
use crate::schema::validate_slug;
use crate::{CapturedPageOutcome, CheckError, Status, WikiSnapshot, parse_wiki_page};

/// Compact, source-attributed page metadata; bodies are never injected by expansion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GraphPage {
    pub slug: String,
    /// Path relative to the supplied wiki root.
    pub path: String,
    pub title: String,
    pub summary: String,
    pub status: Status,
    pub verified: String,
}

/// A page's immediate neighbours, sorted by slug and without duplicate or self edges.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GraphExpansion {
    pub page: GraphPage,
    pub outgoing: Vec<GraphPage>,
    pub backlinks: Vec<GraphPage>,
    /// Outgoing target slugs which have no page in this snapshot.
    pub unresolved: Vec<String>,
    pub isolated: bool,
    pub isolation_note: Option<String>,
}

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("expected a page slug: 1–64 lowercase ASCII letters, digits, or hyphens")]
    InvalidSlug,
    #[error("no page with slug {slug:?}")]
    NotFound { slug: String },
    #[error(transparent)]
    Capture(#[from] CheckError),
    #[error("cannot expand an incomplete graph: pages/{name}: {reason}")]
    InvalidPage { name: String, reason: String },
}

/// Read the wiki once through its shared lock and safe snapshot implementation.
///
/// No search store, model, or network is used. As with `check`, acquiring the
/// shared lock may create `.write.lock`; pages and the catalog are never changed.
pub fn expand_wiki(corpus: &Path, slug: &str) -> Result<GraphExpansion, GraphError> {
    validate_slug(slug).map_err(|_| GraphError::InvalidSlug)?;
    expand_snapshot(&capture_existing_wiki(corpus)?, slug)
}

/// Expand captured values without filesystem access or an embedding query.
///
/// Refuse malformed or unreadable pages instead of presenting missing backlinks
/// as a complete graph. Catalog drift does not affect edges: pages are authoritative.
pub fn expand_snapshot(snapshot: &WikiSnapshot, slug: &str) -> Result<GraphExpansion, GraphError> {
    validate_slug(slug).map_err(|_| GraphError::InvalidSlug)?;
    let mut pages = BTreeMap::new();
    let mut edges = BTreeMap::new();
    for captured in &snapshot.pages {
        let invalid = |reason: String| GraphError::InvalidPage {
            name: captured.name.to_string_lossy().into_owned(),
            reason,
        };
        let name = captured
            .name
            .to_str()
            .ok_or_else(|| invalid("filename is not valid UTF-8".to_owned()))?;
        let bytes = match &captured.outcome {
            CapturedPageOutcome::Present(bytes) => bytes,
            CapturedPageOutcome::NotRegular => {
                return Err(invalid("not a regular file".to_owned()));
            }
            CapturedPageOutcome::Unreadable(_) => {
                return Err(invalid("page is not readable".to_owned()));
            }
            CapturedPageOutcome::Symlink => {
                return Err(invalid("symlink refused".to_owned()));
            }
            CapturedPageOutcome::OutsideCorpus => {
                return Err(invalid("page resolves outside the wiki".to_owned()));
            }
        };
        let source = std::str::from_utf8(bytes)
            .map_err(|_| invalid("page is not valid UTF-8".to_owned()))?;
        let parsed = parse_wiki_page(source).map_err(|error| invalid(error.to_string()))?;
        if name != format!("{}.md", parsed.slug) {
            return Err(invalid(format!(
                "filename does not match slug {:?}",
                parsed.slug
            )));
        }
        let targets = wikilink_targets(source)
            .into_iter()
            .filter(|target| target != &parsed.slug)
            .collect::<BTreeSet<_>>();
        let page = GraphPage {
            path: format!("pages/{name}"),
            slug: parsed.slug,
            title: parsed.title,
            summary: parsed.summary,
            status: parsed.status,
            verified: parsed.verified,
        };
        edges.insert(page.slug.clone(), targets);
        if pages.insert(page.slug.clone(), page).is_some() {
            return Err(invalid("duplicate page slug".to_owned()));
        }
    }

    let page = pages.get(slug).ok_or_else(|| GraphError::NotFound {
        slug: slug.to_owned(),
    })?;
    let mut outgoing = Vec::new();
    let mut unresolved = Vec::new();
    for target in &edges[slug] {
        if let Some(page) = pages.get(target) {
            outgoing.push(page.clone());
        } else {
            unresolved.push(target.clone());
        }
    }
    let backlinks = edges
        .iter()
        .filter(|(_, targets)| targets.contains(slug))
        .map(|(source, _)| pages[source].clone())
        .collect();

    Ok(GraphExpansion {
        page: page.clone(),
        outgoing,
        backlinks,
        unresolved,
        isolated: snapshot.isolated,
        isolation_note: snapshot.isolation_note.clone(),
    })
}
