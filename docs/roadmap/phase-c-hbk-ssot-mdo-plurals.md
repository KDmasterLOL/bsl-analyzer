# Phase C — HBK as source-of-record for MDO plural names & docs

> **Self-contained plan.** Future you, post-compaction: this document is the
> primary reference. Everything you need to execute the work is here. The
> two prior phases referenced below are already shipped on `develop` (commits
> through `v0.1.183`).

## Where this fits

This is the **third** step in a long-running consolidation of HBK ↔ workspace
responsibility in the BSL LSP. Prior steps:

1. **Phase A — HBK platform properties on `*Object` / `*Ref` MetadataRef
   receivers.** Plan: `/home/itrous/.claude/plans/shiny-leaping-church.md` (now
   stale — that file currently holds the Phase B plan; consult `git log
   crates/hir-ty/src/field_enum.rs` for the actual diff).
   Effect: bare `Документы.ПКО.СоздатьДокумент().<TAB>` now surfaces
   `ДополнительныеСвойства`, `Движения`, `ВерсияДанных`, … from HBK on top of
   workspace attributes / tabular sections. Reuse pattern lives in
   `crates/hir-ty/src/field_enum.rs::push_platform_prefix_properties`.

2. **Phase B — HBK as SSoT for non-MDO global identifiers** (this
   conversation's main work). Bare `Метаданные`/`ОбработкаОшибок`/… now show
   in completion + hover with rich HBK markup, gated by
   `Resolver::with_workspace_scope.user_common_module_exists` (same gate
   `infer.rs:1428, :1493` uses) and by full `TyLoweringContext::lower_bare_name`
   equality against `inferred_ty`. Plan: same shiny-leaping-church.md.

Phase C is the **final** cleanup of the layer split. After Phase C, HBK is
the registry-of-record for *every* global identifier name and its
documentation; `MdoType` remains as a compile-time discriminator for
workspace-aware typing only.

## Codex pair-review summary

**First pass — REWORK.** Concrete findings applied in this revision:

1. ❌ Plan claimed wholesale unification of all 20 MdoTypes through HBK. ✅
   Codex found `Cube`, `DimensionTable`, `CommonModule` have NO `Global
   context` HBK entry (they're nested under other managers in `platform_data.json`).
   `MdoType::all()` has 20 variants; HBK covers 17 of them as globals.
   Plan revised: **enrich-where-possible**, not wholesale replacement.
2. ❌ Plan's first draft referenced a nonexistent `PlatformProperty` field for documentation.
   ✅ Docs are fetched via `PlatformDataInner::get_property_docs(prop.id) ->
   PropertyDocs { description, notes, see_also }` (see `crates/bsl-platform/src/db.rs:457-460`,
   `crates/ide/src/hover.rs:578-589`). All references switched to the
   `get_property_docs(prop.id)` API.
3. ❌ `ТаблицыИзмерений` (HBK plural form) vs `таблицыизмерения` (`MdoType::from_plural`-accepted form) — round-trip leak. Actually moot: there is no global-context `ТаблицыИзмерений` HBK property anyway (it's nested under `ExternalDataSourceCubeManager`). Plan now skips DimensionTable explicitly via the 17-vs-20 split, not via `from_plural`.
4. ❌ `ty_info_markup` enrichment would affect every call site (`hover.rs:141, :244, :500, :509, :518` — field hover, variables, params, locals). ✅ HBK enrichment for `Ty::ManagerCollection` gated to `hover_free_name` only.
5. ❌ C-3 (hover) bundled with completion. ✅ **Split**: this PR ships C-1/C-2 (completion only). Hover enrichment (C-3) is now Phase D, separate PR.
6. ❌ Existing test `hover_bare_mdo_plural_keeps_manager_collection` (`hover_globals.rs:90-107`) asserts `!markup.contains("Только чтение")` — would invert under C-3. ✅ Test invalidation is now Phase D's problem.
7. ❌ Sync gap silent: new HBK global with no matching `MdoType` variant would vanish. ✅ **Limitation acknowledged honestly.** Test #10 is a regression pin (frozen baseline catches accidental removal/rename of any of the 17 known MDO plurals). It does NOT auto-detect a new HBK MDO plural without a matching `MdoType` variant — both `complete_mdo_plurals` and any HBK-derived "expected" set route through `MdoType::from_plural`, making detection tautological. Sync-gap discovery is procedural: when `platform_data.json` is regenerated, the regen workflow (`crates/bsl-platform/data/PROVENANCE.md`) must include a manual diff-review step for new global-context properties whose declared type ends in `<X>Менеджер`/`<X>Manager`. No clean fully-automated detection exists without a curated whitelist that itself drifts.
8. ❌ `completion_globals.rs:184-198` asserts `detail.starts_with("Коллекция метаданных")` — must preserve. ✅ New detail format keeps that prefix; explicit regression pin in test plan.
9. ❌ Runtime `tracing::warn!` heuristic (`is_manager_typed_global`) false-positives on existing non-MDO manager APIs (`БиблиотекаКартинокМенеджер`, `WSСсылкиМенеджер`). ✅ Warning dropped. Sync-gap detection moved to CI-time integration test (#10).
10. ❌ Three nested types (`Cube`/`DimensionTable`/`CommonModule`) kept via hardcoded fallback — preserves a real bug (`Кубы.X(...)` doesn't compile in BSL). ✅ Dropped from top-level completion entirely. HBK header semantics make this rigorous: only properties with `type_name == "Global context"` are bareword-accessible; HBK classifies these three as type descriptors. Reflective access (`Метаданные.ОбщиеМодули.<X>`) continues to work via existing dot-completion.
11. ❌ `MdoType::all()`-driven iteration leaks compile-time enum as the registry. ✅ Inverted: `complete_mdo_plurals` iterates `all_global_properties()` (HBK-driven) and discovers MDO plurals via `MdoType::from_plural`. HBK becomes the iteration driver; `MdoType` is downgraded to a name-pattern discriminator.

## Problem — registry duplication where HBK has coverage

Three layers maintain overlapping MDO plural metadata:

| Layer | File | Form | Coverage |
|---|---|---|---|
| `MdoType` enum | `crates/bsl-metadata/src/metadata_object.rs:17-` | Compile-time enum, 20 variants. | All 20. |
| `mdo_type_plural_ru/en()` lookup tables | `crates/ide/src/completion/bsl_completion.rs:404-453` | Compile-time `match` returning bilingual plurals. | All 20. |
| HBK `global_properties` partition | `crates/bsl-platform/data/platform_data.json` (entries with `type_name == "Global context"`) | Runtime data: `{name, english_name, property_types, is_readonly, min_version, context}` + docs via `get_property_docs(id)`. | **17 of 20.** `Cube`, `DimensionTable`, `CommonModule` are NOT global-context properties (they live nested under `ExternalDataSourceManager`, `ExternalDataSourceCubeManager`, `ConfigurationMetadataObject` respectively). |

The 17-vs-20 split is **HBK's own taxonomy decision**, not an extractor
gap. The HBK page header explicitly distinguishes:

- `Глобальный контекст.Документы` / `Глобальный контекст.Справочники` /
  … — global-context properties, bareword-accessible in BSL
  (`Документы.<X>`).
- `ОбщийМодуль` / `Куб` / `ТаблицаИзмерения` (no `Глобальный контекст.`
  prefix) — type descriptors for instances; NOT bareword-accessible.
  Reflective access goes through `Метаданные.ОбщиеМодули.<X>` etc., which
  already works in our LSP via dot-chain on `Метаданные`'s
  `ОбъектМетаданныхКонфигурация` type.

**Today's bug:** `complete_mdo_plurals` iterates `MdoType::all()`
unconditionally, emitting `Кубы`, `ТаблицыИзмерения`, `ОбщиеМодули` as
top-level bareword completion items. The user sees them, picks one, types
e.g. `Кубы.X(...)` — BSL fails to compile because that's not a valid
bareword path. Misleading UX inherited from the original hardcoded
`mdo_type_plural_ru/en` tables.

**Phase C fix:** drop the three from `complete_mdo_plurals` entirely.
HBK becomes the **sole** source for the 17 valid bareword plurals. No
hardcoded fallback path needed.

Sync-gap risk: when the platform ships a NEW global-context MDO type,
HBK gets a new `Global context` property; we need `MdoType::all()`
extended with the matching variant. Coverage test (#10) catches this at
CI time.

Additional friction:

- Top-level completion for MDO plurals (`Документы`, …) emits the
  hardcoded detail `"Коллекция метаданных (Документ)"` instead of HBK's
  description `"Используется для доступа к определенным в конфигурации
  документам."` Information loss vs HBK.
- HBK readonly/min_version/availability NEVER surface for MDO plurals
  today — neither in completion detail nor in hover. For
  `Документы`/`Справочники`/etc., a user gets less info than for non-MDO
  globals (`Метаданные`/`ОбработкаОшибок`).

## What stays as-is

- `Ty::ManagerCollection(MdoType)` remains the type-system carrier for MDO
  plurals. It's strictly more specific than HBK's `PlatformObject(<X>Manager)`
  — it enables `Документы.<DocName>` chain resolution via workspace
  `module_index`. **Do not** route MDO plural typing through HBK.
- `MdoType` enum stays — it's the compile-time discriminator that
  `MetadataKind::object_kind_for`, `field_enum::enumerate_mdo_fields`,
  `mdo_completion.rs`, etc. need. We just stop using it as the source for
  human-facing names/docs.
- `infer.rs::infer_path_name` cascade order untouched (step 4 MDO plural
  before step 6 HBK global). Phase B already documented and tested this.

## Design — HBK as the sole source for valid bareword plurals

### Scope of this PR

Phase C ships **completion only** (C-1 + C-2). Hover enrichment is split
out as **Phase D** (separate PR) because:
- Existing test `hover_globals.rs::hover_bare_mdo_plural_keeps_manager_collection`
  (lines 90-107) asserts `!markup.contains("Только чтение")`. Phase D
  inverts that expectation; landing it together would force a snapshot
  churn that belongs with hover-only review.
- `ty_info_markup` has 5 call sites; gating HBK enrichment requires
  careful hover-only branching, not a `ty_info_markup` extension.
- Completion change is isolated to `complete_mdo_plurals` (one function);
  hover changes touch `hover_free_name` AND test invariants.

### Direction: HBK-driven iteration, not `MdoType`-driven

The `platform_data.json` extractor already tags every property with its
owner type. Global-context properties carry `type_name == "Global context"`;
nested types (like `Куб` under `ExternalDataSourceManager`) carry the
owner's name instead. `PlatformDataInner::all_global_properties()`
returns exactly the global-context partition
(`crates/bsl-platform/src/db.rs:217-225` via the
`GLOBAL_CONTEXT_OWNER` constant).

So the 17/20 split is **inherent to HBK**, not something the LSP needs
to maintain. Iterating `all_global_properties()` yields the 17 valid
bareword MDO plurals automatically; `Cube`, `DimensionTable`,
`CommonModule` are simply absent from that iteration because HBK
classifies them as type descriptors, not globals.

Phase C inverts today's `MdoType::all()`-driven enumeration: we iterate
HBK globals once, partition by `MdoType::from_plural(prop.name)`, and
render each property in its appropriate band. Single source of truth at
the iteration level; no skip-lists between `complete_mdo_plurals` and
`complete_hbk_globals`.

### Bridge: `MdoType::hbk_global_property() -> Option<&'static PlatformProperty>`

Still useful as a utility for callers that have an `MdoType` in hand and
want the corresponding HBK global property (Phase D hover lookup is the
primary consumer; possibly future code action plumbing). Implemented as
a `OnceLock<FxHashMap<MdoType, &'static PlatformProperty>>` built by
iterating `all_global_properties()` and keying via
`MdoType::from_plural(prop.name)`. The 3 non-bareword variants
(`Cube`, `DimensionTable`, `CommonModule`) get `None` automatically
because HBK doesn't list them as globals.

`complete_mdo_plurals` does **not** consume this bridge in Phase C — it
iterates HBK directly, more aligned with HBK-as-SSoT.

### Phase C-1: `MdoType::hbk_global_property() -> Option<&'static PlatformProperty>`

New method on `MdoType` in `crates/bsl-metadata/src/metadata_object.rs`.

Implementation:

```rust
static HBK_MAP: OnceLock<FxHashMap<MdoType, &'static PlatformProperty>> =
    OnceLock::new();

impl MdoType {
    pub fn hbk_global_property(self) -> Option<&'static PlatformProperty> {
        HBK_MAP
            .get_or_init(|| {
                let mut m = FxHashMap::default();
                for prop in PlatformDataInner::instance().all_global_properties() {
                    let Some(t) = MdoType::from_plural(prop.name.as_str()) else {
                        // No runtime warn: a manager-shaped HBK global
                        // alone is not enough to distinguish a "new MDO
                        // type platform added" from an existing non-MDO
                        // manager-API (`БиблиотекаКартинок:
                        // БиблиотекаКартинокМенеджер`, `WSСсылки`, etc.).
                        // Both look identical structurally. Sync-gap
                        // detection lives in a compile-time-stable
                        // coverage test, not a runtime heuristic — see
                        // test #10 below.
                        continue;
                    };
                    m.insert(t, prop);
                }
                m
            })
            .get(&self)
            .copied()
    }
}
```

`PlatformDataInner::instance()` returns `&'static Self`
(`crates/bsl-platform/src/db.rs:73-75`); `all_global_properties()` borrows
from `self.properties` (`db.rs:421-425`), so `&'static PlatformProperty` is
sound. No `Arc` needed.

