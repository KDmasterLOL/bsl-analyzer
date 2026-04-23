# UnusedLocalVariable provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

This is a generic maintainability rule: local variables that are never read should be removed. The idea is standard static-analysis guidance and not a unique analyzer-specific invention.

## Public basis

There is no direct `v8std` mapping required for the rule itself.

## Audit result

The current implementation is local Rust code and one of the stronger local implementations in this diagnostics set. It uses module-level CFG and backward liveness analysis to determine whether a local variable ever becomes live.

The implementation also contains local project-specific exclusions for object and form attributes that can look like local variables in module code.

## Important caveats

- The exact skip lists for object attributes and standard form properties are local implementation details.
- This is a semantic dataflow rule, not a simple pattern match or parser-port rule.

## Conclusion

`UnusedLocalVariable` looks like a strong permissive candidate. The rule is generic, and the current implementation is clearly local and dataflow-based.
