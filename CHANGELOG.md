# Changelog

User-facing changes to Yams, newest first. Each released version links
to its full release notes under `docs/release/notes/`, which carry the
provenance record: the release commit, the validated toolchain, the
`Cargo.lock` checksum, and the release gate that passed.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Concrete memory consultation triggers for schemas and migrations, public
  interfaces, architecture and dependency choices, build/release workflows,
  recurring failures and prior decisions. Mechanical edits can skip retrieval,
  and relevant verified memory can be reused within the same decision.
- Initialization recognizes the original Project memory policy and proposes its
  replacement through the existing reviewed manifest, preserving unrelated
  instructions. Customized policies and duplicate headings remain protected.
- `init_manifest: 4`: omit the project-page input on an existing valid corpus
  to preserve its exact contents during policy/schema upgrades or layout
  migration. Creating memory still requires a project page; supplying one
  deliberately replaces it.
- `yams-wiki supersede WIKI_ROOT SLUG --by REPLACEMENT` preserves old evidence,
  marks it historical, and records an explicit replacement without changing the
  eight-field frontmatter format. The JSON writer supports the same operation.
- Retrieval follows historical replacement chains, promotes the current page
  even at `-k 1`, and reports `supersedes` / `superseded_by` provenance. Existing
  similarity scores and relevance gates remain tied to their original evidence.
- Wiki checks diagnose invalid replacement declarations, missing targets and
  cycles. Capability `supersession: 1` advertises the new behavior.
- `yams-wiki expand WIKI_ROOT SLUG [--json]`: retrieve a page's immediate
  outgoing links, backlinks, and unresolved targets without a model or
  search index. Results carry page summaries, paths, lifecycle status,
  and verification dates; unsafe or malformed pages fail explicitly.
- Capability contract `graph_expansion: 1` for the graph response and
  classified failure envelope.

### Changed

- Project store schema v3 adds replacement metadata. Exact v1/v2 stores migrate
  transactionally; the next index pass backfills links without re-embedding
  unchanged pages from a compatible v2 store.
- Original v1 schema documentation remains recognizable; updating it uses the
  existing inspect/plan/apply manifest flow. Shared pages with a noncanonical
  `**Superseded by:**` line need that line repaired before indexing; private and
  override Markdown only opts in through a valid historical declaration.

## [0.1.0] — 2026-08-17

First public release ([full release notes](docs/release/notes/v0.1.0.md)).

### Added

- `yams`: offline semantic search over a repository's `.agents/memory`
  pages, matching on exact words and meaning, with a confidence gate
  ("no confident match" instead of noise), JSON output, and index
  maintenance (`--index`, `--projects`, `--stats`, `--gc`, `--all`).
- `yams-wiki`: repository-memory initialization in explicit
  inspect → plan → approve → apply steps, page validation (`check`,
  `compat`), catalog maintenance (`catalog`), transactional page
  writes (`write`), and machine-readable interface versions
  (`capabilities`).
- `yams-service`: optional background service that keeps the search
  model warm between queries (`brew services start yams`).
- `memory-search`: compatibility launcher for `yams`.
- Supply-chain-pinned embedding model: downloads come from one fixed,
  known revision and are checksum-verified before use, online or
  offline.
- Homebrew distribution (`brew install ponchopig/yams/yams`) for
  macOS 14+ on Apple Silicon, with a tarball, `SHA256SUMS`, and a
  CycloneDX SBOM attached to the release.

[Unreleased]: https://github.com/PonchoPig/yams/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/PonchoPig/yams/releases/tag/v0.1.0