**Sync-gap detection — honest limitation.** Earlier drafts tried a
runtime `is_manager_typed_global` predicate to warn when an HBK
manager-typed global had no matching `MdoType`. Codex flagged the
false-positive: HBK hosts non-MDO manager APIs (`БиблиотекаКартинок:
БиблиотекаКартинокМенеджер`, `WSСсылки`, etc.) structurally
indistinguishable from MDO plurals without a curated whitelist. A
HBK-derived "expected" set in test #10 would be tautological — both
sides filter via `MdoType::from_plural`, so a new HBK entry the enum
doesn't recognise is absent from both and the assertion passes silently.

The plan does not attempt fully-automated detection. Test #10 (frozen
baseline) only catches regression in the existing 17. Detection of new
platform-added MDO collections is procedural and documented in
`crates/bsl-platform/data/PROVENANCE.md`: review the `platform_data.json`
regen diff for new global-context properties with manager-typed declared
types; if any are MDO collections, extend `MdoType::all()` and refresh
`expected_mdo_plurals.txt` together.

### Phase C-2: rewrite `complete_mdo_plurals` to iterate HBK directly

`crates/ide/src/completion/bsl_completion.rs`:

- **Delete** `mdo_type_plural_ru` (lines 404-427) and `mdo_type_plural_en`
  (lines 430-453). HBK iteration provides RU + EN names per property; the
  hardcoded tables become dead code.
