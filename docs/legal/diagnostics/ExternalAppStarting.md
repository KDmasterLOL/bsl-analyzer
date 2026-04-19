# ExternalAppStarting provenance

## Assessment

`ExternalAppStarting` is a good candidate for `MIT OR Apache-2.0`.

The security concern behind the rule is public and standard-driven: launching external applications or OS commands from 1C code must be controlled carefully. This is supported by `#std774` and by the broader external-code restrictions in `#std669`.

At the same time, the current diagnostic behavior is a local implementation choice. It uses a project-defined method list and flags a conservative subset of launch-related APIs.

## Source basis

- 1C standard on application launch security: <https://its.1c.ru/db/v8std/content/774/hdoc>
- 1C standard on restriction of external code execution: <https://its.1c.ru/db/v8std/content/669/hdoc>
- public mirror: <https://v8std.ru/std/774/>

These sources justify the security rationale. They do not uniquely determine the exact list of methods used by this implementation.

## Implementation notes

The current implementation in `bsl-analyzer` is local:

- method matching is performed during the project's own AST-to-HIR lowering;
- the allowlist/denylist of method names is hard-coded in local lowering code;
- the diagnostic currently matches `КомандаСистемы`, `ЗапуститьСистему`, `ЗапуститьПриложение`, `НачатьЗапускПриложения`, `ЗапуститьПриложениеАсинх`, `ЗапуститьПрограмму`, `ОткрытьПроводник`, and `ОткрытьФайл`;
- it does not currently cover every API discussed in `#std774`.

## Residual risk

Residual risk is low to medium.

- the rule concept itself is safe and public;
- the exact method set is an implementation choice rather than a direct transcription of the standard;
- the main cleanup needed here was to align the documentation with the actual behavior.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket.
