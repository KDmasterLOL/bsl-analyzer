# Use of system information (UseSystemInformation)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

`SystemInformation` / `СистемнаяИнформация` exposes information about the client environment and system configuration. Such data may be sensitive in security-sensitive code because it can be used for profiling, environment discovery, or other forms of information disclosure.

This diagnostic marks direct construction of that object for manual review.

The rule is disabled by default because not every use is automatically wrong. Some projects may need it for administration or diagnostics, but such usage should be explicit and justified.

## Examples

Incorrect:

```bsl
Info = New SystemInfo;
SendToServer(Info.OSVersion, Info.RAM);
```

More explicit and reviewable approach:

```bsl
// Collect only the exact value that is really required,
// and use this only in a reviewed administrative scenario.
```

## Sources
* Public platform semantics of `SystemInfo` / `СистемнаяИнформация`
