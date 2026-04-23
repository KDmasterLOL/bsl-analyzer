# UsingModalWindows provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows directly from public 1C guidance for web-client compatible configurations: modal windows and dialogs should not be used, and non-modal alternatives should be preferred. This is a public platform and UX restriction, not a unique analyzer-specific idea.

## Public sources

- `#std703` "Ограничение на использование модальных окон и синхронных вызовов"
- public methodological material about refusal to use modal windows
- `v8std.ru/diagnostics/bslls/UsingModalWindows/` as a secondary public reference

## Audit result

The current implementation is local Rust code built on top of HIR diagnostics. It reports a fixed set of known global-context modal methods and includes a replacement name in the final message.

## Important caveats

- The current implementation explicitly covers only global-context modal methods, not every possible modal pattern in the platform.
- The mapping from modal method to recommended replacement is a local implementation detail of this project.

## Conclusion

`UsingModalWindows` looks like a strong permissive candidate. The rule is standards-based, and the current implementation path is local and HIR-based.