- Rewrite `complete_mdo_plurals(prefix)`:

  ```rust
  fn complete_mdo_plurals(prefix: &str) -> Vec<CompletionItem> {
      let prefix_lower = prefix.to_lowercase();
      let mut completions = Vec::new();

      // HBK already segregates global-context properties from nested
      // type descriptors via `type_name == "Global context"`. Iterating
      // `all_global_properties()` yields exactly the 17 bareword-valid
      // plural forms; `Cube`, `DimensionTable`, `CommonModule` are
      // absent here because HBK classifies them as type descriptors.
      for prop in PlatformDataInner::instance().all_global_properties() {
          let Some(mdo_type) = MdoType::from_plural(prop.name.as_str())
              .or_else(|| MdoType::from_plural(prop.english_name.as_str()))
          else {
              continue; // non-MDO global; lands in band 25 instead.
          };
          if !matches_prefix_bilingual(&prop.name, &prop.english_name, &prefix_lower) {
              continue;
          }
          completions.push(render_mdo_plural_with_hbk(mdo_type, prop));
      }
      completions
  }
  ```

- `render_mdo_plural_with_hbk(mdo_type, prop)`:
  - `label = prop.name` (RU, as stored by HBK).
  - `kind = CompletionItemKind::MdoType` (workspace shape — preserved UX).
  - `detail` — keeps the existing `"Коллекция метаданных (...)"` prefix
    (pinned by `completion_globals.rs:184-198`). New format:
    `"Коллекция метаданных ({russian_name}) [Только чтение]"` when
    `prop.is_readonly` (HBK marks all 17 readonly today), else just the
    prefix.
  - `documentation` — pulled via
    `PlatformDataInner::instance().get_property_docs(prop.id)` returning
    `PropertyDocs { description, notes, see_also }`
    (`crates/bsl-platform/src/db.rs:457-460`,
    `crates/ide/src/hover.rs:578-589` for the receiver-path consumer).
    Compose: `description` first, then `notes` if non-empty, then
    `see_also` if present. If `get_property_docs` returns `None`, use
    the existing generic "Коллекция объектов метаданных типа X." string
    so the documentation panel is never empty.
  - `filter_text = format!("{} {}", prop.name, prop.english_name)` —
    bilingual.
  - `insert_text = prop.name`.

