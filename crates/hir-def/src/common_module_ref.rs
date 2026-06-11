//! Syntactic resolution of `Переменная = ОбщегоНазначения[Клиент].ОбщийМодуль("Имя")`.
//!
//! The call graph (`call_graph`) and type inference (`hir-ty`) both need to attribute
//! `Переменная.Метод(...)` to the common module the variable was bound to. That binding
//! is decidable purely syntactically (a string literal plus the assignment target), with
//! no type information, so it lives here once and is consumed by both — avoiding the
//! parallel resolvers that drift apart.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    body::Body,
    hir::{Expr, ExprIdx, Literal, Stmt},
    name::Name,
};

/// If `value` is `<ОбщегоНазначения|ОбщегоНазначенияКлиент>.ОбщийМодуль("Имя")` with a
/// plain (non-dotted) string-literal argument, return that common-module name. The dotted
/// form `ОбщийМодуль("Справочники.Имя")` (a manager module) is intentionally rejected.
pub fn common_module_call_target(body: &Body, value: ExprIdx) -> Option<Name> {
    let Expr::Call { callee, args } = body.expr_idx(value) else {
        return None;
    };
    let first_arg = *args.first()?;
    let Expr::Field { base, field } = body.expr_idx(*callee) else {
        return None;
    };
    if field.as_str().to_lowercase() != "общиймодуль" {
        return None;
    }
    let Expr::Path(base_name) = body.expr_idx(*base) else {
        return None;
    };
    let base_lower = base_name.as_str().to_lowercase();
    if base_lower != "общегоназначения" && base_lower != "общегоназначенияклиент"
    {
        return None;
    }
    let Expr::Literal(Literal::String(name_lit)) = body.expr_idx(first_arg) else {
        return None;
    };
    if name_lit.contains('.') {
        return None;
    }
    Some(Name::new(name_lit))
}

/// Per-body map `var(lowercased) → common-module name`.
///
/// Soundness is conservative — a variable binds to module `M` only when it unambiguously
/// holds that module wherever it holds a usable value:
/// - **every** `Stmt::Assign` to it resolves to the same `M`; any other assignment
///   (a non-`ОбщийМодуль` right-hand side, or a different module) poisons it;
/// - it is **not a parameter** (a parameter carries an incoming value, so a use before the
///   reassignment, or on a branch where the reassignment did not run, would be wrong);
/// - it is **not a loop variable** (`Для`/`Для Каждого` rebind it to an element/counter
///   outside the `Stmt::Assign` channel).
///
/// The rule is flow-insensitive but the excluded categories remove the cases where a
/// flow-sensitive value would differ from `M`. A pure local that is only ever assigned the
/// same module is `M` whenever it is usable, so substituting `M` at every use is safe.
pub fn common_module_var_bindings(body: &Body) -> FxHashMap<String, Name> {
    enum Binding {
        Module(Name),
        Poisoned,
    }

    // Variable names that carry a value outside the `Stmt::Assign` channel and must never
    // be treated as a bound module: procedure parameters and loop variables.
    let mut excluded: FxHashSet<String> = FxHashSet::default();
    for param in body.params() {
        excluded.insert(body.binding(param).name.as_str().to_lowercase());
    }
    for (_, stmt) in body.stmts_iter() {
        if let Stmt::For { var, .. } | Stmt::ForEach { var, .. } = stmt {
            excluded.insert(body.binding_idx(*var).name.as_str().to_lowercase());
        }
    }

    let mut acc: FxHashMap<String, Binding> = FxHashMap::default();
    for (_, stmt) in body.stmts_iter() {
        let Stmt::Assign { target, value } = stmt else {
            continue;
        };
        let Expr::Path(var) = body.expr_idx(*target) else {
            continue;
        };
        let key = var.as_str().to_lowercase();
        if excluded.contains(&key) {
            continue;
        }
        let resolved = common_module_call_target(body, *value);
        match (acc.get(&key), resolved) {
            (Some(Binding::Poisoned), _) => {}
            (_, None) => {
                acc.insert(key, Binding::Poisoned);
            }
            (None, Some(module)) => {
                acc.insert(key, Binding::Module(module));
            }
            (Some(Binding::Module(prev)), Some(module)) => {
                if prev.as_str().to_lowercase() != module.as_str().to_lowercase() {
                    acc.insert(key, Binding::Poisoned);
                }
            }
        }
    }

    acc.into_iter()
        .filter_map(|(key, binding)| match binding {
            Binding::Module(module) => Some((key, module)),
            Binding::Poisoned => None,
        })
        .collect()
}

