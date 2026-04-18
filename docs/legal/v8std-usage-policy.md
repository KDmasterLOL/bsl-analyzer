# v8std.ru Usage Policy

## Purpose

This note defines how `bsl-analyzer` may use materials from
`https://github.com/zeegin/v8std` and `https://v8std.ru` during legal cleanup and
future documentation work.

## Current understanding

Observed public signals:

- `zeegin/v8std` publishes a `LICENSE` file with `CC0 1.0`.
- pages on `v8std.ru` display `No Rights Reserved CREATIVE COMMONS CC0`.

That is useful, but it does not automatically mean every upstream source quoted
or mirrored by `v8std` becomes independently safe for verbatim reuse.

`CC0` can only waive rights actually held by the person applying it. If a page
aggregates or mirrors content that originated elsewhere, reuse still depends on
the rights status of that underlying source.

## Project policy

For `bsl-analyzer`, use `v8std.ru` as follows.

### Allowed by default

- use `v8std.ru` as a public navigation aid to standards and diagnostics;
- cite `v8std.ru` as a secondary source in documentation and provenance notes;
- use `stdNNN` identifiers, rule names, and cross-links found there;
- use `v8std.ru` to discover the original source linked on a page.

### Not allowed by default

- do not assume `CC0` on `v8std.ru` clears third-party text for verbatim reuse;
- do not copy long diagnostic descriptions from `v8std.ru` into project docs;
- do not treat `v8std.ru` as a substitute for official 1C documentation when
  writing parser specs or normative language descriptions;
- do not rely on `v8std.ru` alone when a page explicitly attributes its content
  to another project.

## Source priority

When documenting diagnostics or parser behavior, use this priority:

1. official 1C documentation and standards;
2. `v8std.ru` as a public, convenient secondary source;
3. local tests and independently authored project notes.

When a `v8std.ru` page links to another project as `Источник`, treat that link
as the provenance lead and evaluate the underlying source directly before
reusing wording or examples.

## CC0 practical implications

`CC0` is generally low-friction for reuse because it imposes no attribution or
copyleft conditions under copyright law.

However, it still has practical limits:

- it does not grant patent rights;
- it does not grant trademark rights;
- it does not guarantee the affirmer actually cleared third-party rights;
- it does not remove privacy, publicity, or similar rights of other persons.

For software specifically, `CC0` is usable, but it is not an OSI-approved
software license and may be a poorer fit than `MIT` or `Apache-2.0` for code
distributed as an open source project.

## Working rule for this repository

Use `v8std.ru` freely for links and discovery.

Use official 1C docs or independently written text for normative descriptions.

If a diagnostic or parser note needs wording that is close to the original
standard, paraphrase from the official source instead of copying prose from
`v8std.ru`.
