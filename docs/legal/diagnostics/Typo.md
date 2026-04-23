# Typo provenance

## Status

Candidate for `MIT OR Apache-2.0`, with an important quality caveat.

## Why

The idea of dictionary-based typo detection is generic and not specific to any upstream project. There is no visible sign here of a unique analyzer-specific rule concept that would block permissive licensing.

## Public sources

- General dictionary-based spell checking concepts.
- Embedded Hunspell dictionaries and local exception lists.

## Implementation notes

The current implementation is local and heuristic. It:

- uses embedded Russian and English Hunspell dictionaries via `spellbook`;
- splits camelCase identifiers into separate chunks;
- checks selected syntax nodes only: method names, variable names, parameter names, assignment lvalues, and string literals;
- skips some format-string-like literals;
- relies on large hardcoded exception lists and user-configurable ignore words.

## Audit notes

- Rule idea: clean and generic.
- Quality caveat: this is **not** an adequate full spell checker for real 1C code. It has significant false positives and false negatives on domain vocabulary, abbreviations, and mixed-language identifiers.
- The diagnostic is disabled by default for exactly that reason.
- Docs were rewritten to reflect the real state of the feature instead of presenting it as robust spelling validation.
