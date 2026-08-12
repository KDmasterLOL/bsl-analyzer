# Unknown bare names: verified 1C semantics

This note records the Stage-0 contract probe for `UnresolvedName` (issue #185).
It is evidence for resolver capabilities; it is not a parser fixture.

## Probe environment

- 1C:EDT CLI: `2026.1.2.2` (`edt-headless:2026.1.2`)
- imported platform: `8.3.27.1644`
- command: headless `1cedtcli import --build true`, followed by
  `1cedtcli validate`
- source configuration: the repository's designer fixture, copied to a temporary
  directory and augmented with labelled probe expressions
- raw report: `/tmp/issue185-stage0/out/stage0.tsv` on the probing host

The relevant compiler diagnostics are the unqualified EDT configuration errors,
not optional code-style findings.

## Verified matrix

| Source form | EDT verdict | Resolver contract |
|---|---|---|
| `A = CompletelyUnknown` | `Variable ... is not defined` | a complete miss is absent |
| `CompletelyUnknownCall()` | `Procedure or function ... is not defined` | a complete call miss is absent |
| read before later implicit assignment | undefined at the earlier read | implicit locals are flow-sensitive |
| read after implicit assignment | accepted | assignment introduces the local from that point |
| read before an assignment later in the same loop body (method and module code) | accepted | loop CFG/repeating scope can make the assignment reach the earlier read; textual order alone is insufficient |
| `Перем X` inside a procedure | syntax error | BSL has no method-local `Перем` declaration |
| exported global-common-module procedure, called bare | accepted | callable |
| the same exported procedure used as a value | undefined variable | not readable as a value |
| a variable declaration in a common module | `This module can contain only procedures and functions` | common-module variables do not extend a valid global surface |
| managed-application exported variable read from client common module | accepted | readable client global value |
| managed-application exported procedure called from client common module | accepted | callable client global method |
| the same application procedure used as a value | undefined variable | not readable as a value |
| `A = СтрДлина` / `A = STRLEN` | undefined variable | platform functions are not values |
| `СтрДлина("x")` / `StrLen("x")` | accepted | platform functions are callable; aliases are case-insensitive |
| `Метаданные`, `metadata`, mixed-case Russian | accepted | platform properties are bilingual values |
| `ГоризонтальноеПоложение` and `.Право` | accepted | system-enum root is a value |
| `HORIZONTALALIGN` and `.Right` | accepted | system-enum aliases are bilingual/case-insensitive |
| `ГоризонтальноеПоложениеТабличногоДокумента.Право` | root undefined | issue #185 is an unresolved root |
| `РасположениеПолейКомпоновкиДанных.Вместе` | root undefined | issue #185 is an unresolved root |
| a client-only property/enum in a server common module | undefined in `[Server]` | symbol existence and environment availability remain separate facts |
| server-only global-common exports used from a client module | undefined in client environments | export availability is host-specific |

## Consequences

1. `Ty::Unknown` is not evidence of absence.
2. `BareNameUse` is semantic: `Call` and `ReadValue` cannot share an unconditional
   "found by name" verdict.
3. Flow-sensitive implicit-local resolution must include loop back-edges for both
   methods and module initialization code; a plain later assignment still does
   not declare an earlier read.
4. Platform and exported procedures must not suppress an unresolved-value error.
5. Global common modules contribute exported methods, not variables, to valid BSL.
6. Managed application modules contribute exported variables and callable methods
   to the client global context. Until every applicable application-module host is
   indexed with readable/unread completeness, its surface remains an
   `Indeterminate` gap.
7. EDT phrases an unavailable global as "not defined [environment]". The analyzer
   keeps its more precise `UnavailableInEnvironment` ownership for a symbol known
   from an exact catalog; it must not additionally emit `UnresolvedName`.
8. `ExternalConnectionModule.bsl` is the corresponding fixed global-context host
   for external-connection execution. Its exported declarations and unread-body
   completeness must be inventoried alongside application-module hosts; they must
   never be mistaken for server-global declarations.

## Reproduction

The probe used `scripts/extract-edt-platform-globals.py` only for catalog
extraction; validation itself used the existing external helper
`edt-diagnostics/edt-validate.sh`. The exact source expressions and line mapping
are retained in the implementation session artifact `/tmp/issue185-stage0`.
