# IsInRole global method call (IsInRoleMethod)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
Use `AccessRight` / `ПравоДоступа` for access checks against metadata objects.

`IsInRole` / `РольДоступна` is appropriate only for additional marker roles
that do not grant metadata rights directly.

The current implementation reports cases where `IsInRole()` is used in `if` or
`elsif` conditions without `PrivilegedMode()` protection. It also tracks local
variables assigned from `IsInRole()` and later used in such conditions.
## Examples
Invalid: checking metadata access through a role

```bsl
If IsInRole("EditWorldCountries") Then
    AllowEdit = True;
EndIf;
```

Correct:

```bsl
If AccessRight("Edit", Metadata.Catalogs.WorldCountries) Then
    AllowEdit = True;
EndIf;
```

Invalid: additional role check without privileged-mode protection

```bsl
If IsInRole("Treasurer") Then
    OpenTreasuryPanel();
EndIf;
```

Correct:

```bsl
If IsInRole("Treasurer") Or PrivilegedMode() Then
    OpenTreasuryPanel();
EndIf;
```
## Sources
<!-- Необходимо указывать ссылки на все источники, из которых почерпнута информация для создания диагностики -->

* Standard: [Checking access rights (RU)](https://its.1c.ru/db/v8std#content:737:hdoc)
* Standard: [Role and access-right setup (RU)](https://its.1c.ru/db/v8std#content:689:hdoc)
* Public mirror: [v8std.ru / #std737](https://v8std.ru/std/737/)
* Public mirror: [v8std.ru / #std689](https://v8std.ru/std/689/)
