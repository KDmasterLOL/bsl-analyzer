mod code;
mod config;
mod context;
pub mod docs;
mod effective;
mod hir_dispatch;
mod hir_inference_dispatch;
mod metadata;
mod metadata_dispatch;
mod query;
mod runner;
mod scope_gate;
mod single_pass;
mod suppression;
mod types;

pub mod common_module_helpers;
pub mod handlers;
pub mod sdbl_utils;
pub mod utils;

#[cfg(test)]
pub mod test_utils;

pub use code::DiagnosticCode;
pub use config::{DiagnosticsConfig, EffectiveMetadata, MetadataOverride};
pub use context::DiagnosticsContext;
pub use handlers::get_metadata;
pub use metadata::{
    CleanCodeAttribute, DiagnosticCompatibilityMode, DiagnosticMetadata, DiagnosticScope,
    DiagnosticSeverityLevel, DiagnosticType, Impact, ImpactSeverity, MetadataTag, SoftwareQuality,
};
pub use query::file_diagnostics_query;
pub use types::{Diagnostic, DiagnosticOutput, DiagnosticTag, Fix, Severity, TextEdit};

pub fn all_diagnostic_codes() -> impl Iterator<Item = DiagnosticCode> {
    use strum::IntoEnumIterator;
    DiagnosticCode::iter()
}

