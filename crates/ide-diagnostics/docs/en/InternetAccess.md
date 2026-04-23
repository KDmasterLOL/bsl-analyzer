# Referring to Internet resources (InternetAccess)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
This diagnostic is a security-audit rule for code that creates objects used for
network communication or access to Internet-facing resources.

It is intentionally disabled by default. The purpose is not to forbid all such
code automatically, but to make it visible during review and verify that:

- external communication is really required;
- transmitted data is authorized and safe to expose;
- the chosen protocol and endpoint are appropriate;
- access is controlled and documented.

## Examples
```bsl
HTTPConnection = New HTTPConnection("api.example.com", 80);
FTPConnection = New FTPConnection(Server, Port, User, Pwd);
MailClient = New InternetMail();
```

```bsl
// Review required:
// - is external access expected here?
// - is the destination controlled?
// - are secrets or protected data involved?
```

## Sources

This diagnostic has no direct one-to-one normative 1C standard source.

Related public context:

* [ITS / v8std #std794: Restrictions on the use of external resources](https://its.1c.ru/db/v8std#content:794:hdoc)
* [ITS / v8std #std678: Server API security](https://its.1c.ru/db/v8std#content:678:hdoc)
