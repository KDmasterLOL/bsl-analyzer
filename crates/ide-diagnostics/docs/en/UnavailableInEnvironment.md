# Access to API unavailable in the execution environment (UnavailableInEnvironment)

## Description

The 1C:Enterprise platform API is not uniformly available across execution environments: some methods, properties, and global functions exist only on the server, others only in specific client kinds (the platform syntax helper documents this under "Availability"). This diagnostic reports access to a platform member from code that runs in an environment where the member does not exist: for example, calling server-side API from a method behind the `&AtClient` directive, or using a type with no web-client support in client code.

The call site's environment set is the intersection of the module's environments (module kind, common-module flags: Server, ClientManagedApplication, ServerCall, ExternalConnection) and the method's compilation directive (`&AtClient`, `&AtServer`, `&AtClientAtServer`, …). The message qualifier lists only the environments where the member is missing — mirroring 1C:EDT verdicts like "… is not defined [Web client]".

Current limitations (deliberately conservative):

- code inside any `#If … #EndIf` preprocessor block is not checked — the preprocessor condition narrows the environment set, and without evaluating it the check would produce false positives;
- extension modules (interceptors `&Instead`/`&Before`/`&After`, `&ChangeAndValidate`) are not checked;
- the mobile client, the external connection, and the legacy thick client (ordinary application) are excluded from the checked environment set by default (`checked_environments`);
- when a platform type name is ambiguous (e.g. `ЭлементыФормы` names both the managed-form items collection and the legacy form controls), availability of its members is not judged.

## Examples

Incorrect:

```bsl
&AtClient
Procedure ReadFile()
    // TextReader is unavailable in the web client
    Reader = New TextReader;
    Line = Reader.ReadLine();
EndProcedure
```

Correct:

```bsl
&AtServer
Procedure ReadFileAtServer()
    Reader = New TextReader;
    Line = Reader.ReadLine();
EndProcedure
```

or with an explicit preprocessor guard:

```bsl
&AtClient
Procedure ReadFile()
    #If Not WebClient Then
    Reader = New TextReader;
    Line = Reader.ReadLine();
    #EndIf
EndProcedure
```

## Sources

- [1C:Enterprise syntax helper, the "Availability" section of each method/property](https://its.1c.ru/db/v8std)