pub fn simple_hir_diagnostic(
    code: DiagnosticCode,
    message: impl Into<String>,
    range: ide_db::TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    if ctx.is_disabled_with_metadata(code) {
        return None;
    }
    Some(Diagnostic {
        code,
        message: message.into(),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

use hir_dispatch::collect_hir_diagnostics;
use hir_inference_dispatch::{collect_arg_diagnostics, collect_inference_diagnostics};
use metadata_dispatch::collect_metadata_diagnostics;
use runner::{
    collect_configuration_diagnostics, collect_dataflow_diagnostics, collect_item_tree_diagnostics,
    collect_line_diagnostics, collect_module_bodies_diagnostics, collect_sdbl_hir_diagnostics,
    collect_syntax_diagnostics,
};

/// Every collector's findings for one file, ordered and deduplicated — but NOT yet
/// the file's final answer.
///
/// Two later stages can still remove findings from this list, and both are owned by
/// [`apply_extension_merge`]: a paired base module can overturn the base-sensitive
/// layer wholesale, and one finding can supersede another. Applying supersession here
/// would be irreversible in exactly the case where the merge revises the winner — the
/// loser is already gone and nothing can bring it back — so the pipeline exit does it.
pub fn diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut result = Vec::new();

    result.extend(safe_collect("line", || collect_line_diagnostics(ctx)));

    result.extend(safe_collect("syntax", || collect_syntax_diagnostics(ctx)));

    result.extend(safe_collect("item_tree", || collect_item_tree_diagnostics(ctx)));
    result.extend(safe_collect("module_bodies", || collect_module_bodies_diagnostics(ctx)));

    result.extend(safe_collect("configuration", || collect_configuration_diagnostics(ctx)));

    result.extend(safe_collect("sdbl_hir", || collect_sdbl_hir_diagnostics(ctx)));

    result.extend(safe_collect("hir", || collect_hir_diagnostics(ctx)));

    result.extend(safe_collect("hir_inference", || collect_inference_diagnostics(ctx)));

    result.extend(safe_collect("hir_arg_inference", || collect_arg_diagnostics(ctx)));

    result.extend(safe_collect("dataflow", || collect_dataflow_diagnostics(ctx)));

    result.extend(safe_collect("metadata", || collect_metadata_diagnostics(ctx)));

    normalize_diagnostics(&mut result);

    result
}

/// File diagnostics with `&ИзменениеИКонтроль` extension merging applied.
///
/// Runs the ordinary standalone pass, then — only for an extension module that pairs to a
/// base with a usable change-and-validate splice — adds the inference diagnostics computed
/// against the spliced effective module, remapped to the `#Вставка` ranges the author
/// wrote. Copied-base inference false positives inside change-and-validate bodies (which
/// reference base-module siblings absent from the standalone ext file) are suppressed.
///
/// For every ordinary module — and every extension file without a usable change — this is
/// byte-identical to the standalone `diagnostics` pass (`effective_target` returns `None`).
pub fn file_diagnostics(
    db: &dyn ide_db::RootDatabase,
    file_id: vfs::FileId,
    config: &DiagnosticsConfig,
) -> Vec<Diagnostic> {
    if !scope_gate::file_in_scope(db, None, file_id, config) {
        return Vec::new();
    }
    let config_path_input = ide_db::configuration_path_for_file(db, file_id);
    let provider = ide_db::SalsaProvider::new(db, config_path_input);
    let standalone = diagnostics(&DiagnosticsContext::new(config, file_id, &provider));
    apply_extension_merge(db, file_id, config, config_path_input, None, standalone)
}

/// Augment a file's already-computed standalone diagnostics with the configuration-extension
/// merge, when `file_id` is an extension module paired to a base. Two complementary base-aware
/// passes supersede the standalone (base-blind) inference diagnostics: the *weaving* pass
/// re-infers the extension's own bodies with the base module as a same-module sibling fallback
/// (`&Вместо`/`&Перед`/`&После` interceptors and helpers), and the *effective* pass splices
/// `&ИзменениеИКонтроль` `#Вставка` code into the base and remaps its diagnostics back. For
/// every other file this returns `standalone` unchanged.
///
/// Factored out of [`file_diagnostics`] so the batch CLI (which builds its provider
/// `with_file_set` and a run-global configuration path) can reuse the exact same merge while
/// keeping its own standalone provider construction — pass the same `config_path_input` and
/// `file_set` the standalone pass used so the effective pass resolves metadata/cross-module
/// context identically.
pub fn apply_extension_merge<'db>(
    db: &'db dyn ide_db::RootDatabase,
    file_id: vfs::FileId,
    config: &DiagnosticsConfig,
    config_path_input: Option<ide_db::metadata::ConfigurationPathInput<'db>>,
    file_set: Option<&'db vfs::file_set::FileSet>,
    mut standalone: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    let weaving = ide_db::weaving_target(db, file_id);
    let effective = ide_db::effective_target(db, file_id)
        .and_then(|eid| hir::effective_module_text(db, eid).map(|effmod| (eid, effmod)));

    // Ordinary module (and any extension file with no resolvable base): byte-identical to the
    // standalone pass, minus any in-code suppression directives.
    if weaving.is_none() && effective.is_none() {
        supersede_dominated(&mut standalone);
        suppression::apply(db, file_id, config, &mut standalone);
        scope_gate::apply(db, file_set, file_id, config, &mut standalone);
        normalize_diagnostics(&mut standalone);
        return standalone;
    }

    let root = db.parse(file_id).syntax_node();
    // `&ИзменениеИКонтроль` method bodies: copied-base statements reference base siblings;
    // owned by the effective pass (remapped to `#Вставка`), so excluded from the weaving pass.
    let cav_bodies = effective::cav_body_ranges(&root);

    // Strip the standalone BASE-SENSITIVE diagnostics by IDENTITY, recomputed on the same
    // provider the caller used. Removing by identity — not by `DiagnosticCode` — is essential:
    // some codes (e.g. `RedundantAccessToObject`, emitted both by the syntactic `ЭтотОбъект.X`
    // lowering check AND by the inference two-level check) also reach this list from non-
    // base-sensitive collectors, and those must survive. The base-aware passes below republish
    // exactly the layer stripped here.
    let std_provider = ide_db::SalsaProvider::with_file_set(db, config_path_input, file_set);
    let std_ctx = DiagnosticsContext::new(config, file_id, &std_provider);
    let mut std_base_sensitive =
        safe_collect("merge:standalone_base_sensitive", || collect_base_sensitive(&std_ctx));
    standalone.retain(|d| match std_base_sensitive.iter().position(|s| s == d) {
        Some(pos) => {
            std_base_sensitive.swap_remove(pos);
            false
        }
        None => true,
    });

    // Weaving pass: re-infer the extension's own bodies with the paired base module as a
    // same-module sibling fallback. `infer_weaving` is equivalent to standalone inference
    // except where a base sibling resolves — `weaving_base` feeds only purely-additive
    // resolution sites (resolver `base_fallback` + the bare-call site), so every difference is
    // a legitimate base-sibling resolution and its downstream type cascade, never a spurious
    // one. Adopt it everywhere except change-and-validate bodies (the effective pass owns
    // those). Only base-sensitive collectors run (they read `infer` + `module_bodies`, both
    // ext-native here → one coherent source map).
    if let Some(wid) = weaving {
        let w_provider = ide_db::SalsaProvider::with_file_set(db, config_path_input, file_set)
            .with_weaving(wid, file_id);
        let w_ctx = DiagnosticsContext::new(config, file_id, &w_provider);
        let mut w_base_sensitive =
            safe_collect("merge:weaving_base_sensitive", || collect_base_sensitive(&w_ctx));
        w_base_sensitive.retain(|d| !effective::range_inside_any(d.range, &cav_bodies));
        standalone.extend(w_base_sensitive);

        // Structural applicability check: every `&Вместо`/`&Перед`/`&После` interceptor must
        // declare the same signature as the base method it weaves onto. Independent of the
        // overlay resolver — compares the extension's own symbols against the base module's.
        if config.any_enabled(runner::WEAVING_DIAGNOSTICS) {
            let base_module = hir::ModuleId::new(wid.base_file(db));
            let base_symbols = std_ctx.symbol_tree_for(base_module);
            standalone.extend(safe_collect("merge:weaving_signature", || {
                handlers::weaving_signature_mismatch::check(&std_ctx, &base_symbols)
            }));
            standalone.extend(safe_collect("merge:weaving_annotation", || {
                handlers::weaving_annotation_not_applicable::check(&std_ctx, &base_symbols)
            }));
        }
    }

    // Effective pass: the `&ИзменениеИКонтроль` `#Вставка` code spliced into the base module,
    // its diagnostics remapped from effective-text coordinates back to the extension source.
    if let Some((eid, effmod)) = effective {
        let eff_provider = ide_db::SalsaProvider::with_file_set(db, config_path_input, file_set)
            .with_effective(eid, file_id);
        let eff_ctx = DiagnosticsContext::new(config, file_id, &eff_provider);
        let eff_base_sensitive =
            safe_collect("merge:effective_base_sensitive", || collect_base_sensitive(&eff_ctx));
        standalone.extend(effective::remap_inserted(eff_base_sensitive, &effmod.segments));
    }

    supersede_dominated(&mut standalone);
    suppression::apply(db, file_id, config, &mut standalone);
    scope_gate::apply(db, file_set, file_id, config, &mut standalone);
    normalize_diagnostics(&mut standalone);
    standalone
}

/// The layer a paired base module can overturn, and therefore the layer every
/// base-aware pass recomputes in full.
///
/// Inference is the obvious member. The dead-store check joins it not because it is
/// an inference diagnostic, but because it asks inference the one question the base
/// changes the answer to: whether a write to a metadata-collection name declares a
/// local at all, or is refused as a write to the Global context. A collector that
/// starts consulting `ctx.infer()` belongs here — one that does not must stay out,
/// or the merge would strip a finding no base-aware pass republishes.
fn collect_base_sensitive(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let mut result = collect_inference_diagnostics(ctx);
    result.extend(handlers::unused_local_variable::check(ctx));
    result
}

fn safe_collect(name: &str, f: impl FnOnce() -> Vec<Diagnostic>) -> Vec<Diagnostic> {
    let start = std::time::Instant::now();
    tracing::debug!(collector = name, "collector started");
    let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(diags) => diags,
        Err(e) => {
            if e.is::<salsa::Cancelled>() {
                std::panic::resume_unwind(e);
            }
            let msg = if let Some(s) = e.downcast_ref::<&'static str>() {
                (*s).to_owned()
            } else if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else {
                format!("<non-string panic payload: {e:?}>")
            };
            tracing::warn!(collector = name, panic = %msg, "collector panicked");
            Vec::new()
        }
    };
    let elapsed = start.elapsed();
    if elapsed.as_millis() > 100 {
        tracing::info!(
            collector = name,
            elapsed_ms = elapsed.as_millis() as u64,
            diags = result.len(),
            "Slow collector"
        );
    }
    result
}

