# Superseding outdated knowledge

Create and verify the replacement page before retiring the old fact:

```sh
yams-wiki supersede .agents/memory old-fact --by corrected-fact
yams --index
```

`supersede` requires a valid current replacement in the same wiki. It preserves
all old body bytes, changes `status` to `historical`, updates the edit and
verification dates, and appends this standalone paragraph:

```markdown
**Superseded by:** [[corrected-fact]]
```

The eight frontmatter fields and repository layout version stay unchanged. This
body declaration is explicit: ordinary related links or historical status alone
do not redirect retrieval. Fenced code, indented code, inline examples and HTML comments are
not declarations. The parser recognizes physical declaration lines, rather than
the entire Markdown grammar; keep active declarations in their own paragraphs.
HTML comment blocks begin at the start of a line; inline comments must close on
the same line. A malformed or duplicate declaration is refused. Close any
unfinished code fence before running `supersede`.

The command returns the same single-line JSON success/error envelope and exit
codes as `write` (0 success, 2 refusal, 4 operational failure). It uses the same
exclusive lock, pinned paths, revision checks, durable page replacement, and
catalog regeneration. A failure after a page becomes visible reports the paths
that may have changed, just like an ordinary write. It never loads a model,
updates the search store, or commits Git changes.

Automation can send the equivalent request to `yams-wiki write WIKI_ROOT`:

```json
{"supersede":true,"target":"old-fact","replacement":"corrected-fact"}
```

Both interfaces accept an optional `expected_sha256` (CLI
`--expected-sha256 HASH`) containing the digest of the exact old page bytes. The
writer refuses missing pages, unsafe paths, self-replacements, non-current
replacements and a different replacement already declared on the old page.
Repeating the same operation on the same date leaves page bytes unchanged.
Ordinary full-body updates retain the stored declaration and cannot retarget it.
To supersede a replacement later, create another current page and supersede that
replacement in turn.

Existing repositories with the original v1 `SCHEMA.md` remain recognizable to
`init inspect`. An approved full initialization manifest can update only that
exact released schema to the new guidance; customized schema files remain
protected. Search and supersession do not require rewriting existing schema
documentation.

## Retrieval

After indexing, selection and the relevance gate run on the original matches.
For every selected historical page with a declaration, search follows the chain
to its terminal current page in the same directory and corpus. That page takes
the historical match's result slot, even with `-k 1` or when the replacement was
outside the hybrid candidate window. Shared replacements are deduplicated. The
original historical hits follow primary results when the result limit permits.

The promoted page keeps its own cosine score, body and snippet; it does not
inherit an exact-identifier match from the old page. A replacement reached only
through the relationship may have a score below the relevance threshold. That
is explained by additive provenance fields in the existing search JSON contract:

- `supersedes`: paths of selected historical matches that led to this result.
- `superseded_by`: the immediate replacement path on a historical result.

Human output labels these relationships too. `--explain` still describes the
original gate decision; a promoted page carries its own available rank signals.
Direct queries, service queries and `--all` use the same composition path.

A missing, cyclic or non-current terminal replacement is never guessed. Search
keeps the historical match with its `superseded_by` annotation; run
`yams-wiki check` to diagnose and repair the chain. Relations never cross corpus
or directory boundaries. A query whose original matches fail the gate does not
promote anything.

## Index compatibility

`yams-wiki capabilities --json` advertises `contracts.supersession: 1`. Existing
`search_results: 1` fields and ordinary result behavior remain compatible; the
provenance fields are optional additions.

Project store schema v3 records relations from complete source pages, separately
from chunks, so chunk boundaries cannot create or erase declarations. Exact v1
and v2 stores migrate in the existing transaction. The next index pass backfills
relations from unchanged sources; a compatible v2 store reuses its embeddings.
In shared memory, malformed declarations abort indexing without publishing
partial changes. Older pages may have free-form forward links written under the
original guidance. If a line starts `**Superseded by:**` but is not in the new
canonical form, edit it into a valid declaration with `status: historical`, or
rename that prose label to an ordinary related link. Verify the replacement,
then run `yams-wiki check`, `yams-wiki catalog` and `yams --index`.

Private and override corpora may contain arbitrary Markdown. There, only a
valid declaration on a historical page opts into supersession; malformed,
non-historical and self-referential declarations are treated as ordinary text
and cannot block indexing the shared corpus.
Older binaries cannot open a v3 project store. The source Markdown remains the
portable record and can rebuild an index.
