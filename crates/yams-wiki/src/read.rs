//! Strict page parsing shared by read-only wiki views.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::{CapturedPageOutcome, ParsedWikiPage, WikiSnapshot, parse_wiki_page};

#[derive(Debug, Error)]
#[error("pages/{name}: {reason}")]
pub struct PageReadError {
    pub name: String,
    pub reason: String,
}

pub(crate) struct ReadPage<'a> {
    pub metadata: ParsedWikiPage,
    pub source: &'a str,
}

pub(crate) fn read_pages(
    snapshot: &WikiSnapshot,
) -> Result<BTreeMap<String, ReadPage<'_>>, PageReadError> {
    let mut pages = BTreeMap::new();
    for captured in &snapshot.pages {
        let invalid = |reason: String| PageReadError {
            name: captured.name.to_string_lossy().into_owned(),
            reason,
        };
        let name = captured
            .name
            .to_str()
            .ok_or_else(|| invalid("filename is not valid UTF-8".into()))?;
        let bytes = match &captured.outcome {
            CapturedPageOutcome::Present(bytes) => bytes,
            CapturedPageOutcome::NotRegular => return Err(invalid("not a regular file".into())),
            CapturedPageOutcome::Unreadable(_) => {
                return Err(invalid("page is not readable".into()));
            }
            CapturedPageOutcome::Symlink => return Err(invalid("symlink refused".into())),
            CapturedPageOutcome::OutsideCorpus => {
                return Err(invalid("page resolves outside the wiki".into()));
            }
        };
        let source =
            std::str::from_utf8(bytes).map_err(|_| invalid("page is not valid UTF-8".into()))?;
        let metadata = parse_wiki_page(source).map_err(|error| invalid(error.to_string()))?;
        if name != format!("{}.md", metadata.slug) {
            return Err(invalid(format!(
                "filename does not match slug {:?}",
                metadata.slug
            )));
        }
        if pages
            .insert(metadata.slug.clone(), ReadPage { metadata, source })
            .is_some()
        {
            return Err(invalid("duplicate page slug".into()));
        }
    }
    Ok(pages)
}
