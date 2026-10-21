use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use assert_cmd::cargo::cargo_bin_cmd;
use chrono::Local;
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};
use yams_core::MAX_FILE_BYTES;

const BEGIN_MARKER: &str =
    "<!-- BEGIN GENERATED INDEX — edited by yams-wiki catalog, not by hand -->";
const END_MARKER: &str = "<!-- END GENERATED INDEX -->";
const PAGE: &str = "---\n\
slug: alpha\n\
title: Alpha beacon\n\
type: gotcha\n\
status: current\n\
owner: shared\n\
updated: 2026-08-08\n\
verified: 2026-08-08\n\
summary: a fictional beacon needs violet mode\n\
---\n\n\
A fictional beacon needs violet mode.\n";

struct Wiki {
    _temporary: TempDir,
    path: PathBuf,
}

impl Wiki {
    fn with_index(index: &str) -> Self {
        let temporary = tempdir().unwrap();
        let path = temporary.path().join("wiki");
        fs::create_dir(&path).unwrap();
        fs::create_dir(path.join("pages")).unwrap();
        fs::write(path.join("pages/alpha.md"), PAGE).unwrap();
        fs::write(path.join("INDEX.md"), index).unwrap();
        Self {
            _temporary: temporary,
            path,
        }
    }

    fn canonical() -> Self {
        Self::with_index(&format!(
            "{BEGIN_MARKER}\n\n## Gotchas\n\n- [alpha](pages/alpha.md) — a fictional beacon needs violet mode\n\n{END_MARKER}\n"
        ))
    }

    fn stale() -> Self {
        Self::with_index(&format!("{BEGIN_MARKER}\n\nstale\n\n{END_MARKER}\n"))
    }

    fn index(&self) -> String {
        fs::read_to_string(self.path.join("INDEX.md")).unwrap()
    }
}

fn one_json_line(output: &[u8]) -> Value {
    assert!(
        output.ends_with(b"\n"),
        "output lacks final newline: {output:?}"
    );
    assert_eq!(
        output.iter().filter(|byte| **byte == b'\n').count(),
        1,
        "output was not exactly one line: {output:?}"
    );
    serde_json::from_slice(&output[..output.len() - 1]).unwrap()
}

fn create_request() -> Value {
    json!({
        "title": "Violet mode wakes the beacon",
        "type": "gotcha",
        "owner": "codex",
        "fact": "A fictional beacon wakes only in violet mode.",
        "why": "Synthetic trials observed a signal only in violet mode.",
        "how_to_apply": "Enable violet mode before starting the beacon.",
        "falsified_by": "A signal from the fictional beacon in amber mode.",
        "summary": "violet mode wakes the fictional beacon",
        "related": ["alpha"]
    })
}

fn retired_begin_marker() -> String {
    let brand = String::from_utf8(vec![0x6d, 0x6f, 0x6e, 0x65, 0x74, 0x61]).unwrap();
    format!("<!-- BEGIN GENERATED INDEX — edited by {brand}-wiki reindex, not by hand -->")
}

#[test]
fn clap_usage_errors_exit_two() {
    cargo_bin_cmd!("yams-wiki").assert().code(2).stdout("");

    let wiki = Wiki::canonical();
    cargo_bin_cmd!("yams-wiki")
        .args(["catalog", wiki.path.to_str().unwrap(), "--check", "--adopt"])
        .assert()
        .code(2)
        .stdout("");
}

