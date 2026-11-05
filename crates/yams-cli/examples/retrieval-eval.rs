//! Opt-in, offline end-to-end evaluation with the production model and index.
//! Run with --help; no real memory is read and all mutable state is temporary.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use yams_cli::{InvocationTime, RuntimeInputs, execute_direct_with_embedder};
use yams_core::ExitCode;
use yams_embed::{Embedder, JinaEmbedder};
use yams_wiki::{INDEX_TEMPLATE, IndexPage, parse_wiki_page, rebuild_index};

const FIXTURE: &[u8] = include_bytes!("fixtures/retrieval-v1.json");

#[derive(Parser)]
#[command(about = "Evaluate fictional memory with the pinned local model; emit a JSON report")]
struct Args {
    /// Alternate fictional evaluation fixture; defaults to the frozen v1 corpus.
    #[arg(long)]
    fixture: Option<PathBuf>,
    /// Existing verified model cache (never downloaded or modified by this harness).
    #[arg(long)]
    model_cache: PathBuf,
    /// Machine description saved with the report (for latency comparisons).
    #[arg(long)]
    hardware: String,
    /// Record untimed, ungated score/rank diagnostics after each measured case.
    #[arg(long)]
    diagnostics: bool,
    /// Measured warm queries per case, after one unmeasured warmup.
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..=100))]
    repeats: u32,
    /// Previous report using the same fixture and evaluation protocol.
    #[arg(long)]
    baseline: Option<PathBuf>,
    /// Evaluate a different confidence floor without changing production defaults.
    #[arg(long, allow_hyphen_values = true)]
    min_score: Option<f64>,
    /// Evaluate a different maximum score gap.
    #[arg(long)]
    max_gap: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    description: String,
    pages: Vec<Page>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    slug: String,
    title: String,
    summary: String,
    body: String,
    status: Option<String>,
    replacement: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    slice: String,
    query: String,
    expected: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CaseResult {
    id: String,
    slice: String,
    expected: Vec<String>,
    returned: Vec<String>,
    exit_code: i32,
    hit_at_5: Option<f64>,
    recall_at_5: Option<f64>,
    reciprocal_rank: Option<f64>,
    false_positive: Option<bool>,
    latency_ms: Vec<f64>,
    context_bytes: usize,
    estimated_context_tokens: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diagnostics: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Metrics {
    cases: usize,
    positive_cases: usize,
    negative_cases: usize,
    hit_at_5: Option<f64>,
    recall_at_5: Option<f64>,
    mean_reciprocal_rank: Option<f64>,
    false_positive_rate: Option<f64>,
    latency_p50_ms: f64,
    latency_p95_ms: f64,
    mean_context_bytes: f64,
    mean_estimated_context_tokens: f64,
}

#[derive(Deserialize, Serialize)]
struct Report {
    schema_version: u32,
    fixture_sha256: String,
    provenance: Value,
    protocol: Value,
    configuration: Value,
    model_load_ms: f64,
    index_ms: f64,
    overall: Metrics,
    slices: BTreeMap<String, Metrics>,
    cases: Vec<CaseResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    comparison: Option<Value>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    if args
        .min_score
        .is_some_and(|v| !v.is_finite() || !(-1.0..=1.0).contains(&v))
        || args.max_gap.is_some_and(|v| !v.is_finite() || v < 0.0)
        || args.hardware.trim().is_empty()
    {
        return Err("invalid gate override or empty hardware description".into());
    }
    let fixture_bytes = args
        .fixture
        .as_ref()
        .map(fs::read)
        .transpose()?
        .unwrap_or_else(|| FIXTURE.to_vec());
    let fixture: Fixture = serde_json::from_slice(&fixture_bytes)?;
    validate_fixture(&fixture)?;
    // Read and validate baseline compatibility before constructing the model.
    let baseline: Option<Report> = args
        .baseline
        .as_ref()
        .map(|path| -> Result<_, Box<dyn Error>> { Ok(serde_json::from_slice(&fs::read(path)?)?) })
        .transpose()?;
    let fixture_sha256 = format!("{:x}", Sha256::digest(&fixture_bytes));
    let protocol = json!({
        "k": 5, "repeats": args.repeats, "warmups_per_case": 1,
        "execution": "production direct CLI execution with one warm model; service transport and process startup excluded",
        "context": "name + newline + snippet + newline per hit; Unicode characters / 4 estimates tokens, not a tokenizer",
        "percentile": "nearest rank", "negatives": "included; any returned hit is a false positive"
    });
    if let Some(previous) = &baseline
        && (previous.schema_version != 1
            || previous.fixture_sha256 != fixture_sha256
            || previous.protocol != protocol)
    {
        return Err("baseline fixture or evaluation protocol differs".into());
    }
    let temp = tempfile::tempdir()?;
    let project = temp.path().join("fictional-project");
    let wiki = project.join(".agents/memory");
    fs::create_dir_all(wiki.join("pages"))?;
    let mut index_pages = Vec::new();
    for page in &fixture.pages {
        let source = page_source(page);
        let parsed = parse_wiki_page(&source)?;
        fs::write(
            wiki.join("pages").join(format!("{}.md", parsed.slug)),
            source,
        )?;
        index_pages.push(IndexPage {
            slug: parsed.slug,
            page_type: parsed.page_type,
            summary: parsed.summary,
        });
    }
    fs::write(
        wiki.join("INDEX.md"),
        rebuild_index(INDEX_TEMPLATE, &index_pages)?,
    )?;
    let mut inputs = RuntimeInputs::current()?;
    inputs.cwd = project.canonicalize()?;
    let variables = vec![
        (
            OsString::from("YAMS_HOME"),
            temp.path().join("state").into_os_string(),
        ),
        (OsString::from("YAMS_DIRS"), wiki.into_os_string()),
        (OsString::from("YAMS_NO_SERVICE"), OsString::from("1")),
    ];
    let started = Instant::now();
    let mut model = JinaEmbedder::offline(&args.model_cache, temp.path().join("model-locks"))?;
    let model_load_ms = started.elapsed().as_secs_f64() * 1000.0;
    let when = InvocationTime::capture();
    let started = Instant::now();
    let indexed = execute_direct_with_embedder(
        ["yams", "--index", "--json"],
        variables.clone(),
        &inputs,
        &[],
        &when,
        &mut model,
    );
    if indexed.exit_code != ExitCode::Ok {
        return Err(format!(
            "fixture indexing failed: {}{}",
            indexed.stdout, indexed.stderr
        )
        .into());
    }
    let index_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut results = Vec::new();
    for case in &fixture.cases {
        let mut argv = vec!["yams".to_owned(), "--json".into(), "-k".into(), "5".into()];
        for (flag, value) in [("--min-score", args.min_score), ("--max-gap", args.max_gap)] {
            if let Some(value) = value {
                argv.extend([flag.into(), value.to_string()]);
            }
        }
        argv.extend(["--".into(), case.query.clone()]);
        let mut observed = None;
        let mut latencies = Vec::new();
        for repeat in 0..=args.repeats {
            let start = Instant::now();
            let result = execute_direct_with_embedder(
                argv.clone(),
                variables.clone(),
                &inputs,
                &[],
                &when,
                &mut model,
            );
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            if !matches!(
                result.exit_code,
                ExitCode::Ok | ExitCode::Empty | ExitCode::Unsure
            ) {
                return Err(format!(
                    "case {} failed: {}{}",
                    case.id, result.stdout, result.stderr
                )
                .into());
            }
            let hits: Vec<Value> = serde_json::from_str(&result.stdout)?;
            let mut returned = Vec::new();
            let mut context = String::new();
            for hit in hits {
                let path = hit["path"].as_str().ok_or("hit has no path")?;
                let slug = Path::new(path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .ok_or("invalid hit path")?;
                returned.push(slug.to_owned());
                context.push_str(hit["name"].as_str().ok_or("hit has no name")?);
                context.push('\n');
                context.push_str(hit["snippet"].as_str().ok_or("hit has no snippet")?);
                context.push('\n');
            }
            let current = (returned, context, i32::from(result.exit_code));
            if let Some(previous) = &observed
                && previous != &current
            {
                return Err(format!("case {} changed across repeated queries", case.id).into());
            }
            observed = Some(current);
            if repeat > 0 {
                latencies.push(elapsed);
            }
        }
        let (returned, context, exit_code) = observed.expect("at least one repetition");
        let mut result = score(case, returned);
        result.exit_code = exit_code;
        result.latency_ms = latencies;
        result.context_bytes = context.len();
        result.estimated_context_tokens = context.chars().count() as f64 / 4.0;
        if args.diagnostics {
            let mut diagnostic_argv = argv.clone();
            diagnostic_argv.splice(1..1, ["--explain".into(), "--no-gate".into()]);
            let diagnostic = execute_direct_with_embedder(
                diagnostic_argv,
                variables.clone(),
                &inputs,
                &[],
                &when,
                &mut model,
            );
            if !matches!(
                diagnostic.exit_code,
                ExitCode::Ok | ExitCode::Empty | ExitCode::Unsure
            ) {
                return Err(format!(
                    "diagnostics failed for {}: {}{}",
                    case.id, diagnostic.stdout, diagnostic.stderr
                )
                .into());
            }
            let diagnostic: Value = serde_json::from_str(&diagnostic.stdout)?;
            let hits: Vec<_> = diagnostic["hits"]
                .as_array()
                .ok_or("diagnostics missing hits")?
                .iter()
                .map(|hit| {
                    let path = hit["path"].as_str().ok_or("diagnostic hit missing path")?;
                    let slug = Path::new(path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .ok_or("invalid diagnostic path")?;
                    Ok(json!({"slug": slug, "score": hit["score"],
                        "exact": hit["exact"], "explain": hit["explain"]}))
                })
                .collect::<Result<_, Box<dyn Error>>>()?;
            result.diagnostics = Some(json!({
                "baseline": diagnostic["gate"]["baseline"],
                "floor_fired": diagnostic["gate"]["floor_fired"],
                "identifier_missing": diagnostic["gate"]["identifier_dropped"]
                    .as_array().is_some_and(|entries| !entries.is_empty()),
                "hits": hits
            }));
        }
        results.push(result);
    }
    let slices = results
        .iter()
        .map(|r| r.slice.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|slice| {
            let metrics = aggregate(results.iter().filter(|r| r.slice == slice));
            (slice, metrics)
        })
        .collect();
    let executable_sha256 = format!("{:x}", Sha256::digest(fs::read(std::env::current_exe()?)?));
    let mut report = Report {
        schema_version: 1,
        fixture_sha256,
        provenance: json!({"version": env!("CARGO_PKG_VERSION"), "executable_sha256": executable_sha256,
            "hardware": args.hardware,
            "cargo_lock_sha256": format!("{:x}", Sha256::digest(include_bytes!("../../../Cargo.lock"))),
            "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
            "logical_cpus": std::thread::available_parallelism().map(usize::from).ok(),
            "debug_assertions": cfg!(debug_assertions), "timestamp": when.utc_timestamp,
            "model_signature": model.signature(), "fixture_description": fixture.description}),
        protocol,
        configuration: json!({"min_score": args.min_score.unwrap_or(yams_core::MIN_SCORE),
            "max_gap": args.max_gap.unwrap_or(yams_core::MAX_GAP), "lexical_weight": yams_core::LEXICAL_WEIGHT,
            "rrf_k": yams_core::RRF_K, "candidates": yams_core::CANDIDATES}),
        model_load_ms,
        index_ms,
        overall: aggregate(&results),
        slices,
        cases: results,
        comparison: None,
    };
    if let Some(previous) = baseline {
        report.comparison = Some(compare(&previous, &report)?);
    }
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn page_source(page: &Page) -> String {
    let mut source = format!(
        "---\nslug: {}\ntitle: {}\ntype: decision\nstatus: {}\nowner: shared\nupdated: 2026-08-01\nverified: 2026-08-01\nsummary: {}\n---\n\n{}\n",
        page.slug,
        page.title,
        page.status.as_deref().unwrap_or("current"),
        page.summary,
        page.body
    );
    if let Some(replacement) = &page.replacement {
        source.push_str(&format!("\n**Superseded by:** [[{replacement}]]\n"));
    }
    source
}

fn validate_fixture(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let mut slugs = BTreeSet::new();
    for page in &fixture.pages {
        parse_wiki_page(&page_source(page))?;
        if !slugs.insert(&page.slug) {
            return Err("duplicate fixture page".into());
        }
    }
    for page in &fixture.pages {
        if let Some(replacement) = &page.replacement
            && !slugs.contains(replacement)
        {
            return Err("missing fixture replacement".into());
        }
    }
    let mut ids = BTreeSet::new();
    for case in &fixture.cases {
        if !ids.insert(&case.id) || case.query.trim().is_empty() {
            return Err("invalid fixture case".into());
        }
        if case.expected.iter().collect::<BTreeSet<_>>().len() != case.expected.len()
            || case.expected.iter().any(|slug| !slugs.contains(slug))
        {
            return Err("invalid expected fixture pages".into());
        }
    }
    if slugs.is_empty() || ids.is_empty() {
        return Err("empty fixture".into());
    }
    Ok(())
}

fn score(case: &Case, returned: Vec<String>) -> CaseResult {
    let found = case
        .expected
        .iter()
        .filter(|slug| returned.iter().take(5).any(|hit| hit == *slug))
        .count();
    let positive = !case.expected.is_empty();
    let rank = returned
        .iter()
        .take(5)
        .position(|slug| case.expected.contains(slug));
    CaseResult {
        id: case.id.clone(),
        slice: case.slice.clone(),
        expected: case.expected.clone(),
        hit_at_5: positive.then_some(f64::from(found > 0)),
        recall_at_5: positive.then(|| found as f64 / case.expected.len() as f64),
        reciprocal_rank: positive.then(|| rank.map_or(0.0, |i| 1.0 / (i + 1) as f64)),
        false_positive: (!positive).then_some(!returned.is_empty()),
        returned,
        exit_code: 0,
        latency_ms: Vec::new(),
        context_bytes: 0,
        estimated_context_tokens: 0.0,
        diagnostics: None,
    }
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let values: Vec<_> = values.collect();
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn percentile(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[((sorted.len() as f64 * percentile).ceil() as usize).saturating_sub(1)]
}

fn aggregate<'a>(results: impl IntoIterator<Item = &'a CaseResult>) -> Metrics {
    let results: Vec<_> = results.into_iter().collect();
    let latency: Vec<_> = results
        .iter()
        .flat_map(|r| r.latency_ms.iter().copied())
        .collect();
    Metrics {
        cases: results.len(),
        positive_cases: results.iter().filter(|r| r.hit_at_5.is_some()).count(),
        negative_cases: results
            .iter()
            .filter(|r| r.false_positive.is_some())
            .count(),
        hit_at_5: mean(results.iter().filter_map(|r| r.hit_at_5)),
        recall_at_5: mean(results.iter().filter_map(|r| r.recall_at_5)),
        mean_reciprocal_rank: mean(results.iter().filter_map(|r| r.reciprocal_rank)),
        false_positive_rate: mean(
            results
                .iter()
                .filter_map(|r| r.false_positive.map(f64::from)),
        ),
        latency_p50_ms: percentile(&latency, 0.5),
        latency_p95_ms: percentile(&latency, 0.95),
        mean_context_bytes: mean(results.iter().map(|r| r.context_bytes as f64)).unwrap_or(0.0),
        mean_estimated_context_tokens: mean(results.iter().map(|r| r.estimated_context_tokens))
            .unwrap_or(0.0),
    }
}

fn compare(previous: &Report, current: &Report) -> Result<Value, Box<dyn Error>> {
    let mut changed = Vec::new();
    if previous.cases.len() != current.cases.len() {
        return Err("baseline case count differs".into());
    }
    for (old, new) in previous.cases.iter().zip(&current.cases) {
        if old.id != new.id || old.expected != new.expected {
            return Err("baseline cases differ".into());
        }
        if old.returned != new.returned {
            changed.push(json!({"id": new.id, "before": old.returned, "after": new.returned}));
        }
    }
    let delta = |before: Option<f64>, after: Option<f64>| before.zip(after).map(|(a, b)| b - a);
    // Recompute quality from discrete labels/results, avoiding round-trip
    // drift in serialized aggregate fractions such as 27/28.
    let quality = |report: &Report| {
        let results: Vec<_> = report
            .cases
            .iter()
            .map(|result| {
                score(
                    &Case {
                        id: result.id.clone(),
                        slice: result.slice.clone(),
                        query: String::new(),
                        expected: result.expected.clone(),
                    },
                    result.returned.clone(),
                )
            })
            .collect();
        aggregate(&results)
    };
    let before_quality = quality(previous);
    let after_quality = quality(current);
    // Per-case estimates are exact quarter-token increments. Subtract those
    // before averaging so JSON round trips of the aggregate mean cannot turn
    // identical context into a spurious nonzero delta.
    let context_delta = mean(
        previous
            .cases
            .iter()
            .zip(&current.cases)
            .map(|(old, new)| new.estimated_context_tokens - old.estimated_context_tokens),
    )
    .unwrap_or(0.0);
    Ok(
        json!({"baseline_provenance": previous.provenance, "baseline_configuration": previous.configuration,
        "delta": {"hit_at_5": delta(before_quality.hit_at_5, after_quality.hit_at_5),
            "recall_at_5": delta(before_quality.recall_at_5, after_quality.recall_at_5),
            "false_positive_rate": delta(before_quality.false_positive_rate, after_quality.false_positive_rate),
            "latency_p50_ms": current.overall.latency_p50_ms - previous.overall.latency_p50_ms,
            "latency_p95_ms": current.overall.latency_p95_ms - previous.overall.latency_p95_ms,
            "mean_estimated_context_tokens": context_delta},
        "changed_cases": changed}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_fixture_is_valid_and_exercises_every_slice() {
        let fixture: Fixture = serde_json::from_slice(FIXTURE).unwrap();
        validate_fixture(&fixture).unwrap();
        assert_eq!(
            fixture
                .cases
                .iter()
                .map(|c| c.slice.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "exact",
                "multi_page",
                "negative",
                "paraphrase",
                "supersession"
            ])
        );
    }

    #[test]
    fn validation_fixture_preserves_v1_and_adds_balanced_regressions() {
        let bytes = include_bytes!("fixtures/retrieval-v2.json");
        let fixture: Fixture = serde_json::from_slice(bytes).unwrap();
        validate_fixture(&fixture).unwrap();
        let original: Value = serde_json::from_slice(FIXTURE).unwrap();
        let expanded: Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(original["pages"], expanded["pages"]);
        assert_eq!(
            original["cases"].as_array().unwrap(),
            &expanded["cases"].as_array().unwrap()[..17]
        );
        assert_eq!(
            fixture
                .cases
                .iter()
                .filter(|case| !case.expected.is_empty())
                .count(),
            28
        );
        assert_eq!(
            fixture
                .cases
                .iter()
                .filter(|case| case.expected.is_empty())
                .count(),
            19
        );
    }

    #[test]
    fn alias_fixture_requires_retrieval_without_literal_query_presence() {
        let fixture: Fixture =
            serde_json::from_slice(include_bytes!("fixtures/retrieval-aliases-v1.json")).unwrap();
        validate_fixture(&fixture).unwrap();
        for case in &fixture.cases {
            assert_eq!(case.expected.len(), 1);
            let page = fixture
                .pages
                .iter()
                .find(|p| p.slug == case.expected[0])
                .unwrap();
            let source = page_source(page).to_lowercase();
            assert_eq!(
                source.contains(&case.query.to_lowercase()),
                case.slice == "alias_control"
            );
        }
    }

    #[test]
    fn scoring_separates_negative_queries_from_positive_recall() {
        let positive = Case {
            id: "multi".into(),
            slice: "multi".into(),
            query: "fictional".into(),
            expected: vec!["alpha".into(), "beta".into()],
        };
        let result = score(
            &positive,
            vec!["noise".into(), "alpha".into(), "alpha".into()],
        );
        assert_eq!(
            (result.hit_at_5, result.recall_at_5, result.reciprocal_rank),
            (Some(1.0), Some(0.5), Some(0.5))
        );
        let negative = Case {
            expected: vec![],
            ..positive
        };
        let false_positive = score(&negative, vec!["noise".into()]);
        let abstention = score(&negative, vec![]);
        let metrics = aggregate([&result, &false_positive, &abstention]);
        assert_eq!(metrics.recall_at_5, Some(0.5));
        assert_eq!(metrics.false_positive_rate, Some(0.5));
        assert_eq!(metrics.positive_cases, 1);
        assert_eq!(metrics.negative_cases, 2);
    }

    #[test]
    fn nearest_rank_percentiles_keep_tail_latency() {
        assert_eq!(percentile(&[4.0, 1.0, 3.0, 2.0], 0.5), 2.0);
        assert_eq!(percentile(&[4.0, 1.0, 3.0, 2.0], 0.95), 4.0);
    }

    #[test]
    fn identical_quality_and_context_have_zero_delta_after_serializing_the_baseline() {
        let case = Case {
            id: "one".into(),
            slice: "exact".into(),
            query: "fictional".into(),
            expected: vec!["alpha".into()],
        };
        let mut results: Vec<_> = (0..28)
            .map(|i| {
                let mut result = score(&case, if i == 0 { vec![] } else { vec!["alpha".into()] });
                result.id = i.to_string();
                result.estimated_context_tokens = (i * 13) as f64 / 4.0;
                result
            })
            .collect();
        results[0].estimated_context_tokens = 6.25;
        let report = Report {
            schema_version: 1,
            fixture_sha256: "fictional".into(),
            provenance: json!({}),
            protocol: json!({}),
            configuration: json!({}),
            model_load_ms: 0.0,
            index_ms: 0.0,
            overall: aggregate(&results),
            slices: BTreeMap::new(),
            cases: results,
            comparison: None,
        };
        let previous: Report =
            serde_json::from_slice(&serde_json::to_vec(&report).unwrap()).unwrap();
        let comparison = compare(&previous, &report).unwrap();
        assert_eq!(comparison["delta"]["mean_estimated_context_tokens"], 0.0);
        assert_eq!(comparison["delta"]["hit_at_5"], 0.0);
        assert_eq!(comparison["delta"]["recall_at_5"], 0.0);
        assert_eq!(comparison["changed_cases"], json!([]));
    }
}
