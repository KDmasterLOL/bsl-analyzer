# Provenance: IsInRoleMethod

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is based on public 1C guidance about access checks and role
design.

The closest public sources are:

- `#std737`, which says metadata access checks in code should normally use
  `ПравоДоступа`, not `РольДоступна`;
- `#std689`, which allows checking a role in code when the role represents an
  additional application-level right rather than access to metadata objects.

So the core rule idea is public: `РольДоступна` is not a general substitute for
`ПравоДоступа`, and additional-role checks often require explicit handling of
privileged execution.

## Audit result

### Production code

Current implementation in
`crates/ide-diagnostics/src/handlers/is_in_role_method.rs` is local and
HIR-based:

- it scans method bodies and module-level code independently;
- it tracks local variables assigned from `IsInRole()` / `РольДоступна()` and
  `PrivilegedMode()` / `ПривилегированныйРежим()`;
- it reports direct calls or tracked variables used in `if` / `elsif`
  conditions without privileged-mode protection.

This is clearly a local implementation, not a Java-to-Rust line-by-line port.

### Documentation

Local RU/EN documentation was rewritten during this audit to describe the
current implementation honestly:

- use `ПравоДоступа` for metadata access checks;
- use `РольДоступна` only for additional marker roles;
- current analyzer logic checks unprotected usage in conditions.

### Tests

Current tests are local inline Rust scenarios covering:

- direct `РольДоступна` / `IsInRole` calls;
- protected and unprotected conditions;
- tracked local variables;
- reassignment clearing tracked values;
- `elsif` branches;
- Russian and English spellings.

No external upstream fixture file is required.

## Important caveat

The public standards explain when `РольДоступна` is appropriate, but they do
not define the exact detection algorithm used here. The exact implementation in
`bsl-analyzer` is local policy.

Also, the current diagnostic is narrower than the full public guidance:

- it only checks `if` / `elsif` conditions;
- it only treats `PrivilegedMode()` as the protection mechanism;
- it does not model `Пользователи.РолиДоступны` from BSP or broader
  full-privilege patterns.

## Remaining caveats

- repository history may still contain earlier wording closer to upstream docs;
- repository-wide relicensing still depends on the broader audit of shared HIR
  infrastructure.

## Conclusion

`IsInRoleMethod` is a strong permissive candidate because the rule idea is
public and security-related, while the current implementation, tests, and
documentation are local.
