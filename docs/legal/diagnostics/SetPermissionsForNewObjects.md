# SetPermissionsForNewObjects provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows directly from public 1C access-rights guidance: the `Set permissions for new objects` flag should remain enabled only for full-access administrative roles. This is a public security and role-configuration concern, not a unique analyzer-specific idea.

## Public sources

- `#std532` "Установка прав для новых объектов и полей объектов"
- `#std689` "Настройка ролей и прав доступа"

## Audit result

The current implementation is local Rust code that:

- only runs for `ManagedApplicationModule`;
- loads configuration metadata;
- iterates role definitions;
- reports every role with `setForNewObjects=true` unless its name is in the configured allowlist.

## Important caveats

- Diagnostics are attached to the beginning of `ManagedApplicationModule`, because the issue lives in role metadata rather than in a specific code line.
- The allowlist parameter `namesFullAccessRole` is a local project-specific configuration choice.
- The implementation trusts role names, not some deeper semantic notion of “administrator role”.

## Conclusion

`SetPermissionsForNewObjects` looks like a strong permissive candidate. The rule is standards-based, and the current implementation is local metadata-driven validation with a clearly documented configuration surface.
