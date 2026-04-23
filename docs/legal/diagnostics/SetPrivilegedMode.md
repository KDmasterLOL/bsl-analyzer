# SetPrivilegedMode provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows from public 1C security guidance: enabling privileged mode disables normal access-right checks and therefore should be treated as a dangerous operation that requires review. This is a public security concern, not a unique analyzer-specific idea.

## Public sources

- `#std485` "Использование привилегированного режима"
- `#std678` "Безопасность прикладного программного интерфейса сервера"
- `#std669` "Ограничение на выполнение внешнего кода"

## Audit result

The current implementation is local Rust code with a deliberately narrow hotspot detector:

- it reports calls that enable privileged mode;
- it ignores `SetPrivilegedMode(False)` / `УстановитьПривилегированныйРежим(Ложь)`;
- it emits a generic review message without trying to prove the call is definitely wrong.

## Important caveats

- This is a security-hotspot rule, not a full semantic verifier.
- The implementation does not analyze whether access rights were manually checked before the call.
- It does not distinguish all possible safe and unsafe contexts; it simply highlights enabling privileged mode for review.

## Conclusion

`SetPrivilegedMode` looks like a strong permissive candidate. The rule is grounded in public security guidance, and the current implementation is local project-specific hotspot detection with clearly documented limitations.