/// Deduplicates dominated findings and puts the result into a canonical total order.
///
/// Several handlers group findings in hash collections whose iteration order varies
/// run to run; sorting once here makes every consumer (LSP publish, CLI reporters,
/// SARIF baselines compared byte-for-byte) deterministic without each handler having
/// to enforce its own emit order.
/// Pairs `(superseded, by)`: the second finding is the precise account of the same
/// statement, so the first — reported by a syntactic check that cannot know better —
/// is dropped when it spans the winner.
///
/// `Справочники = Справочники` is refused by the platform outright, and saying in the
/// same breath that a VARIABLE is assigned to itself contradicts the finding that no
/// variable is created at all.
const SUPERSEDED_ON_THE_SAME_STATEMENT: &[(DiagnosticCode, DiagnosticCode)] =
    &[(DiagnosticCode::SelfAssign, DiagnosticCode::GlobalPropertyNotWritable)];

/// Drop findings that a more precise finding on the same statement supersedes.
///
/// Runs at the pipeline exit and nowhere else: the winner may be a base-sensitive
/// diagnostic that a paired base module later withdraws, and a loser dropped before
/// that withdrawal can never be recovered — nothing recomputes the syntactic layer.
fn supersede_dominated(diagnostics: &mut Vec<Diagnostic>) {
    for (superseded, by) in SUPERSEDED_ON_THE_SAME_STATEMENT {
        let winners: Vec<_> =
            diagnostics.iter().filter(|d| d.code == *by).map(|d| d.range).collect();
        if winners.is_empty() {
            continue;
        }
        diagnostics.retain(|d| {
            d.code != *superseded || !winners.iter().any(|w| d.range.contains_range(*w))
        });
    }
}

