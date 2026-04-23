# Assignment to a read-only property

Fires on an assignment whose left-hand side is a field access on a platform-type property marked `Использование: Только чтение` (read-only) in the platform help book. Examples: `Query.Parameters`, `QueryResult.Columns`.

## Why this is a problem

The parser accepts the statement, but at runtime the assignment either raises an exception or silently fails to modify the object. The idiomatic replacement is typically a dedicated setter method (`SetParameter` for query parameters) or a full rebuild of the object.

## Examples

Wrong:

```bsl
Query = New Query;
Query.Parameters = New Structure; // <-- read-only property
```

Correct:

```bsl
Query = New Query;
Query.SetParameter("Key", Value);
```

## Suppression

If the HBK entry is stale for a given property, or the diagnostic fires on legitimate code, it can be disabled through the analyzer configuration in the usual way.
