# Storing confidential information in code (UsingHardcodeSecretInformation)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Confidential values should not be stored directly in source code.

The current implementation is primarily focused on hardcoded passwords and password-like fields. By default it looks for names such as `Password` / `Пароль`, and it also checks several structural patterns such as:

* assignments to fields with password-like names
* `Insert("Password", "...")` into structures or maps
* `New Structure(..., "Password", "...")`
* password arguments in `HTTPConnection` / `FTPConnection`

If the project uses SSL sub-system, then passwords should be stored in safe storage.

### Addition

Strings with all symbols `*` are excluded from the check:

```bsl
Password = "**********";
```

## Examples

Incorrect:

```bsl
Password = "12345";
```

Correct:

```bsl
Passwords = CommonModule.ReadDataFromSafeStorage("StoringIdentifier", "Password");
Password = Passwords.Password;
```

## Sources

* [Standard: Store passwords safe (RU)](https://its.1c.ru/db/v8std#content:740:hdoc)
* [v8std: UsingHardcodeSecretInformation](https://v8std.ru/diagnostics/bslls/UsingHardcodeSecretInformation/)