fn normalize_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    let dedupe_codes = [DiagnosticCode::UnreachableCode];

    let (mut to_dedupe, mut keep): (Vec<_>, Vec<_>) =
        diagnostics.drain(..).partition(|d| dedupe_codes.contains(&d.code));

    if !to_dedupe.is_empty() {
        to_dedupe.sort_by(|a, b| {
            a.range.start().cmp(&b.range.start()).then_with(|| b.range.len().cmp(&a.range.len()))
        });

        let mut deduped: Vec<Diagnostic> = Vec::with_capacity(to_dedupe.len());
        for diag in to_dedupe {
            let dominated = deduped.iter().any(|existing| {
                existing.range.contains_range(diag.range)
                    || (existing.range.start() == diag.range.start()
                        || existing.range.end() == diag.range.end())
                        && ranges_overlap(existing.range, diag.range)
            });
            if !dominated {
                deduped.push(diag);
            }
        }
        keep.extend(deduped);
    }

    keep.sort_by(|a, b| {
        (a.range.start(), a.range.end(), a.code.as_str(), &a.message).cmp(&(
            b.range.start(),
            b.range.end(),
            b.code.as_str(),
            &b.message,
        ))
    });

    *diagnostics = keep;
}

fn ranges_overlap(a: ide_db::TextRange, b: ide_db::TextRange) -> bool {
    a.start() < b.end() && b.start() < a.end()
}

#[cfg(test)]
mod normalize_tests {
    use super::*;

    fn diag(code: DiagnosticCode, start: u32, end: u32, message: &str) -> Diagnostic {
        Diagnostic {
            code,
            message: message.to_string(),
            severity: Severity::Information,
            range: ide_db::TextRange::new(start.into(), end.into()),
            tags: vec![],
            fixes: vec![],
        }
    }

    #[test]
    fn normalize_orders_by_range_then_code_then_message() {
        let mut diagnostics = vec![
            diag(DiagnosticCode::LineLength, 10, 20, "b"),
            diag(DiagnosticCode::LineLength, 0, 5, "z"),
            diag(DiagnosticCode::MagicNumber, 10, 20, "a"),
            diag(DiagnosticCode::LineLength, 10, 20, "a"),
        ];

        normalize_diagnostics(&mut diagnostics);

        let keys: Vec<_> = diagnostics
            .iter()
            .map(|d| (u32::from(d.range.start()), d.code.as_str(), d.message.as_str()))
            .collect();
        assert_eq!(
            keys,
            vec![
                (0, "LineLength", "z"),
                (10, "LineLength", "a"),
                (10, "LineLength", "b"),
                (10, "MagicNumber", "a"),
            ]
        );
    }

    /// Точный вердикт вытесняет общий на том же операторе, но только когда он
    /// действительно внутри: соседнее самоприсваивание остаётся.
    #[test]
    fn a_refused_write_supersedes_the_self_assign_on_the_same_statement() {
        let mut diagnostics = vec![
            diag(DiagnosticCode::SelfAssign, 10, 30, "self"),
            diag(DiagnosticCode::GlobalPropertyNotWritable, 10, 21, "write"),
            diag(DiagnosticCode::SelfAssign, 40, 50, "elsewhere"),
        ];

        supersede_dominated(&mut diagnostics);
        normalize_diagnostics(&mut diagnostics);

        let keys: Vec<_> =
            diagnostics.iter().map(|d| (u32::from(d.range.start()), d.code.as_str())).collect();
        assert_eq!(
            keys,
            vec![(10, "GlobalPropertyNotWritable"), (40, "SelfAssign")],
            "only the self-assign spanning the refused write is dropped"
        );
    }

