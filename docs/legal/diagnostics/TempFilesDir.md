# TempFilesDir provenance

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule follows directly from public 1C guidance around temporary files: temporary file names should normally be obtained through `ПолучитьИмяВременногоФайла()` so the platform can manage their lifecycle. This is a public filesystem-usage rule, not a unique analyzer-specific idea.

## Public sources

- `#std542` "Доступ к файловой системе из кода конфигурации"

## Audit result

The current implementation is local Rust code with a deliberately narrow detector:

- it reports direct global calls to `КаталогВременныхФайлов()` / `TempFilesDir()`;
- it ignores qualified calls like `Модуль.КаталогВременныхФайлов()`.

## Important caveats

- The implementation is narrower than the full public guidance in `#std542`.
- It does not verify whether the surrounding code later deletes the file or otherwise handles temporary resources safely.
- It flags the API usage pattern itself, not the full lifecycle of a temporary file or directory.

## Conclusion

`TempFilesDir` looks like a strong permissive candidate. The rule is standards-based, and the current implementation is local narrow-pattern detection with clearly documented limits.
