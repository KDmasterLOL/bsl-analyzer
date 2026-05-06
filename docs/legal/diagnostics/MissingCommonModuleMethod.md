# Provenance: MissingCommonModuleMethod

> **Deprecated since v0.1.176.** Replaced by `UnresolvedMethodCall`
> (`BSL-TY-UnresolvedMethodCall`). Phase 2 of the qualified-call
> clean-architecture refactor lifted classification into hir-ty
> inference, which has the resolver and the receiver type. The
> diagnostic is no longer emitted; the rule definition stays for
> downstream compatibility (SonarQube rule export, user
> `bsl-analyzer.toml`) until full removal in Phase 4.

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule exists

This diagnostic is a generic semantic correctness rule.

The underlying idea is straightforward: a qualified call of the form
`CommonModule.Method()` is valid only when the referenced common module exposes
such a method as part of its public API. Calling a missing or non-export method
leads to runtime failure.

There is no strong direct normative `v8std` source for this exact rule.
`v8std.ru` itself classifies `MissingCommonModuleMethod` as a diagnostic without
direct standard mapping.

## Audit result

### Production code

Current implementation (Phase 3 of the qualified-call refactor): the
diagnostic is **deprecated and no longer emitted**. The handler in
`crates/ide-diagnostics/src/handlers/missing_common_module_method.rs`
keeps a no-op `from_hir` stub solely so the dispatch round-trip
doesn't panic on a stale `BodyDiagnostic` value. Replacement is
`UnresolvedMethodCall` (`BSL-TY-UnresolvedMethodCall`), emitted by
hir-ty's `dispatch_bare_ident_field_call` cascade gate — the
classification was lifted out of body lowering into type inference
because lowering had no `db` / receiver type and was producing
false-positives for form attributes, implicit form globals,
module-level `Перем` declarations, and platform globals.

The deprecated rule definition (`DiagnosticCode::MissingCommonModuleMethod`,
SonarQube rule entry, user-facing docs, `bsl-analyzer.toml`
acceptance) stays in place so existing downstream profiles keep
validating; full removal lands in Phase 4.

This still favors permissive licensing treatment: the
implementation is local, semantic-analysis based, with no copied
parser or upstream documentation logic — and the active replacement
follows the same architectural pattern.

### Documentation

RU/EN documentation was rewritten during this audit to describe the current
behavior directly instead of reusing placeholder or upstream wording.

### Tests

Current tests are local inline fixtures kept under
`crates/ide-diagnostics/src/handlers/missing_common_module_method.rs::tests`,
migrated to the `UnresolvedMethodCall` channel in Phase 2 — they pin
both directions of the deprecation: the legacy `MissingCommonModuleMethod`
code stays silent, and the active replacement
`UnresolvedMethodCall` fires with the right `kind`
(`MethodNotFound` vs `ReceiverNotResolved`). Two fixtures remain
`#[ignore]`d pending Phase 5 e2e setup
(`crates/ide/tests/diagnostics_form_attribute_call.rs`).

## Remaining caveats

- there is no clean official standard page to cite as a primary normative
  source;
- repository-wide relicensing still depends on the broader audit of shared
  infrastructure.

## Conclusion

`MissingCommonModuleMethod` is a strong permissive candidate because it is a
generic semantic correctness rule with local HIR-based implementation, local
tests, and now-local documentation.