#[test]
fn capabilities_are_one_stable_json_line() {
    let output = cargo_bin_cmd!("yams-wiki")
        .args(["capabilities", "--json"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    let body = one_json_line(&output.stdout);
    assert_eq!(
        body,
        json!({
            "ok": true,
            "yams_version": env!("CARGO_PKG_VERSION"),
            "contracts": {
                "search_results": 1,
                "repository_layout": 1,
                "init_manifest": 4,
                "wiki_maintenance": 3,
                "graph_expansion": 1,
                "supersession": 1
            }
        })
    );

    let version = serde_json::to_string(env!("CARGO_PKG_VERSION")).unwrap();
    let expected = format!(
        "{{\"ok\":true,\"yams_version\":{version},\"contracts\":{{\"search_results\":1,\"repository_layout\":1,\"init_manifest\":4,\"wiki_maintenance\":3,\"graph_expansion\":1,\"supersession\":1}}}}\n"
    );
    assert_eq!(output.stdout, expected.as_bytes());
}

#[test]
fn capabilities_requires_the_json_flag() {
    cargo_bin_cmd!("yams-wiki")
        .arg("capabilities")
        .assert()
        .code(2)
        .stdout("");
}

#[test]
fn expand_returns_one_json_line_without_a_search_store() {
    let wiki = Wiki::stale();
    fs::write(
        wiki.path.join("pages/alpha.md"),
        format!("{PAGE}\n[[beta|Next]] [[future]]\n"),
    )
    .unwrap();
    fs::write(
        wiki.path.join("pages/beta.md"),
        format!(
            "{}\n[[alpha#Details]]\n",
            PAGE.replace("slug: alpha", "slug: beta")
        ),
    )
    .unwrap();
    let before = wiki.index();
    let output = cargo_bin_cmd!("yams-wiki")
        .args(["expand", wiki.path.to_str().unwrap(), "alpha", "--json"])
        .env("YAMS_HOME", wiki.path.join("no-search-store"))
        .env("YAMS_ALLOW_NET", "0")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    let body = one_json_line(&output.stdout);
    assert_eq!(body["ok"], true);
    assert_eq!(body["graph"]["page"]["path"], "pages/alpha.md");
    assert_eq!(body["graph"]["page"]["status"], "current");
    assert_eq!(body["graph"]["outgoing"][0]["slug"], "beta");
    assert_eq!(body["graph"]["backlinks"][0]["slug"], "beta");
    assert_eq!(body["graph"]["unresolved"], json!(["future"]));
    assert_eq!(body["graph"]["isolated"], true);
    assert_eq!(wiki.index(), before);
    assert!(!wiki.path.join("no-search-store").exists());
}

#[test]
fn expand_distinguishes_missing_seed_invalid_seed_and_operational_failure() {
    let empty = tempdir().unwrap();
    let output = cargo_bin_cmd!("yams-wiki")
        .args(["expand", empty.path().to_str().unwrap(), "alpha", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(one_json_line(&output.stdout)["code"], "capture_failed");
    assert!(!empty.path().join(".write.lock").exists());

    let wiki = Wiki::canonical();
    for (slug, code, exit) in [
        ("missing", "page_not_found", 1),
        ("../alpha", "invalid_slug", 2),
    ] {
        let output = cargo_bin_cmd!("yams-wiki")
            .args(["expand", wiki.path.to_str().unwrap(), slug, "--json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(exit));
        assert_eq!(output.stderr, b"");
        let body = one_json_line(&output.stdout);
        assert_eq!(body["ok"], false);
        assert_eq!(body["code"], code);
        assert!(body.get("graph").is_none());
    }

    fs::write(wiki.path.join("pages/broken.md"), "not a canonical page").unwrap();
    let output = cargo_bin_cmd!("yams-wiki")
        .args(["expand", wiki.path.to_str().unwrap(), "alpha", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(output.stderr, b"");
    let body = one_json_line(&output.stdout);
    assert_eq!(body["code"], "invalid_page");
    assert!(body.get("graph").is_none());

    fs::remove_file(wiki.path.join("INDEX.md")).unwrap();
    let output = cargo_bin_cmd!("yams-wiki")
        .args(["expand", wiki.path.to_str().unwrap(), "alpha", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(one_json_line(&output.stdout)["code"], "capture_failed");
}

#[test]
fn expand_text_reports_an_isolated_page_and_missing_page_on_separate_channels() {
    let wiki = Wiki::canonical();
    let output = cargo_bin_cmd!("yams-wiki")
        .args(["expand", wiki.path.to_str().unwrap(), "alpha"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!(
            "alpha [current] — Alpha beacon\n",
            "  pages/alpha.md (verified 2026-08-08)\n",
            "  a fictional beacon needs violet mode\n",
            "\nOutgoing (0):\n\nBacklinks (0):\n\nUnresolved (0):\n",
        )
    );
    cargo_bin_cmd!("yams-wiki")
        .args(["expand", wiki.path.to_str().unwrap(), "missing"])
        .assert()
        .code(1)
        .stdout("")
        .stderr("no page with slug \"missing\"\n");
}

#[test]
fn expand_preserves_unisolated_read_only_diagnostics_in_both_output_modes() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let wiki = Wiki::canonical();
    if fs::metadata(&wiki.path).unwrap().uid() == 0 {
        return;
    }
    struct RestoreModes(Vec<(PathBuf, fs::Permissions)>);
    impl Drop for RestoreModes {
        fn drop(&mut self) {
            for (path, permissions) in &self.0 {
                let _ = fs::set_permissions(path, permissions.clone());
            }
        }
    }
    let paths = [wiki.path.clone(), wiki.path.join("pages")];
    let files = [wiki.path.join("INDEX.md"), wiki.path.join("pages/alpha.md")];
    let _restore = RestoreModes(
        paths
            .iter()
            .chain(&files)
            .map(|path| (path.clone(), fs::metadata(path).unwrap().permissions()))
            .collect(),
    );
    for path in &paths {
        fs::set_permissions(path, fs::Permissions::from_mode(0o555)).unwrap();
    }
    for path in &files {
        fs::set_permissions(path, fs::Permissions::from_mode(0o444)).unwrap();
    }
    let output = cargo_bin_cmd!("yams-wiki")
        .args(["expand", wiki.path.to_str().unwrap(), "alpha", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    let body = one_json_line(&output.stdout);
    assert_eq!(body["graph"]["isolated"], false);
    let note = body["graph"]["isolation_note"].as_str().unwrap();
    assert_eq!(note, "unwritable corpus");
    let output = cargo_bin_cmd!("yams-wiki")
        .args(["expand", wiki.path.to_str().unwrap(), "alpha"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!("graph read without isolation: {note}\n")
    );
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("alpha [current]")
    );
    assert!(!wiki.path.join(".write.lock").exists());
}

#[test]
fn check_accepts_a_path_and_is_silent_when_the_wiki_is_clean() {
    let wiki = Wiki::canonical();

    cargo_bin_cmd!("yams-wiki")
        .args(["check", wiki.path.to_str().unwrap()])
        .assert()
        .success()
        .stdout("")
        .stderr("");
}

#[test]
fn check_routes_validation_findings_to_stderr_and_exits_one() {
    let wiki = Wiki::canonical();
    fs::write(
        wiki.path.join("pages/alpha.md"),
        PAGE.replace("title: Alpha beacon\n", ""),
    )
    .unwrap();

    cargo_bin_cmd!("yams-wiki")
        .args(["check", wiki.path.to_str().unwrap()])
        .assert()
        .code(1)
        .stdout("")
        .stderr("pages/alpha.md is missing a title\n");
}

#[test]
fn check_routes_raw_notes_to_stdout_without_changing_success() {
    let wiki = Wiki::canonical();
    fs::write(
        wiki.path.join("pages/alpha.md"),
        PAGE.replace(
            "A fictional beacon needs violet mode.",
            "A fictional beacon needs violet mode. See [[future-moon]].",
        ),
    )
    .unwrap();

    cargo_bin_cmd!("yams-wiki")
        .args(["check", wiki.path.to_str().unwrap()])
        .assert()
        .success()
        .stdout("pages/alpha.md links [[future-moon]], not yet written\n")
        .stderr("");
}

#[test]
fn reindex_accepts_a_path_and_replaces_a_stale_index() {
    let wiki = Wiki::stale();

    cargo_bin_cmd!("yams-wiki")
        .args(["catalog", wiki.path.to_str().unwrap()])
        .assert()
        .success()
        .stdout("INDEX.md rewritten.\n")
        .stderr("");

    assert!(wiki.index().contains("pages/alpha.md"));
    assert!(!wiki.index().contains("stale"));
}

#[test]
fn reindex_reports_an_unchanged_canonical_index() {
    let wiki = Wiki::canonical();

    cargo_bin_cmd!("yams-wiki")
        .args(["catalog", wiki.path.to_str().unwrap()])
        .assert()
        .success()
        .stdout("INDEX.md unchanged.\n")
        .stderr("");
}

#[test]
fn reindex_check_refuses_drift_without_printing_or_applying_the_internal_diff() {
    let wiki = Wiki::stale();
    let before = wiki.index();

    let output = cargo_bin_cmd!("yams-wiki")
        .args(["catalog", wiki.path.to_str().unwrap(), "--check"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        output.stdout,
        b"INDEX.md differs from what catalog would produce.\n"
    );
    assert_eq!(output.stderr, b"");
    assert!(
        !output
            .stdout
            .windows(12)
            .any(|bytes| bytes == b"--- INDEX.md")
    );
    assert_eq!(wiki.index(), before);
}

#[test]
fn reindex_check_reports_an_up_to_date_index() {
    let wiki = Wiki::canonical();

    cargo_bin_cmd!("yams-wiki")
        .args(["catalog", wiki.path.to_str().unwrap(), "--check"])
        .assert()
        .success()
        .stdout("INDEX.md is up to date.\n")
        .stderr("");
}

#[test]
fn reindex_adopt_accepts_the_exact_legacy_shape() {
    let legacy = "A fictional handbook.\n\n\
## Gotchas — tooling and environment\n\n\
- [alpha](pages/alpha.md) — a fictional beacon needs violet mode\n\n\
## Gotchas — retrieval traps\n\n\
## Decisions\n\n\
## Patterns\n\n\
## Workflow\n\n\
## Features — architecture pointers\n";
    let wiki = Wiki::with_index(legacy);

    cargo_bin_cmd!("yams-wiki")
        .args(["catalog", wiki.path.to_str().unwrap(), "--adopt"])
        .assert()
        .success()
        .stdout("INDEX.md rewritten.\n")
        .stderr("");

    let index = wiki.index();
    assert!(index.starts_with("A fictional handbook.\n\n"));
    assert_eq!(index.matches(BEGIN_MARKER).count(), 1);
    assert_eq!(index.matches(END_MARKER).count(), 1);
    assert!(index.contains("pages/alpha.md"));
}

#[test]
fn reindex_adopt_upgrades_the_retired_marker_for_normal_operations() {
    let retired = retired_begin_marker();
    let original = format!(
        "A fictional handbook.\r\n\r\n{retired}\r\n\n## Gotchas\n\n\
- [alpha](pages/alpha.md) — a fictional beacon needs violet mode\n\n{END_MARKER}\r\n\
Curated footer remains exact.\r\n"
    );
    let wiki = Wiki::with_index(&original);

    cargo_bin_cmd!("yams-wiki")
        .args(["catalog", wiki.path.to_str().unwrap(), "--adopt"])
        .assert()
        .success()
        .stdout("INDEX.md rewritten.\n")
        .stderr("");

    let upgraded = wiki.index();
    assert!(upgraded.starts_with(&format!("A fictional handbook.\r\n\r\n{BEGIN_MARKER}\r")));
    assert!(upgraded.ends_with(&format!(
        "{END_MARKER}\r\nCurated footer remains exact.\r\n"
    )));
    assert!(!upgraded.contains(&retired));

    cargo_bin_cmd!("yams-wiki")
        .args(["catalog", wiki.path.to_str().unwrap(), "--check"])
        .assert()
        .success()
        .stdout("INDEX.md is up to date.\n")
        .stderr("");
    cargo_bin_cmd!("yams-wiki")
        .args(["catalog", wiki.path.to_str().unwrap()])
        .assert()
        .success()
        .stdout("INDEX.md unchanged.\n")
        .stderr("");

    cargo_bin_cmd!("yams-wiki")
        .args(["write", wiki.path.to_str().unwrap()])
        .write_stdin(serde_json::to_vec(&create_request()).unwrap())
        .assert()
        .success()
        .stderr("");
    assert!(
        wiki.index()
            .contains("pages/violet-mode-wakes-the-beacon.md")
    );
}

#[test]
fn reindex_safe_refusals_exit_two_and_operational_errors_exit_four() {
    let unsafe_shape = Wiki::with_index("no generated markers\n");
    cargo_bin_cmd!("yams-wiki")
        .args(["catalog", unsafe_shape.path.to_str().unwrap()])
        .assert()
        .code(2)
        .stdout("")
        .stderr(
            "catalog refused: unsafe INDEX.md shape: expected exactly one BEGIN marker, found 0\n",
        );

    let root = tempdir().unwrap();
    let missing = root.path().join("missing-wiki");
    let operational = cargo_bin_cmd!("yams-wiki")
        .args(["catalog", missing.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(operational.status.code(), Some(4));
    assert_eq!(operational.stdout, b"");
    assert!(!operational.stderr.is_empty());
}

#[test]
fn write_reads_stdin_uses_the_local_date_and_prints_one_json_object() {
    let wiki = Wiki::canonical();
    let before = Local::now().date_naive().to_string();
    let output = cargo_bin_cmd!("yams-wiki")
        .args(["write", wiki.path.to_str().unwrap()])
        .write_stdin(serde_json::to_vec(&create_request()).unwrap())
        .output()
        .unwrap();
    let after = Local::now().date_naive().to_string();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    let body = one_json_line(&output.stdout);
    assert_eq!(body["ok"], true);
    assert_eq!(body["slug"], "violet-mode-wakes-the-beacon");
    let page = fs::read_to_string(wiki.path.join("pages/violet-mode-wakes-the-beacon.md")).unwrap();
    assert!(
        page.contains(&format!("updated: {before}\n"))
            || page.contains(&format!("updated: {after}\n")),
        "page did not use the current local calendar date: {page}"
    );
}

#[test]
fn write_wording_only_preserves_old_dates_through_the_cli() {
    let wiki = Wiki::canonical();
    let request = json!({
        "title": "Alpha", "type": "gotcha",
        "fact": "The fictional beacon requires violet mode.",
        "why": "The original evidence supports the same requirement.",
        "how_to_apply": "Enable violet mode.", "falsified_by": "A beacon working in another mode.",
        "summary": "violet mode is required for the fictional beacon", "related": [],
        "update": true, "target": "alpha", "wording_only": true
    });
    let output = cargo_bin_cmd!("yams-wiki")
        .args(["write", wiki.path.to_str().unwrap()])
        .write_stdin(serde_json::to_vec(&request).unwrap())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stderr, b"");
    assert_eq!(one_json_line(&output.stdout)["index_regenerated"], true);
    let stored = fs::read_to_string(wiki.path.join("pages/alpha.md")).unwrap();
    assert!(stored.contains("updated: 2026-08-08\nverified: 2026-08-08\n"));
    assert!(stored.contains("The fictional beacon requires violet mode."));
    assert!(
        wiki.index()
            .contains("violet mode is required for the fictional beacon")
    );
}

#[test]
fn write_preserves_one_structured_json_response_for_refusal_and_operational_results() {
    let wiki = Wiki::canonical();
    let refusal = cargo_bin_cmd!("yams-wiki")
        .args(["write", wiki.path.to_str().unwrap()])
        .write_stdin(b"not json".as_slice())
        .output()
        .unwrap();
    assert_eq!(refusal.status.code(), Some(2));
    assert_eq!(refusal.stderr, b"");
    let refusal_body = one_json_line(&refusal.stdout);
    assert_eq!(refusal_body["ok"], false);
    assert_eq!(refusal_body["exit"], 2);

    let root = tempdir().unwrap();
    let missing = root.path().join("missing-wiki");
    let operational = cargo_bin_cmd!("yams-wiki")
        .args(["write", missing.to_str().unwrap()])
        .write_stdin(serde_json::to_vec(&create_request()).unwrap())
        .output()
        .unwrap();
    assert_eq!(operational.status.code(), Some(4));
    assert_eq!(operational.stderr, b"");
    let operational_body = one_json_line(&operational.stdout);
    assert_eq!(operational_body["ok"], false);
    assert_eq!(operational_body["exit"], 4);
}

#[test]
fn write_stops_reading_at_cap_plus_one_without_waiting_for_eof() {
    let wiki = Wiki::canonical();
    let binary = cargo_bin_cmd!("yams-wiki");
    let mut command = std::process::Command::new(binary.get_program());
    command
        .args(["write", wiki.path.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let (written_tx, written_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer = thread::spawn(move || {
        stdin
            .write_all(&vec![b' '; MAX_FILE_BYTES as usize + 1])
            .unwrap();
        stdin.flush().unwrap();
        written_tx.send(()).unwrap();
        release_rx.recv().unwrap();
    });
    written_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            release_tx.send(()).unwrap();
            let _ = child.kill();
            let _ = child.wait();
            writer.join().unwrap();
            panic!("write waited for EOF after receiving MAX_FILE_BYTES + 1 bytes");
        }
        thread::sleep(Duration::from_millis(10));
    };

    release_tx.send(()).unwrap();
    writer.join().unwrap();
    let mut stdout = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    let mut stderr = Vec::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    assert_eq!(status.code(), Some(2));
    assert_eq!(stderr, b"");
    let body = one_json_line(&stdout);
    assert_eq!(body["ok"], false);
    assert_eq!(body["exit"], 2);
}

#[test]
fn compat_reports_violations_and_a_clean_wiki_passes() {
    let wiki = Wiki::canonical();

    let clean = cargo_bin_cmd!("yams-wiki")
        .args(["compat", wiki.path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        clean.status.success(),
        "{}",
        String::from_utf8_lossy(&clean.stderr)
    );

    fs::write(
        wiki.path.join("pages/alpha.md"),
        PAGE.replace(
            "A fictional beacon needs violet mode.\n",
            "A fictional beacon needs violet mode.\n\nembed ![[beta]]\n",
        ),
    )
    .unwrap();
    let dirty = cargo_bin_cmd!("yams-wiki")
        .args(["compat", wiki.path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!dirty.status.success());
    let stderr = String::from_utf8(dirty.stderr).unwrap();
    assert!(stderr.contains("embed"), "{stderr}");
}

#[test]
fn supersede_command_writes_json_preserves_old_body_and_never_loads_a_store() {
    let wiki = Wiki::canonical();
    fs::write(
        wiki.path.join("pages/beta.md"),
        PAGE.replace("slug: alpha", "slug: beta"),
    )
    .unwrap();
    yams_wiki::reindex_wiki(&wiki.path, &yams_wiki::ReindexOptions::default()).unwrap();
    let state = wiki.path.join("unused-state");
    let output = cargo_bin_cmd!("yams-wiki")
        .args([
            "supersede",
            wiki.path.to_str().unwrap(),
            "alpha",
            "--by",
            "beta",
        ])
        .env("YAMS_HOME", &state)
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert!(output.stderr.is_empty());
    assert_eq!(one_json_line(&output.stdout)["ok"], true);
    let old = fs::read_to_string(wiki.path.join("pages/alpha.md")).unwrap();
    assert!(old.contains("A fictional beacon needs violet mode."));
    assert!(old.contains("**Superseded by:** [[beta]]"));
    assert_eq!(
        yams_wiki::parse_wiki_page(&old).unwrap().status,
        yams_wiki::Status::Historical
    );
    assert!(!state.exists());
}