- `complete_hbk_globals` (band 25) is unchanged structurally — it still
  iterates `all_global_properties()` and skips entries where
  `MdoType::from_plural` returns `Some` (those land in band 20). The two
  bands form a strict partition of the HBK global-context property set:
  band 20 = MDO plurals, band 25 = everything else.

### Out of Phase C (split as Phase D, separate PR)

- Hover enrichment for `Ty::ManagerCollection(MdoType)` to surface HBK
  readonly/min_version/availability/description. Requires inverting the
  existing test `hover_bare_mdo_plural_keeps_manager_collection`
  (`hover_globals.rs:90-107`); separate review surface.

## Layer responsibilities (post-Phase C contract)

| Question | Answered by | Mechanism |
|---|---|---|
| Does identifier `X` exist on global context? | HBK | `all_global_properties()` / `get_global_property(name)` |
| What's the platform-side type of `X`? | HBK | `prop.property_types` |
| What's the human label (RU + EN) for `X`? | HBK | `prop.name` / `prop.english_name` |
| Doc / readonly / availability / min_version of `X`? | HBK | `prop.is_readonly`, `prop.min_version`, `prop.context`; docs via `PlatformDataInner::instance().get_property_docs(prop.id) -> Option<PropertyDocs { description, notes, see_also }>` |
| Is `X` an MDO plural? | `MdoType::from_plural(X)` | Bridge HBK ↔ `MdoType` |
| What concrete documents exist in this config? | workspace | `bsl-metadata` from `Configuration.xml` |
| What's the `Ty` of `X`? | composition | MDO plural → `Ty::ManagerCollection(MdoType)`; else HBK lowering → `lower_bare_name(prop.property_types[0])` |
| Workspace shadows global with same name? | workspace | `Resolver::user_common_module_exists` (Phase B gate) |
| Field/method on `Ty::MetadataRef`? | composition | `mdo.attributes` (workspace) ∪ HBK platform properties (Phase A) |

