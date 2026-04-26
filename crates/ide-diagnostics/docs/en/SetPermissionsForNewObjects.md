# The check box «Set permissions for new objects» should only be selected for the FullAccess role (SetPermissionsForNewObjects)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

The `Set permissions for new objects` flag should normally be enabled only for full-access administrative roles. If it is enabled for another role, that role will automatically accumulate permissions for newly created metadata objects.

The current implementation loads role metadata and reports every role with `setForNewObjects=true` unless its name is explicitly included in the allowed-role list.

## Examples

Incorrect:

```text
Role: Manager
  SetPermissionsForNewObjects = true
```

Correct:

```text
Role: FullAccess
  SetPermissionsForNewObjects = true

Role: Manager
  SetPermissionsForNewObjects = false
```

## Configuration

- `namesFullAccessRole` (string, default: `FullAccess,ПолныеПрава`)  
  Comma-separated list of role names that are allowed to keep this flag enabled.

## Sources

- [#std532: Setting rights for new objects and object fields (RU)](https://its.1c.ru/db/v8std#content:532:hdoc)
- [#std689: Roles and access-right configuration (RU)](https://its.1c.ru/db/v8std#content:689:hdoc)
