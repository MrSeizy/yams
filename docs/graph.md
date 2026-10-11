# Page graph expansion

`yams-wiki expand WIKI_ROOT SLUG [--json]` retrieves a page's immediate
outgoing links and backlinks directly from the wiki. `WIKI_ROOT` is the
directory containing `pages/` and `INDEX.md`, usually `.agents/memory`.
`SLUG` is the exact filename stem, such as `night-mode-rollout`; it is not a
title or a path. Prefix a leading-hyphen slug with the standard `--` argument
separator.

Use expansion after finding a relevant page, when you need the decisions or
gotchas connected to it. It never performs a semantic search, loads a model,
or builds an index. It returns metadata rather than entire page bodies, so
the caller can choose which neighbouring source pages to read and verify.

## JSON contract

`yams-wiki capabilities --json` advertises `contracts.graph_expansion: 1`.
With `--json`, a successful call emits exactly one JSON line:

```json
{
  "ok": true,
  "graph": {
    "page": {
      "slug": "night-mode-rollout",
      "path": "pages/night-mode-rollout.md",
      "title": "Night mode rollout",
      "summary": "Night mode ships behind the beacon flag.",
      "status": "current",
      "verified": "2026-08-01"
    },
    "outgoing": [],
    "backlinks": [],
    "unresolved": ["lantern-console"],
    "isolated": true,
    "isolation_note": null
  }
}
```

`outgoing` and `backlinks` contain the same page metadata shape as `page`.
Paths are relative to `WIKI_ROOT`. Both lists and `unresolved` are sorted by
slug and deduplicated. A page can appear in both directions. Self-links do
not appear in either list. Neighbours' neighbours are not expanded.
`unresolved` contains outgoing target slugs which have no page in the snapshot.

All statuses are included and returned explicitly; expansion does not imply
that a historical or in-progress page is current. Like the wiki checker,
expansion recognises `[[slug]]`, `[[slug|alias]]`, and `[[slug#heading]]`
using lexical matching over the page text. Embeds and block references do
not create edges. This is the existing checker graph, not a full Obsidian
Markdown renderer.

After argument parsing, JSON failures have the shape
`{"ok":false,"code":"page_not_found","message":"..."}`; they contain no
partial graph. Without `--json`, errors go to stderr. Clap argument errors
(such as a missing positional argument) use the ordinary CLI diagnostics.

| Exit | JSON code | Meaning |
| --- | --- | --- |
| 0 | — | Page exists; its graph may have no neighbours |
| 1 | `page_not_found` | No page has the requested slug |
| 2 | `invalid_slug` | Slug is outside the canonical 1–64 byte ASCII format |
| 4 | `invalid_page` | A captured page is malformed, unreadable, or unsafe |
| 4 | `capture_failed` | A consistent snapshot could not be captured |

## Consistency and filesystem safety

Expansion uses the same safe, shared-lock snapshot reader as `yams-wiki
check`. It does not follow symlink pages or recursively scan other folders.
It parses every page against the canonical schema and checks that its slug
matches its filename. An invalid page fails the call, even if it is not a
known neighbour: otherwise its backlinks could silently disappear.

The catalog must be readable and safe to capture, but its contents do not
determine edges. A stale catalog therefore does not prevent expansion and
is not regenerated. Pages and catalog bytes are never changed; acquiring
the shared lock may create `.write.lock`. Missing or unreadable wiki inputs
are rejected before lock acquisition, so an incorrect root does not leave
a lock file behind.

If the shared reader permits an unisolated read of an unwritable corpus,
`isolated` is false and `isolation_note` explains why. Human-readable output
reports that note on stderr. Callers must retain it when interpreting the
result. A missing search database has no effect on expansion.
