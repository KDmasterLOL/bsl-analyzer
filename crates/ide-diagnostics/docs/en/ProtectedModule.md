# Protected modules (ProtectedModule)

## Description

This diagnostic reports password-protected common modules found in the
configuration metadata.

When a common module is protected, its source code is not available in ordinary
text form. That makes review, auditing, search, and normal version-control
workflow harder. Because of that, such modules deserve explicit attention.

In the current project the diagnostic is evaluated only from `SessionModule` and
produces one diagnostic per protected common module found in metadata.

## Sources

- Secondary reference: [v8std.ru: ProtectedModule](https://v8std.ru/diagnostics/bslls/ProtectedModule/)
