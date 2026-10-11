use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::symlink;

use tempfile::{TempDir, tempdir};
use yams_wiki::{
    CapturedPage, CapturedPageOutcome, GraphError, GraphPage, Status, capture_wiki,
    expand_snapshot, expand_wiki,
};

fn page(slug: &str, body: &str) -> String {
    format!(
        "---\nslug: {slug}\ntitle: Fictional {slug}\ntype: decision\nstatus: current\nowner: shared\nupdated: 2026-08-08\nverified: 2026-08-09\nsummary: A fictional {slug} decision\n---\n\n{body}\n"
    )
}

fn wiki(pages: &[(&str, String)]) -> TempDir {
    let tmp = tempdir().unwrap();
    fs::create_dir(tmp.path().join("pages")).unwrap();
    // Deliberately stale: graph retrieval must use the pages, not the catalog.
    fs::write(tmp.path().join("INDEX.md"), "An old catalog.\n").unwrap();
    for (name, contents) in pages {
        fs::write(tmp.path().join("pages").join(name), contents).unwrap();
    }
    tmp
}

fn slugs(pages: &[GraphPage]) -> Vec<&str> {
    pages.iter().map(|page| page.slug.as_str()).collect()
}

#[test]
fn expansion_returns_one_hop_in_each_direction_with_source_metadata() {
    let tmp = wiki(&[
        (
            "seed.md",
            page(
                "seed",
                "[[beta|Alias]] [[alpha#Details]] [[beta#Details|Again]] [[seed]] [[future]] [[future|Later]] ![[embedded]] [[block#^id]]",
            ),
        ),
        ("alpha.md", page("alpha", "[[seed|Back]] [[remote]]")),
        (
            "beta.md",
            page("beta", "Older decision").replace("status: current", "status: historical"),
        ),
        ("gamma.md", page("gamma", "[[seed#Details]] [[seed]]")),
        ("embedded.md", page("embedded", "No graph edge")),
        ("block.md", page("block", "No graph edge")),
        ("remote.md", page("remote", "Two hops from seed")),
    ]);
    let graph = expand_wiki(tmp.path(), "seed").unwrap();
    assert_eq!(slugs(&graph.outgoing), ["alpha", "beta"]);
    assert_eq!(slugs(&graph.backlinks), ["alpha", "gamma"]);
    assert_eq!(graph.unresolved, ["future"]);
    assert_eq!(graph.page.slug, "seed");
    assert_eq!(graph.page.path, "pages/seed.md");
    assert_eq!(graph.page.title, "Fictional seed");
    assert_eq!(graph.page.summary, "A fictional seed decision");
    assert_eq!(graph.page.verified, "2026-08-09");
    assert_eq!(graph.outgoing[1].status, Status::Historical);
    assert!(graph.isolated);
    assert_eq!(graph.isolation_note, None);
    assert_eq!(
        fs::read_to_string(tmp.path().join("INDEX.md")).unwrap(),
        "An old catalog.\n"
    );
}

#[test]
fn captured_graph_survives_filesystem_removal_and_is_independent_of_entry_order() {
    let tmp = wiki(&[
        (
            "seed.md",
            page("seed", "[[zeta]] [[alpha]] [[omega]] [[missing]]"),
        ),
        ("zeta.md", page("zeta", "[[seed]]")),
        ("alpha.md", page("alpha", "[[seed]]")),
    ]);
    let mut snapshot = capture_wiki(tmp.path()).unwrap();
    fs::remove_dir_all(tmp.path()).unwrap();
    let graph = expand_snapshot(&snapshot, "seed").unwrap();
    snapshot.pages.reverse();
    assert_eq!(expand_snapshot(&snapshot, "seed").unwrap(), graph);
    assert_eq!(slugs(&graph.outgoing), ["alpha", "zeta"]);
    assert_eq!(slugs(&graph.backlinks), ["alpha", "zeta"]);
    assert_eq!(graph.unresolved, ["missing", "omega"]);

    snapshot.isolated = false;
    snapshot.isolation_note = Some("read-only fixture".to_owned());
    let graph = expand_snapshot(&snapshot, "seed").unwrap();
    assert!(!graph.isolated);
    assert_eq!(graph.isolation_note.as_deref(), Some("read-only fixture"));
}

