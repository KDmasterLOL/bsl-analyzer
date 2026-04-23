# UsingHardcodeSecretInformation provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

This is a generic security rule: secrets should not be hardcoded in source code. That idea is common secure-coding guidance and not a unique analyzer-specific invention.

## Public sources

- `#std740` "Безопасное хранение паролей"
- `v8std.ru/diagnostics/bslls/UsingHardcodeSecretInformation/` as a secondary public reference

## Audit result

The current implementation is local Rust code. By default it focuses on password-like fields via configurable search words (`Пароль|Password`) and several local structural patterns:

- direct assignment to password-like identifiers or fields
- insertion into structures/maps under password-like keys
- `New Structure` / `New Map` style initialization with password-like keys
- password arguments in HTTP/FTP connection constructors

It also excludes empty strings and masking strings made only of `*`.

## Important caveats

- Despite the broader wording often used in generic docs, the current default behavior is mainly about passwords unless `searchWords` is reconfigured.
- The exact search words and supported structural patterns are local implementation policy of this project.

## Conclusion

`UsingHardcodeSecretInformation` looks like a strong permissive candidate. The rule is generic, and the current implementation is clearly local security logic with configurable keyword coverage.
