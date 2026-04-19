# FileSystemAccess provenance

## Assessment

`FileSystemAccess` is a good candidate for `MIT OR Apache-2.0` at the rule level, but the exact method and constructor coverage is a local implementation choice rather than a direct transcription of one standard.

The security rationale is public and standard-driven: access to files and directories from configuration code requires careful review because it depends on OS permissions, temporary-file handling, client/server boundaries, and safe path construction.

## Source basis

- 1C standard on file system access from configuration code: <https://its.1c.ru/db/v8std/content/542/hdoc>
- 1C standard on application launch security: <https://its.1c.ru/db/v8std/content/774/hdoc>
- Developer's Guide on safe mode: <https://its.1c.ru/db/v8323doc#bookmark:dev:TI000000186>
- public mirror: <https://v8std.ru/std/542/>

These sources justify the security concern and the need for manual review.

## Implementation notes

The current implementation in `bsl-analyzer` is local:

- detection happens during the project's own AST-to-HIR lowering;
- it matches a conservative list of constructor types and global/object methods related to file access;
- annotations do not suppress the diagnostic;
- the diagnostic is intentionally disabled by default and used as an audit aid.

## Residual risk

Residual risk is low to medium.

- the rule concept is public and defensible;
- however, the exact API list is project-specific and broader than a single normative statement;
- this is acceptable, but it should be understood as a local security policy layered on top of public guidance.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket, with a note that its exact coverage is a local policy decision.