#[test]
fn missing_page_is_distinct_from_a_page_with_no_neighbours() {
    let tmp = wiki(&[("seed.md", page("seed", "An isolated decision"))]);
    let graph = expand_wiki(tmp.path(), "seed").unwrap();
    assert!(graph.outgoing.is_empty());
    assert!(graph.backlinks.is_empty());
    assert!(graph.unresolved.is_empty());
    assert!(
        matches!(expand_wiki(tmp.path(), "absent"), Err(GraphError::NotFound { slug }) if slug == "absent")
    );
}

#[test]
fn invalid_seed_is_rejected_before_any_filesystem_access() {
    let tmp = tempdir().unwrap();
    let missing = tmp.path().join("not-a-wiki");
    for slug in [
        "",
        "../seed",
        "pages/seed.md",
        "Seed",
        "seed.md",
        "/seed",
        "séed",
        &"x".repeat(65),
    ] {
        assert!(
            matches!(expand_wiki(&missing, slug), Err(GraphError::InvalidSlug)),
            "{slug:?}"
        );
    }
    assert!(!missing.exists());
}

#[test]
fn missing_wiki_inputs_are_rejected_without_leaving_a_lock() {
    let tmp = tempdir().unwrap();
    let corpus = tmp.path().join("missing");
    assert!(matches!(
        expand_wiki(&corpus, "seed"),
        Err(GraphError::Capture(_))
    ));
    assert!(!corpus.exists());
    fs::create_dir(&corpus).unwrap();
    assert!(matches!(
        expand_wiki(&corpus, "seed"),
        Err(GraphError::Capture(_))
    ));
    assert!(!corpus.join(".write.lock").exists());
    fs::create_dir(corpus.join("pages")).unwrap();
    assert!(matches!(
        expand_wiki(&corpus, "seed"),
        Err(GraphError::Capture(_))
    ));
    assert!(!corpus.join(".write.lock").exists());
    fs::create_dir(corpus.join("INDEX.md")).unwrap();
    assert!(matches!(
        expand_wiki(&corpus, "seed"),
        Err(GraphError::Capture(_))
    ));
    assert!(!corpus.join(".write.lock").exists());
}

#[test]
fn invalid_neighbour_never_produces_an_incomplete_success() {
    let tmp = wiki(&[("seed.md", page("seed", "A decision"))]);
    let clean = capture_wiki(tmp.path()).unwrap();
    let outcomes = [
        CapturedPageOutcome::NotRegular,
        CapturedPageOutcome::Symlink,
        CapturedPageOutcome::OutsideCorpus,
        CapturedPageOutcome::Unreadable("denied".to_owned()),
        CapturedPageOutcome::Present(vec![0xff]),
        CapturedPageOutcome::Present(page("wrong-slug", "A decision").into_bytes()),
        CapturedPageOutcome::Present(
            page("bad", "A decision")
                .replace("status: current", "status: unknown")
                .into_bytes(),
        ),
    ];
    for outcome in outcomes {
        let mut snapshot = clean.clone();
        snapshot.pages.push(CapturedPage {
            name: "bad.md".into(),
            outcome,
        });
        assert!(matches!(
            expand_snapshot(&snapshot, "seed"),
            Err(GraphError::InvalidPage { .. })
        ));
    }
    let mut snapshot = clean.clone();
    snapshot.pages.push(CapturedPage {
        name: OsString::from_vec(vec![0xff, b'.', b'm', b'd']),
        outcome: CapturedPageOutcome::Present(page("bad", "A decision").into_bytes()),
    });
    assert!(matches!(
        expand_snapshot(&snapshot, "seed"),
        Err(GraphError::InvalidPage { .. })
    ));

    let mut snapshot = clean;
    snapshot.pages.push(snapshot.pages[0].clone());
    assert!(
        matches!(expand_snapshot(&snapshot, "seed"), Err(GraphError::InvalidPage { reason, .. }) if reason == "duplicate page slug")
    );
}

#[test]
fn symlink_pages_are_refused_and_never_read_as_neighbours() {
    let tmp = wiki(&[("seed.md", page("seed", "[[outside]]"))]);
    let outside = tempdir().unwrap();
    fs::write(
        outside.path().join("outside.md"),
        page("outside", "[[seed]]"),
    )
    .unwrap();
    symlink(
        outside.path().join("outside.md"),
        tmp.path().join("pages/outside.md"),
    )
    .unwrap();
    assert!(
        matches!(expand_wiki(tmp.path(), "seed"), Err(GraphError::InvalidPage { reason, .. }) if reason == "symlink refused")
    );
}