After Phase C, hardcoded MDO knowledge is bounded to:

- `MdoType` enum — compile-time discriminator (irreducible; Rust types).

That's it. `mdo_type_plural_ru/en` lookup tables are **deleted**.
Display name, docs, readonly, min_version, availability — all sourced
from HBK for the 17 global-context types. The 3 non-global variants
(`Cube`, `DimensionTable`, `CommonModule`) are dropped from top-level
completion automatically because HBK's `all_global_properties()` doesn't
include them; reflective access via `Метаданные.<X>` continues through
existing dot-completion paths.

The architectural shift: top-level completion no longer enumerates MDO
plurals from `MdoType::all()`. It iterates `all_global_properties()` and
**discovers** which entries are MDO plurals via `MdoType::from_plural`.
HBK is the iteration driver; `MdoType` is downgraded to a name-pattern
discriminator. Future platform additions land in HBK first; the only
manual code change is extending `MdoType::all()` with the new variant.

**Sync-gap detection — honest limitation.** Test #10 (frozen baseline)
is a regression pin: it catches accidental removal or rename of any of
the 17 known MDO plurals. It does NOT auto-detect a new HBK MDO plural
that the `MdoType` enum hasn't been extended for — both
`complete_mdo_plurals` and any HBK-derived "expected" set go through
`MdoType::from_plural`, so a new HBK entry the enum doesn't know about
is absent from both sides and the test passes silently. A runtime warn
attempted in an earlier draft couldn't distinguish "platform added a new
MDO type" from existing non-MDO manager APIs
(`БиблиотекаКартинокМенеджер`, `WSСсылкиМенеджер`, …) — both look
structurally identical without a curated whitelist that itself drifts.

