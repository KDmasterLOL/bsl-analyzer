# Common module invalid type (CommonModuleInvalidType)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

Common modules should use one of the standard execution-context combinations
described in the 1C development rules. Arbitrary flag combinations make the
module purpose harder to understand and can produce confusing behavior when the
same module is called from different runtime contexts.

This diagnostic reports common modules whose metadata flags do not match one of
the accepted module types.

| Common module type              | Naming example                 | Server call | Server | External connection | Client (Ordinary application) | Client (Managed application) |
| ------------------------------- | ------------------------------ | ----------- | ------ | ------------------- | ----------------------------- | ---------------------------- |
| Server-side                     | Common (or CommonServer)       |             | +      | +                   | +                             |                              |
| Server-side to call from client | CommonServerCall               | +           | +      |                     |                               |                              |
| Client-side                     | CommonClient (or CommonGlobal) |             |        |                     | +                             | +                            |
| Client-server                   | CommonClientServer             |             | +      | +                   | +                             | +                            |


## Examples

### Wrong

A common module combines flags in a way that does not correspond to `Server`,
`ServerCall`, `Client`, or `ClientServer`.

### Correct

Choose one of the standard module types and align both the metadata flags and
the module naming convention with that type.

## Sources

Primary source: [Standard: rules for creating common modules (RU)](https://its.1c.ru/db/v8std#content:469:hdoc:1.2)

Secondary source: [v8std.ru: #std469](https://v8std.ru/std/469/)

Additional reference: [v8std.ru: CommonModuleInvalidType](https://v8std.ru/diagnostics/bslls/CommonModuleInvalidType/)
