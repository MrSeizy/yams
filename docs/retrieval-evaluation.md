# Retrieval evaluation

The opt-in `retrieval-eval` example runs the production indexing and direct
search paths with the pinned Jina model against a frozen fictional corpus.
It creates its project, index, vector store, query log, and locks in a temporary
directory and removes them when it exits. It never reads repository memories,
uses the service, or downloads a model. Supply an existing verified model cache.
The example is a development tool and is not shipped in release tarballs.

```sh
cargo run -p yams-cli --release --locked --example retrieval-eval -- \
  --model-cache "$HOME/Library/Caches/yams/rust-v1/models" \
  --hardware 'describe your machine here' > /tmp/yams-baseline.json

# Compare an implementation or gate configuration against that report:
cargo run -p yams-cli --release --locked --example retrieval-eval -- \
  --model-cache "$HOME/Library/Caches/yams/rust-v1/models" \
  --hardware 'describe your machine here' \
  --baseline /tmp/yams-baseline.json > /tmp/yams-candidate.json
```

Use `--min-score` and `--max-gap` to evaluate alternate gate settings. These
are invocation overrides, not changes to production defaults. An unreadable
cache, invalid fixture, operational search failure, or nondeterministic result
across repetitions fails the run. Retrieval misses and false positives remain
visible in the report; they do not cause the harness to discard evidence or
exit unsuccessfully.

Use `--fixture crates/yams-cli/examples/fixtures/retrieval-v2.json` for the
expanded regression corpus. `--diagnostics` adds an untimed, ungated search
after each case and records its cosine baseline, floor/identifier decisions,
and per-hit ranks. Diagnostic results use fixture slugs, not temporary paths.
Compare reports from the same fixture; v1 remains frozen.

Use `--fixture crates/yams-cli/examples/fixtures/retrieval-aliases-v1.json`
for eight positive alias/control cases: HTTP2/HTTP/2, OAuth2/OAuth 2.0,
PostgreSQL/Postgres, and IPv6/IP version 6. These four fictional pages test
whether valid alternate spellings survive changes to literal matching.

## What is measured

The fixture contains ten pages and seventeen questions: paraphrases, exact
symbols, explicit supersession, a two-page answer, and four questions with no
answer in the corpus. Its source is
[`retrieval-v1.json`](../crates/yams-cli/examples/fixtures/retrieval-v1.json).
All pages and questions are invented. Keep expected results independent of the
observed output; adding difficulty is useful, changing labels to hide failures
is not.

The v2 fixture retains every v1 page and question and adds fifteen positive
paraphrases/case variants and fifteen hard negatives. Its 47 cases include 28
answerable questions and 19 unanswerable ones. Missing attributes of familiar
topics are deliberately labeled unanswerable even when a related page exists.

The report includes overall and per-slice metrics:

- **hit@5:** fraction of answerable questions with any expected page in the
  top five.
- **recall@5:** mean fraction of each answerable question's expected pages
  returned in the top five. Duplicate results cannot inflate recall.
- **mean reciprocal rank:** the reciprocal rank of the first expected page
  within the top five, or zero for a miss, averaged over answerable questions.
- **false-positive rate:** fraction of unanswerable questions that return any
  hit. Negative questions are excluded from recall and included here.
- **p50/p95 latency:** nearest-rank percentiles of measured direct searches,
  including query embedding, store reads, ranking, rendering, and query logging.
- **context size:** bytes and estimated tokens for each result's name and
  snippet, each followed by a newline. Tokens are Unicode characters divided
  by four, an explicit estimate. Full chunks, paths, JSON envelopes, and tool
  instructions are outside this context measure.

There is one unmeasured warmup per question, then three measured repetitions
(`--repeats` changes this). One model remains loaded across the run. Model load
and index build times are separate fields. These numbers describe warm direct
execution, not process startup or service transport latency.

Reports record the fixture hash, executable hash, compiled lockfile hash,
model signature, package version, hardware description, platform, timestamp,
and ranking constants. A comparison requires matching report version, fixture,
and evaluation protocol. It reports changed result lists and aggregate deltas;
check hardware and build mode before interpreting latency changes. A comparison
against the same implementation should have zero quality and context deltas;
wall-clock latency will vary.

## Using the evidence

The initial run recovered all expected pages but returned hits for the unknown
symbol and the question about a missing project fact: two of four negative
queries. Those queries remain in the fixture. This small corpus is a regression
probe, not evidence of general retrieval accuracy or calibrated confidence.

The expanded baseline exposed an important limit: unsupported questions about
known topics can score above 0.85, while valid paraphrases can score below 0.75.
Increasing the global score floor cannot separate these cases without losing
useful results. Case or digits alone do not distinguish code symbols from
product and protocol names. A broader literal gate rejected HTTP2, OAuth2,
PostgreSQL, and IPv6 when pages used their common alternate spellings.

The identifier gate therefore applies only to a single ASCII token containing
an underscore and a letter, without a leading digit. Punctuation wrappers are
allowed. The token must occur in indexed chunk text or a page title at ASCII
identifier boundaries, ignoring case. Names without underscores, ordinary
words, numbers, multi-token queries, and non-ASCII queries retain the existing
semantic gate. The stricter case-sensitive, unique-page exact rescue is
unchanged. Presence alone does not bypass the score gates.

On the pinned Jina model, the narrower gate rejected all five absent
underscored-symbol cases in v2. False positives fell from 17/19 to 12/19;
positive recall stayed at 27/28 and every positive result list stayed
identical. All eight alias/control cases were retrieved before and after the
change. The unanswerable `FixedCalendar` case still returns related pages:
bare mixed-case names are intentionally left to semantic retrieval. These are
small synthetic regression measurements, not general accuracy rates.

Rejected single-project identifier queries return exit 3. `--no-gate` still
shows candidates; `--explain` adds `gate.identifier_dropped` (an empty array
when this gate does not reject). Empty indexes retain exit 1. Supersession
follows selection as before, so a documented retired symbol can still lead
to its replacement.

For prose, retrieval remains a source of leads. An answering agent must verify
that a retrieved passage actually supplies the requested fact and abstain when
it does not. A future answer-support verifier would need separate evaluation
on these missing-attribute cases and additional unseen fixtures, with recall,
latency, and model cost measured alongside false positives.

Before changing the model, chunking, fusion, gate, or supersession selection,
capture a baseline, run the candidate against the same fixture, inspect each
changed case, and weigh recall against false positives, latency, and context
size. Add fictional regression cases when a new failure is understood.

The harness's scoring and fixture tests run without a model:

```sh
cargo test -p yams-cli --example retrieval-eval --locked
```

This evaluation format was informed by
[ai-memory's retrieval A/B reports](https://github.com/akitaonrails/ai-memory/blob/1bd866375b01031d63db54862a133376598d3701/docs/benchmarks/retrieval-ab-r2.md).
The fixture and implementation here are specific to Yams.
