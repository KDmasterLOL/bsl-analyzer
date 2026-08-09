//! A module whose bytes cannot be read lowers to no methods at all, so a
//! measurement taken over it reports a smaller, faster graph than the workspace
//! really has — the run would look better precisely because it analysed less.
//! These gates pin that such a run fails instead.
//!
//! The stand always corrupts the module AFTER boot. A module unreadable from the
//! start never enters the source root at all (it is built from decoded bytes), so
//! corrupting it earlier would measure a silently smaller universe rather than the
//! refusal.

use std::path::{Path, PathBuf};

use super::{boot, execute_once, resolve_target, BenchEnv, BetweenIndexPassesHook, ResolvedTarget};
use crate::bench::manifest::FeatureSpec;
use crate::bench::runner::RunError;

const FIXTURE: &str = "\
Процедура Внутренняя(Знач Пар1, Пар2 = 0)
    Локальная = Пар1 + Пар2;
КонецПроцедуры

Процедура БенчЭкспортная() Экспорт
    Внутренняя(1, 2);
    Внутренняя(3, 4);
КонецПроцедуры
";

fn index_build_spec() -> FeatureSpec {
    FeatureSpec::CallHierarchyIndexBuild { batch_size: 1 }
}

/// Boots a one-module workspace and resolves the target, which is as far as the
/// stand may go before corrupting anything: `resolve_target` reads the file text,
/// and a closed BSL file is disk-backed, so an unreadable one panics there instead
/// of reaching the measurement.
fn stand() -> (tempfile::TempDir, BenchEnv, ResolvedTarget, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let module = dir.path().join("Module.bsl");
    std::fs::write(&module, FIXTURE).expect("write fixture");
    let env = boot(dir.path(), 60_000).expect("boot the one-module workspace");
    let resolved = resolve_target(&env, "Module.bsl", None).expect("resolve the module");
    (dir, env, resolved, module)
}

/// CP-1251 bytes for `Процедура`: reads go through `fs::read_to_string`, so
/// undecodable bytes fail deterministically — no permissions, no races, and the
/// same failure on every platform.
fn make_unreadable(path: &Path) {
    std::fs::write(path, [0xCF, 0xF0, 0xEE, 0xF6, 0xE5, 0xE4, 0xF3, 0xF0, 0xE0])
        .expect("overwrite the module with undecodable bytes");
}

/// `Observation` is not `Debug`, and deriving it to please `expect_err` would
/// change production code for a test's convenience.
fn expect_refusal(result: Result<(u64, super::Observation), RunError>, why: &str) -> RunError {
    match result {
        Ok((_, observation)) => panic!("{why}; measured {} pair(s) instead", observation.count),
        Err(err) => err,
    }
}

/// Demands the specific refusal, not merely a failed run: the branch has other
/// ways to return `Err`, and a gate that accepts any of them would survive the
/// removal of the very check it exists to hold.
fn assert_unread_failure(err: RunError, module: &Path) {
    let RunError::Other(message) = err else {
        panic!("the unread check reports RunError::Other, got {err:?}");
    };
    assert!(
        message.contains("could not be read"),
        "the refusal must name unreadable bytes as its reason, got: {message}"
    );
    let expected = std::fs::canonicalize(module).unwrap_or_else(|_| module.to_path_buf());
    assert!(
        message.contains(&expected.display().to_string()),
        "the refusal must name the module, got: {message}"
    );
}

/// The seam is an extension point, so a later hook may well want to install or
/// drop a guard of its own. Holding the cell borrowed across the call would kill
/// such a hook with a borrow panic — pinned here so the property stays a property
/// of the seam rather than a rule its writers must remember.
#[test]
fn a_hook_may_touch_the_seam_while_it_runs() {
    let ran_to_completion = std::rc::Rc::new(std::cell::Cell::new(false));
    let flag = std::rc::Rc::clone(&ran_to_completion);
    let _hook = BetweenIndexPassesHook::install(move || {
        let _nested = BetweenIndexPassesHook::install(|| {});
        flag.set(true);
    });

    super::run_between_index_passes_hook();

    assert!(ran_to_completion.get(), "the hook must reach its last statement");
}

#[test]
fn a_readable_workspace_measures_its_only_call_pair() {
    let (_dir, mut env, resolved, _module) = stand();

    let (elapsed_ns, observation) = execute_once(&mut env, &resolved, &index_build_spec())
        .expect("a fully readable workspace must produce a measurement");

    assert!(elapsed_ns > 0, "the build must take non-zero time");
    assert_eq!(observation.count, 1, "the fixture holds exactly one caller pair");
}

#[test]
fn a_module_unreadable_before_the_build_fails_the_measurement() {
    let (_dir, mut env, resolved, module) = stand();
    make_unreadable(&module);

    let err = expect_refusal(
        execute_once(&mut env, &resolved, &index_build_spec()),
        "a measurement over a module nobody could read is not a measurement",
    );

    assert_unread_failure(err, &module);
}

#[test]
fn a_module_unreadable_between_the_two_passes_fails_the_measurement() {
    let (_dir, mut env, resolved, module) = stand();
    // The observation reopens every batch, so a file that survived the build pass
    // still has a second chance to become unreadable; its report must count too.
    let corrupted = module.clone();
    let _hook = BetweenIndexPassesHook::install(move || make_unreadable(&corrupted));

    let err = expect_refusal(
        execute_once(&mut env, &resolved, &index_build_spec()),
        "the observation pass reads the batches again and must be heard",
    );

    assert_unread_failure(err, &module);
}
