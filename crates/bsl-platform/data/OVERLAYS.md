# Curated platform overlays

`platform_overlays.json` is project-maintained factual correction data. It is
not generated and is merged into the extracted platform JSON by `build.rs`
before Rust structures are generated. The base `platform_data.json` remains an
unchanged extract of the 1C platform help archives.

## Method parameter override schema

```json
{
  "schema_version": 1,
  "method_parameter_overrides": [
    {
      "canonical_type": "EnglishCanonicalType",
      "russian_name": "РусскоеИмяМетода",
      "english_name": "EnglishMethodName",
      "min_version": "8.1",
      "max_version": "8.3.99",
      "parameter_index": 0,
      "replacement_type_list": ["TypeA", "TypeB"],
      "evidence_source": "source identifying the platform contract",
      "rationale": "why the extracted signature needs this narrow correction"
    }
  ]
}
```

`min_version` and `max_version` are optional inclusive bounds for the target
method's documented minimum platform version. The build rejects malformed
entries, missing or ambiguous RU/EN targets, duplicate parameter overrides,
out-of-range parameter indices, invalid bounds, and duplicate type-list
members. Applying an override changes only the selected parameter type list;
method IDs and method ordering remain those of the extracted data.

The DOM append-child correction is deliberately method-local. It does not
declare a global subtype relation between HTML and DOM element types.
