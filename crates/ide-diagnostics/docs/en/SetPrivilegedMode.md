# Using privileged mode (SetPrivilegedMode)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
This diagnostic marks code that enables privileged mode through `SetPrivilegedMode` / `УстановитьПривилегированныйРежим`. Privileged mode disables normal access-right checks, so every such call deserves manual review.

The current implementation is intentionally simple:

- calls that enable privileged mode are reported;
- `SetPrivilegedMode(False)` is ignored;
- the rule does not try to decide whether the call is justified.

So this is a security-hotspot rule, not an automatic proof that the code is wrong.

## Examples

```bsl
SetPrivilegedMode(True); // review required

Value = True;
SetPrivilegedMode(Value); // review required

SetPrivilegedMode(False); // ignored
```
## Sources

- [#std485: Using privileged mode (RU)](https://its.1c.ru/db/v8std#content:485:hdoc)
- [#std678: Server API security (RU)](https://its.1c.ru/db/v8std#content:678:hdoc)
- [#std669: Restriction on executing external code (RU)](https://its.1c.ru/db/v8std#content:669:hdoc)
