# Unresolved name (UnresolvedName)

Reports a bare-name read only when the name is proven absent from every applicable
local, module, user-global, and platform surface.

The rule stays silent when any surface is incomplete, for example when a global
common-module body could not be read or the bundled platform catalog version is
not attested. A missing member of a known object belongs to a different diagnostic.

## Example

```bsl
Result = UnknownName.Property;
```

Correct the spelling or declare the corresponding variable, method, or global
symbol.