    #[test]
    fn normalize_is_stable_across_input_permutations() {
        let build = || {
            vec![
                diag(DiagnosticCode::MagicNumber, 3, 7, "m"),
                diag(DiagnosticCode::LineLength, 3, 7, "m"),
                diag(DiagnosticCode::LineLength, 1, 2, "x"),
            ]
        };

        let mut forward = build();
        normalize_diagnostics(&mut forward);

        let mut reversed: Vec<_> = build().into_iter().rev().collect();
        normalize_diagnostics(&mut reversed);

        assert_eq!(forward, reversed);
    }
}

/// Ведущий BOM не добавляет и не убирает находок.
///
/// Сверка сквозная: определений тривии много, а BOM — ровно тот вид, который
/// из них выпадал, и место, забывшее его, видит невидимый символ как значимый
/// токен.
///
/// **Чего она НЕ ловит.** К сведению определений тривии к одному на слой она
/// нечувствительна: на этой фикстуре ни один из переведённых наборов состава
/// находок не меняет — разница видна только при включённом исключении
/// хвостовых комментариев, и её сторожит свой тест в `line_length`. Здесь
/// сверка стоит ради НОВЫХ мест: у неё нет списка проверяемых, поэтому она
/// заметит и то, которого ещё нет.
///
/// Сравниваются коды, а не сообщения: BOM занимает позицию в строке, и
/// сообщение о длине строки законно отличается на единицу.
#[cfg(test)]
mod byte_order_mark_tests {
    use crate::test_utils::check_file_diagnostics;

    /// Первая строка — длинный комментарий: под BOM он и различает наборы.
    ///
    /// Место, считающее BOM значимым токеном, метит первую строку как несущую
    /// код, а комментарий на строке с кодом из замера длины исключается — и
    /// находка о длине пропадает целиком. Обычная фикстура этого не
    /// показывает: BOM стоит прямым потомком корня, и обходы внутри узлов до
    /// него не добираются.
    const MODULE: &str = "// оооооооооооооооооооооочень длинный комментарий, который заведомо длиннее любого разумного предела длины строки в модуле\nпроцедура Тест(А, Б) экспорт\n\tЕсли А=1 Тогда\n\t\tВ = А+Б;\n\tКонецЕсли;\nконецпроцедуры\n";

    /// Находка, которую BOM отменяет не через определения тривии.
    ///
    /// Расхождение старше этой сверки: оно живёт в привязке описания метода к
    /// объявлению, а не в наборах видов, и воспроизводится на сборке, где ни
    /// один набор ещё не сведён. Названо здесь, чтобы сверка ловила НОВЫЕ
    /// расхождения, а не молчала обо всех разом; когда привязка починится,
    /// исключение обязано стать лишним — это проверяется отдельно.
    const KNOWN_BOM_SENSITIVE: &str = "MissingParameterDescription";

    fn codes(source: &str) -> Vec<String> {
        let mut found: Vec<String> =
            check_file_diagnostics(source).iter().map(|d| format!("{:?}", d.code)).collect();
        found.sort();
        found
    }

    #[test]
    fn a_leading_byte_order_mark_changes_no_finding() {
        let without = codes(MODULE);
        assert!(!without.is_empty(), "фикстура обязана давать находки, иначе сверка пуста");

        let with_bom = codes(&format!("\u{feff}{MODULE}"));

        let expected: Vec<String> =
            without.iter().filter(|c| *c != KNOWN_BOM_SENSITIVE).cloned().collect();
        assert_eq!(with_bom, expected, "BOM изменил состав находок сверх известного");
    }

    /// Известное отклонение всё ещё существует.
    ///
    /// Без этой проверки исключение выше переживёт починку и будет молча
    /// прикрывать новое расхождение того же кода.
    #[test]
    fn the_known_deviation_is_still_there() {
        assert!(
            codes(MODULE).iter().any(|c| c == KNOWN_BOM_SENSITIVE),
            "фикстура перестала давать находку, ради которой заведено исключение"
        );
        assert!(
            !codes(&format!("\u{feff}{MODULE}")).iter().any(|c| c == KNOWN_BOM_SENSITIVE),
            "отклонение исчезло — исключение в сверке пора убрать"
        );
    }
}
