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

## Type property addition schema

```json
{
  "schema_version": 1,
  "type_property_additions": [
    {
      "canonical_type": "ClientApplicationForm",
      "russian_name": "РусскоеИмяСвойства",
      "english_name": "EnglishPropertyName",
      "property_types": ["ТипЗначения"],
      "is_readonly": false,
      "min_version": "8.2",
      "evidence_source": "source identifying the platform contract",
      "rationale": "why the extracted data omits or misfiles this property"
    }
  ]
}
```

Adds a standard property the help extract omits or files under a misleading name
— for example the managed-form `РежимОткрытияОкна`, whose help page ships under
its enum type `FormWindowOpeningMode`. `canonical_type` must be a known platform
type; the (type, name) pair must not already exist (that is a correction, not an
addition, and is rejected). `is_readonly` defaults to `false` and `min_version`
is optional. The property receives a synthetic id so documentation lookups do
not alias an existing property.

## Method-local scope

Overlays are deliberately method-local. For example, a DOM `appendChild` correction
that widens an argument to accept an HTML element applies only to that method's
parameter and does not declare a global subtype relation between HTML and DOM
element types.

## Evidence requirements

Every override must include:
- `evidence_source`: A link to official documentation (ITS, syntax assistant), a minimal reproduction script proving the platform behavior, or a specific extracted platform record (for example, a syntax-assistant snippet or a JSON field reference) that demonstrates the contract.
- `rationale`: A clear explanation of why the current extracted data is insufficient and how the override improves type safety without introducing false positives.

Overrides must be justifiable by verifiable, specific evidence that supports the narrow correction without introducing false positives.
