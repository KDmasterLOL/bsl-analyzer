# Using unavailable in Unix objects (UsingObjectNotAvailableUnix)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

On Linux, technologies such as `COM`, `OLE`, and `ActiveDocument` are unavailable. When cross-platform support is required, such integrations should be replaced with alternatives such as XML exchange, HTTP/web services, or `NativeAPI` add-ins.

The current implementation reports use of:

* `COMObject`
* `Mail`

### Addition

The diagnostic also checks whether the call is guarded by a platform condition. In the current implementation, reporting is suppressed when a surrounding condition mentions platform-specific markers such as:

* `Linux_x86`
* `Windows`
* `MacOs`

## Examples

```bsl
Component = New COMObject("System.Text.UTF8Encoding");
```

or

```bsl
Mail = New Mail;
```
Instead of this you can use `StartApplication()`.

```bsl
SystemInformation = New SystemInformation();
If Not SystemInformation.PlatformType = PlatformType.Linux_x86 OR PlatformType.Linux_x86_64 Then
    Mail = New Mail;
EndIf;
```

## Sources

* [Features of the development of cross-platform applied solutions (RU)](https://its.1c.ru/db/v8314doc#bookmark:dev:TI000001208)
* [Features of the client application running Linux (RU)](https://its.1c.ru/db/v8314doc#bookmark:dev:TI000001283)