The pragmatic detection path is procedural, documented in
`crates/bsl-platform/data/PROVENANCE.md`: when `platform_data.json` is
regenerated, the workflow must include a manual diff-review step for new
global-context properties whose declared type ends in
`<X>Менеджер`/`<X>Manager`. If any are MDO collections, extend
`MdoType::all()` and refresh `expected_mdo_plurals.txt` in the same
commit. Phase C does not introduce a tighter automated check because
none exists without trade-offs (false positives or curated whitelist
drift). Hover-side enrichment for the 17 covered types lands in
Phase D.

## Critical files

Edit:
- `crates/bsl-metadata/src/metadata_object.rs` — add `MdoType::hbk_global_property() → Option<&'static PlatformProperty>` with `OnceLock<FxHashMap>` cache built by iterating `all_global_properties()` and keying via `MdoType::from_plural`. No runtime warning. Utility for Phase D hover consumers; not used by `complete_mdo_plurals` in Phase C.
- `crates/ide/src/completion/bsl_completion.rs` — rewrite `complete_mdo_plurals` to iterate `PlatformDataInner::instance().all_global_properties()` directly; partition each property via `MdoType::from_plural(prop.name).or_else(|| from_plural(prop.english_name))`. Add `render_mdo_plural_with_hbk` helper. **Delete** `mdo_type_plural_ru` and `mdo_type_plural_en` lookup tables (lines 404-453) — `Cube`, `DimensionTable`, `CommonModule` are dropped from top-level emission since HBK doesn't classify them as global-context properties.

Read-only (do not modify in this PR):
- `crates/ide/src/hover.rs` — hover enrichment is Phase D, separate PR.

Read-only references (do not modify):
- `crates/bsl-platform/src/types.rs` — `PlatformProperty` struct fields: `id`, `type_name`, `name`, `english_name`, `property_types`, `is_readonly`, `min_version`, `context`. Documentation lives separately in `PropertyDocs` and is fetched via `PlatformDataInner::get_property_docs(prop.id)`, not as a field on `PlatformProperty`.
- `crates/bsl-platform/src/db.rs::all_global_properties` and `get_global_property` — primary HBK iteration API.
- `crates/bsl-metadata/src/metadata_object.rs:202-205` — `MdoType::from_plural` (case-insensitive RU+EN).
- `crates/bsl-metadata/src/metadata_object.rs:164-179` — `MdoType::all()` returns `&'static [MdoType]`.
- `crates/hir-ty/src/infer.rs:1315-1503` — bare-ident resolution cascade (do not change).
- `crates/ide/src/hover.rs::append_availability` (~line 697) — existing context-rendering helper, reuse it.
- `crates/ide/src/hover.rs::render_property_hover` (~line 503) — pattern reference for HBK markup composition (do not re-call directly for MDO plurals; the Ty discrimination matters).

## Tests (mandatory by CLAUDE.md)

New test file `crates/ide/tests/completion_mdo_plurals_hbk.rs`:

