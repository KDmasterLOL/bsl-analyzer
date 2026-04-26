# Form fields do not have a data path (WrongDataPathForFormElements)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

# Form fields do not have a data path (WrongDataPathForFormElements)

|  Type   | Scope |  Severity  | Activated<br>by default | Minutes<br> to fix |      Tags       |
|:-------:|:-----:|:----------:|:-----------------------------:|:------------------------:|:---------------:|
| `Error` | `BSL` | `Critical` |             `Yes`             |           `5`            | `unpredictable` |

<!-- Блоки выше заполняются автоматически, не трогать -->
## Diagnostics description
This diagnostic reports broken form bindings that are already visible in form metadata.

When a form attribute is deleted, renamed, or disconnected from an element, the platform stores the element `DataPath` with a `~` prefix in the XML form representation. Such a value points to an unresolved reference, for example `~Object.Description`.

In practice this usually appears after refactoring form attributes, changing the main form attribute, or manually changing a dynamic list query without restoring the element bindings. The element may stop working correctly in Designer and can disappear from the form.

The current implementation is metadata-based. It checks only form elements whose saved `DataPath` starts with `~`.

## Examples

### Broken binding in form XML

```xml
<DataPath>~Object.MissingAttribute</DataPath>
```

### Valid binding

```xml
<DataPath>Object.Description</DataPath>
```

## Sources
- [General requirements - Standards 1C (RU)](https://its.1c.ru/db/v8std#content:467:hdoc)
- [v8std.ru: WrongDataPathForFormElements (RU)](https://v8std.ru/diagnostics/bslls/WrongDataPathForFormElements/)
