# ForbiddenMetadataName provenance

## Assessment

`ForbiddenMetadataName` is a good candidate for `MIT OR Apache-2.0`.

The rule follows directly from the public 1C standard `#std474`: metadata object names must not collide with query table names such as `Документ`, `Справочник`, `РегистрСведений`, and similar reserved words.

## Source basis

- 1C standard on name, synonym, and comment: <https://its.1c.ru/db/v8std/content/474/hdoc>
- public mirror: <https://v8std.ru/std/474/>

In that standard there is an explicit requirement not to use names that coincide with query table names, because this worsens readability and leads to query errors.

## Implementation notes

The current implementation in `bsl-analyzer` is local:

- it uses a project-defined set of forbidden Russian and English names;
- it checks common modules, metadata objects, attributes, tabular sections, dimensions, resources, and session-module-visible objects;
- diagnostics are produced through the project's own metadata model and module metadata resolution.

The exact forbidden-name set is an implementation detail of this project, but it is a straightforward realization of the public standard requirement.

## Residual risk

Residual risk is low.

- the rule is explicitly standard-based;
- the implementation is local and metadata-driven;
- the main cleanup needed here was documentation wording and explicit provenance.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket.