1. `completion_mdo_plural_label_from_hbk` — `Док|` shows `Документы`; label = HBK property's `name`. Proves the source migration (not just casing).
2. `completion_mdo_plural_bilingual_english_from_hbk` — `Doc|` finds `Документы` via HBK `english_name` ("Documents"); filter_text contains both RU+EN.
3. `completion_mdo_plural_readonly_marker_in_detail` — detail contains `[Только чтение]` (or the locale-aware variant). HBK's `is_readonly` must surface, distinguishing the HBK path from the hardcoded path.
4. `completion_mdo_plural_documentation_from_hbk` — `documentation` contains a non-empty substring from HBK `get_property_docs(prop.id).description` (probe HBK directly in the test to pick the expected substring; skip-guard if doc empty).
5. `completion_mdo_plural_kind_remains_mdo_type` — regression pin: kind is `CompletionItemKind::MdoType`, NOT `Property`. Workspace shape preserved.
6. `completion_mdo_plural_detail_keeps_legacy_prefix` — strict pin against the existing assertion in `completion_globals.rs:184-198`: detail must `starts_with("Коллекция метаданных")`. Catches accidental rename to e.g. "HBK Property".
7. `completion_mdo_plural_common_module_not_emitted_top_level` — `ОбщиеМо|` does NOT emit `ОбщиеМодули` from band `20_`. `CommonModule` is a type descriptor in HBK, not a global-context property. Workspace CommonModule items (band `30_`, e.g. `ОбщегоНазначения`) are unaffected and still emit. Reflective access via `Метаданные.ОбщиеМодули` continues to work through dot-completion.
8. `completion_mdo_plural_cube_not_emitted_top_level` — `Куб|` does NOT emit `Кубы` at top level. Same rationale: HBK has `Куб` as type descriptor nested under `ExternalDataSourceManager`, not as a global-context property.
9. `completion_mdo_plural_dimension_table_not_emitted_top_level` — `ТаблицыИ|` does NOT emit `ТаблицыИзмерения` at top level. HBK has `ТаблицаИзмерения` nested under `ExternalDataSourceCubeManager`.
10. `mdo_plural_completion_set_matches_frozen_baseline` — integration test in `crates/ide/tests/completion_mdo_plurals_hbk.rs`. Compares emitted MDO plural labels against a **frozen-baseline file** committed alongside the test (`crates/ide/tests/fixtures/expected_mdo_plurals.txt`, one label per line, RU). Construction:

   ```rust
   let emitted: BTreeSet<String> = complete_mdo_plurals("")
       .into_iter()
       .map(|item| item.label)
       .collect();
   let baseline_txt = include_str!("fixtures/expected_mdo_plurals.txt");
   let expected: BTreeSet<String> = baseline_txt
       .lines()
       .map(|line| line.trim())
       .filter(|line| !line.is_empty() && !line.starts_with('#'))
       .map(String::from)
       .collect();
   assert_eq!(emitted, expected);
   ```

   Why a frozen baseline rather than a HBK-derived expected set: both
   `emitted` and any HBK-derived `expected` would route through
   `MdoType::from_plural`, making the comparison tautological — adding a
   new HBK MDO plural the enum doesn't know about would be absent from
   both sides and the test would silently pass. The frozen baseline is
   **independent of the filter**. The pin catches regressions in
   already-known plurals only. New MDO collections added to the platform
   will silently pass this test until the baseline file is manually
   updated per `crates/bsl-platform/data/PROVENANCE.md`.

   Subsidiary assertions for the explicit non-emit pin:
   - `assert!(!emitted.contains("Кубы"));`
   - `assert!(!emitted.contains("ТаблицыИзмерения"));`
   - `assert!(!emitted.contains("ОбщиеМодули"));`

   Baseline file contents (commit-time snapshot, ≈17 lines): one entry
   per global-context MDO plural shipped by HBK at the time of Phase C
   landing — exact list determined at implementation by probing the
   build-time `platform_data.json`. Add a header comment explaining
   the regen procedure.

Regression sweep:
- `cargo test --workspace`
- The existing `test_complete_mdo_plural_forms` and
  `test_complete_mdo_symbols_bilingual` in `bsl_completion.rs::tests` must
  still pass — both call `complete_mdo_plurals(prefix)` which is the
  refactored API. They check label + kind, not detail format, so the new
  HBK-detail format is compatible (`bsl_completion.rs:1028-1104`).
- `completion_globals.rs::completion_mdo_plural_not_duplicated` —
  asserts `detail.starts_with("Коллекция метаданных")`. Pin #6 above
  duplicates this, intentionally — guards both surfaces.

Out of test scope (covered by Phase D):
- Hover MDO-plural metadata pin.

## Verification

