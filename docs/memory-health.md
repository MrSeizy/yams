# Advisory memory health

`yams-wiki health WIKI_ROOT [--json] [--stale-days N] [--as-of YYYY-MM-DD]`
reports knowledge-maintenance candidates without editing pages, regenerating
the catalog, changing ranking, or loading a model.

```sh
yams-wiki health .agents/memory
yams-wiki health .agents/memory --json --stale-days 90
yams-wiki health .agents/memory --as-of 2026-10-24
```

The default verification interval is 180 days, with an allowed range of 1 to
36500 days. The reference date defaults to the local calendar date. `--as-of`
makes reports reproducible and accepts an exact, valid ASCII calendar date.

Only **current** pages contribute findings. Historical pages preserve old
evidence; in-progress pages are unsettled. Every captured page still undergoes
strict parsing, so malformed or unsafe excluded pages cannot silently disappear.

| Finding | Meaning |
| --- | --- |
| `duplicate_title` | Two or more current titles match after lowercasing and whitespace normalization |
| `duplicate_body` | Two or more nonempty current bodies match after CRLF-to-LF and outer-blank-line normalization; code indentation is preserved |
| `verification_due` | Verification is at least the configured number of calendar days old |
| `future_verification` | The verification date is later than the reference date |
| `uninterpretable_verification` | A legacy accepted date cannot be interpreted as an ASCII calendar date, including impossible dates |

Duplicate groups contain sorted relative paths. Findings sort by kind, then
path list. Similarity is not treated as contradiction, and a newer page is not
automatically considered correct. Age asks for re-verification; it does not
invalidate a claim. Review the evidence before deciding whether to update a
page or use explicit supersession.

## Response and failure behavior

`yams-wiki capabilities --json` advertises `memory_health: 1`.
A successful JSON call emits one line:

```json
{
  "ok": true,
  "health": {
    "as_of": "2026-10-24",
    "stale_days": 180,
    "page_count": 5,
    "current_pages": 4,
    "findings": [],
    "isolated": true,
    "isolation_note": null
  }
}
```

Each finding has `kind`, `paths`, and `message`. Findings are advisory and
**exit 0**, even when present. This command complements `yams-wiki check`,
which retains its existing structural validation and failure contract. Health
does not check catalog drift or the validity of replacement chains; run
`check` for those concerns.

Usage errors exit 2. Once arguments are parsed, malformed or unsafe pages exit
4 with `{"ok":false,"code":"invalid_page","message":"..."}`; snapshot
failures use `capture_failed`. No partial report is returned. Human errors go
to stderr. The reader uses the same safe shared-lock snapshot as graph
expansion, including its explicit unisolated-read diagnostic. It may create
`.write.lock`, but never initializes an absent wiki or changes source bytes.
