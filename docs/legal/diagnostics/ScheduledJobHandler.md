# ScheduledJobHandler provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows from public platform behavior and public 1C guidance around scheduled jobs: a scheduled job should point to a valid server-side handler that can actually be executed. This is a metadata-integrity and runtime-correctness concern, not a unique analyzer-specific idea.

## Public sources

- `#std540` "Общие требования к регламентным заданиям"
- 1C developer guide section about scheduled jobs

## Audit result

The current implementation is local Rust code that:

- only runs for `SessionModule`;
- loads configuration metadata;
- validates handler presence and format;
- resolves the referenced common module;
- checks server-side availability;
- resolves the method in the symbol tree;
- verifies `Экспорт`, empty body, and the special predefined-job restriction on parameters;
- reports duplicate handler usage across scheduled jobs.

## Important caveats

- All diagnostics are attached to the beginning of `SessionModule`, because the rule is metadata-driven and not tied to a single code location.
- The implementation does not validate every possible semantic detail of scheduled job parameter compatibility; it only applies the checks that are currently encoded in this project.
- Duplicate-handler detection is a local project policy choice layered on top of basic runtime-correctness checks.

## Conclusion

`ScheduledJobHandler` looks like a strong permissive candidate. The rule is grounded in public platform behavior, and the current implementation is local metadata- and symbol-tree-based validation logic.