```bash
# New tests
cargo test -p ide --test completion_mdo_plurals_hbk

# Regression sweep
cargo test --workspace
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

Manual smoke (LSP on real configuration, e.g. `niagara_ut`):
- `Док<Ctrl-Space>` → `Документы` visible, documentation panel shows HBK
  description ("Используется для доступа..." or similar from
  `get_property_docs`).
- `Обр<Ctrl-Space>` → `Обработки` visible with same shape.
- `Общ<Ctrl-Space>` → workspace CommonModules visible (band `30_`, e.g.
  `ОбщегоНазначения`); `ОбщиеМодули` (the metadata-type descriptor) is
  NOT visible at top level. Reflective access via `Метаданные.ОбщиеМодули`
  continues to work through dot-completion.
- `Куб<Ctrl-Space>` and `ТаблицыИ<Ctrl-Space>` → nothing MDO-pluralish
  shown. `Кубы` / `ТаблицыИзмерения` are not bareword-accessible in BSL.
- Hover on `Документы` → unchanged from Phase B (workspace
  `ManagerCollection` shape only; HBK enrichment is Phase D).

## Out of scope (separate PRs)

- **Phase D — hover enrichment for MDO plurals.** Surface HBK
  readonly/min_version/availability/description on hover for
  `Ty::ManagerCollection(MdoType)`. Inverts existing test
  `hover_bare_mdo_plural_keeps_manager_collection`
  (`hover_globals.rs:90-107`); needs hover-only branching in
  `hover_free_name` (NOT in `ty_info_markup`, which has 5 call sites:
  `hover.rs:141, :244, :500, :509, :518`).
- **E1**: Dot-receiver CM-shadow gate (`platform_completion.rs:62, :106`)
  uses raw `module_index.resolve_common_module` instead of
  `Resolver::with_workspace_scope`. Tracked in
  `project_dot_receiver_cm_shadow_followup.md`.
- **E2**: ThisObject implicit-member band in `complete_top_level` (gap for
  `infer.rs` steps 5c/5d). Tracked in
  `project_completion_thisobject_band_followup.md`.
- **E3**: Ref-view readonly uplift and union-type uplift
  (Phase A follow-up B1).
- **E4**: Drop `MdoType` enum entirely. Unlikely — Rust types need
  compile-time discriminators; this is the irreducible hardcode.

## Pair-mode workflow

Per `CLAUDE.md` Codex pair-mode rule:

1. Claude reads this plan and starts implementation.
2. Before claiming completion, Claude runs `/codex:review` on the diff.
3. Address Codex findings, iterate.
4. Stop-time gate: full `cargo test --workspace` + `cargo clippy
   --all-targets --all-features -- -D warnings` + `cargo fmt --check` clean,
   manual smoke confirmed by user.
5. Version bump (current at compaction time: check `Cargo.toml`
   `[workspace.package].version`), commit, push **origin only** (no
   github mirror — see `feedback_push_origin_only.md`).

The plan was not yet reviewed by Codex at writing time. First action in
implementation session: feed this plan to `codex:codex-rescue` for
adversarial review, integrate findings, then code.

## Lookups for future-Claude

If context was compacted and you need orientation:

- Workspace root: `/home/itrous/src/tools_migration/lsp/bsl-analyzer`.
- Project guidance: `CLAUDE.md` at workspace root.
- Memory index: `/home/itrous/.claude/projects/-home-itrous-src-tools-migration-lsp-bsl-analyzer/memory/MEMORY.md`.
- Related memories:
  - `project_dot_receiver_cm_shadow_followup.md`
  - `project_completion_thisobject_band_followup.md`
  - `project_type_system_design.md`
  - `project_type_inference_state.md`
- Phase A field-enum code: `crates/hir-ty/src/field_enum.rs::push_platform_prefix_properties` (this is the model pattern for HBK-property-on-workspace-Ty composition).
- Phase B completion entry: `crates/ide/src/completion/bsl_completion.rs::complete_top_level` (bands `00_`/`10_`/`15_`/`20_`/`25_`/`30_`).
- Phase B hover gate: `crates/ide/src/hover.rs::hover_for_global_property` (inferred_ty equality via `TyLoweringContext::lower_bare_name`).
