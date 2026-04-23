# MissingTempStorageDeletion provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This rule is grounded in public 1C guidance around temporary storage usage and cleanup. The general requirement to remove data from temporary storage after use comes from public platform and standards materials, not from an original upstream idea.

## Public sources

- `#std487` Minimizing the number of server calls and traffic.
- `#std642` Long-term operations on the server.
- 1C developer documentation on temporary storage.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It:

- finds `ПолучитьИзВременногоХранилища` / `GetFromTempStorage` calls;
- searches for a later `УдалитьИзВременногоХранилища` / `DeleteFromTempStorage` call in the same body;
- matches the first argument structurally rather than by raw text.

This makes member-access cases like `Результат.АдресРезультата` work correctly. At the same time, the implementation is intentionally narrower than the full lifecycle guidance for reusable temporary storage patterns.

## Audit notes

- Rule idea: clean and public-guidance-based.
- Docs were rewritten to avoid overstating platform recommendations that the current implementation does not fully model.
- Existing tests are local and cover direct matches, structural member-access equality, ordering, bilingual names, and simple valid/invalid cases.
