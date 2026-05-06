# Calling a missing common module method (MissingCommonModuleMethod)

> **Deprecated since v0.1.176.** Replaced by
> [`UnresolvedMethodCall`](UnresolvedMethodCall.md) (`BSL-TY-UnresolvedMethodCall`).
>
> Phase 2 of the qualified-call clean-architecture refactor lifted the
> "this is a CommonModule call" classification out of body lowering
> and into hir-ty's `dispatch_bare_ident_field_call`, which has the
> resolver and the receiver's inferred type. Instead of two
> overlapping diagnostics (`MissingCommonModuleMethod` and
> `UnresolvedMethodCall`) the user now sees a single, more precise
> `UnresolvedMethodCall`: `kind: MethodNotFound` when the module is
> registered but the method is missing or non-exported,
> `kind: ReceiverNotResolved` when the receiver name doesn't resolve
> anywhere.
>
> The public surface (`DiagnosticCode` enum, SonarQube rule export,
> `bsl-analyzer.toml` parser) is intentionally kept so existing
> downstream configurations don't break — the rule never fires, but
> mentioning it in `disabled` / `enabled` is still valid. Full removal
> is scheduled for Phase 4 of the refactor.

## Description

This diagnostic reports calls to common module methods that cannot be resolved
as exported methods of the referenced module.

Typical cases:

- the common module does not contain the requested method;
- the method exists, but it is not exported;
- source code for the target common module is unavailable, so its public API
  cannot be confirmed.

The diagnostic does not trigger when the left side of the qualified call is a
local variable or a parameter that shadows the common module name.

## Examples

Incorrect:

```bsl
Процедура Тест()
    ЦеноваяПолитика.РассчитатьСкидку(Сумма);
КонецПроцедуры
```

```bsl
Процедура Тест()
    ОбщегоНазначения.ВнутреннийМетод();
КонецПроцедуры
```

Correct:

```bsl
Процедура Тест()
    ЦеноваяПолитика.ПолучитьСкидку(Сумма);
КонецПроцедуры
```

## Sources

- Secondary reference: [v8std.ru: MissingCommonModuleMethod](https://v8std.ru/diagnostics/bslls/MissingCommonModuleMethod/)
