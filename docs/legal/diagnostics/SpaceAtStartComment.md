# SpaceAtStartComment provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule is a straightforward formatting requirement: regular line comments should have a space after `//`. This follows directly from public module text style guidance and is not a unique analyzer-specific idea.

## Public sources

- `#std456` "Тексты модулей"
- `v8std.ru/diagnostics/bslls/SpaceAtStartComment/` as a secondary public reference

## Audit result

The current implementation is local Rust code. It scans lexer comment tokens and reports comments that do not match the local "good comment" pattern.

The implementation also contains local project-specific exceptions:

- separator lines made only of `/` plus spaces or tabs
- supported annotation prefixes from configuration defaults such as `//@`, `//(c)`, and `//©`
- empty comments containing only `//`

The diagnostic also offers a local quick-fix that inserts a space after the leading slashes.

## Important caveats

- Current behavior is stricter than a generic prose description because `use_strict = true` treats `/// text` and `//// text` as violations unless they match an allowed annotation pattern or separator form.
- The TODO about skipping commented-out code is implementation detail, not a provenance concern.

## Conclusion

`SpaceAtStartComment` looks like a strong permissive candidate. The rule is standards-based, and the current implementation is local token-based logic with local configuration-driven exceptions.