/// Rollout gate for the **inference-side diagnostic** only (the call graph consumes the
/// resolver unconditionally, since resolved edges are a pure improvement). Emitting a new
/// `UnresolvedMethodCall` class on every workspace is a user-visible change held behind an
/// env flag until the rollout decision is made.
pub fn diagnostics_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("BSL_COMMON_MODULE_BY_NAME").is_ok_and(|v| !v.is_empty() && v != "0")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bindings(src: &str) -> Vec<(String, String)> {
        let parse = parser::parse(src);
        let module_bodies =
            crate::ModuleBodies::from_parse(&parse, crate::ModuleId::new(vfs::FileId(0)));
        let lower_result = module_bodies.lower_result(0).expect("one method body");
        let mut v: Vec<(String, String)> = common_module_var_bindings(&lower_result.body)
            .into_iter()
            .map(|(k, m)| (k, m.as_str().to_string()))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn binds_single_assignment() {
        let v = bindings(
            "Процедура Тест()\n    М = ОбщегоНазначения.ОбщийМодуль(\"Утилиты\");\n    М.Метод();\nКонецПроцедуры",
        );
        assert_eq!(v, vec![("м".to_string(), "Утилиты".to_string())]);
    }

    #[test]
    fn client_accessor_also_binds() {
        let v = bindings(
            "Процедура Тест()\n    М = ОбщегоНазначенияКлиент.ОбщийМодуль(\"УтилитыКлиент\");\nКонецПроцедуры",
        );
        assert_eq!(v, vec![("м".to_string(), "УтилитыКлиент".to_string())]);
    }

    #[test]
    fn reassignment_to_non_module_poisons() {
        let v = bindings(
            "Процедура Тест()\n    М = ОбщегоНазначения.ОбщийМодуль(\"Утилиты\");\n    М = 1;\nКонецПроцедуры",
        );
        assert!(v.is_empty(), "reassignment to a non-module must drop the binding, got {v:?}");
    }

    #[test]
    fn conflicting_modules_poison() {
        let v = bindings(
            "Процедура Тест()\n    М = ОбщегоНазначения.ОбщийМодуль(\"А\");\n    М = ОбщегоНазначения.ОбщийМодуль(\"Б\");\nКонецПроцедуры",
        );
        assert!(v.is_empty(), "two different modules must drop the binding, got {v:?}");
    }

    #[test]
    fn parameter_receiver_is_not_bound() {
        // A parameter carries an incoming value; a use before the reassignment (or on a
        // branch where it did not run) would be misresolved, so parameters never bind.
        let v = bindings(
            "Процедура Тест(М)\n    М.СтарыйМетод();\n    М = ОбщегоНазначения.ОбщийМодуль(\"Утилиты\");\nКонецПроцедуры",
        );
        assert!(v.is_empty(), "a parameter must never bind to a module, got {v:?}");
    }

    #[test]
    fn conditionally_assigned_parameter_is_not_bound() {
        let v = bindings(
            "Процедура Тест(М, Условие)\n    Если Условие Тогда\n        М = ОбщегоНазначения.ОбщийМодуль(\"Утилиты\");\n    КонецЕсли;\n    М.Метод();\nКонецПроцедуры",
        );
        assert!(v.is_empty(), "a conditionally-assigned parameter must not bind, got {v:?}");
    }

    #[test]
    fn loop_variable_is_not_bound() {
        let v = bindings(
            "Процедура Тест(Коллекция)\n    М = ОбщегоНазначения.ОбщийМодуль(\"Утилиты\");\n    Для Каждого М Из Коллекция Цикл\n        М.Метод();\n    КонецЦикла;\nКонецПроцедуры",
        );
        assert!(v.is_empty(), "a loop variable must not bind to a module, got {v:?}");
    }

    #[test]
    fn dotted_manager_form_is_ignored() {
        let v = bindings(
            "Процедура Тест()\n    М = ОбщегоНазначения.ОбщийМодуль(\"Справочники.Контрагенты\");\nКонецПроцедуры",
        );
        assert!(v.is_empty(), "dotted manager form must not bind, got {v:?}");
    }
}
