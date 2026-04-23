# ServerSideExportFormMethod provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows from public 1C guidance about form modules and export methods: export methods of managed forms are meaningful only in the client context, while server-side export methods in forms are discouraged or nonsensical in normal platform usage. This is a public platform-design concern, not a unique analyzer-specific idea.

## Public sources

- `#std630` "Правила создания модулей форм"
- `#std544` "Ограничения на использование экспортных процедур и функций"
- 1C UI development guide chapter about form-module execution on client and server

## Audit result

The current implementation is local Rust code that:

- only runs for `FormModule`;
- only applies to managed forms;
- reports exported procedures and functions;
- requires the explicit `&НаКлиенте` annotation.

## Important caveats

- The implementation is narrower than the broad standards text: it does not ban all export methods in forms, only those that are not explicitly client-side.
- Ordinary forms are ignored.
- The rule is syntax- and metadata-driven; it does not inspect actual external call sites.

## Conclusion

`ServerSideExportFormMethod` looks like a strong permissive candidate. The rule is grounded in public platform guidance, and the current implementation is local item-tree validation with a clearly documented scope.
