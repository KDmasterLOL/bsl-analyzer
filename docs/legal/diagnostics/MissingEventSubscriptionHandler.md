# MissingEventSubscriptionHandler provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This is a generic metadata-integrity and API-wiring rule. Validating that event subscriptions point to an existing exported server-side handler is a direct consequence of the platform configuration model, not a unique upstream idea.

## Public sources

- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and metadata-based. It runs only for `SessionModule` files and validates event subscriptions from configuration metadata by checking:

- empty handler;
- malformed handler format;
- missing common module;
- non-server common module;
- missing method;
- non-exported method.

Findings are intentionally reported at the start of `SessionModule`, because the issue originates in subscription metadata rather than in a concrete source position.

## Audit notes

- Rule idea: clean and metadata-driven.
- Docs were rewritten to match the actual validation scope.
- Existing tests are local and cover metadata loading, handler resolution, export checks, and SessionModule-only behavior.
