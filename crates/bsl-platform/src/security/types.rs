//! Type definitions for the curated security-API registry.
//!
//! These types describe a hand-maintained catalogue of platform APIs that
//! security diagnostics flag as hotspots. The catalogue is **not** derived
//! from HBK syntax-help dumps — `Category`, `Severity`, and per-parameter
//! [`Role`] are design choices that classify how an API is dangerous and
//! how dataflow should treat its arguments.
//!
//! Layer note: this module lives in `bsl-platform` (a leaf crate) so that
//! `dataflow` and `ide-diagnostics` can both consult the same catalogue
//! without re-introducing a `hir-def` dependency. Lookups are `&str`-keyed
//! to avoid taking on `hir-def`'s `Name` type.

/// High-level danger category. One handler typically owns one category.
///
/// Categories drive the choice of diagnostic emitted at a call site and let
/// dataflow `EffectSummary` aggregate effect-bits per category instead of
/// per individual API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    /// File system: read/write/move/delete files and directories.
    FileSystem,
    /// Outbound network: HTTP, FTP, mail, web-services.
    Internet,
    /// Launching external processes / shell commands.
    ExternalApp,
    /// Reading the OS user list.
    OsUsers,
    /// Dynamic execution of BSL strings (`Выполнить`, `Вычислить`).
    ExecuteExternalCode,
    /// Privileged-mode lifetime control (`УстановитьПривилегированныйРежим`).
    PrivilegedMode,
    /// Safe-mode lifetime control (`УстановитьБезопасныйРежим`,
    /// `УстановитьОтключениеБезопасногоРежима`).
    SafeMode,
    /// Querying the safe-mode flag (`БезопасныйРежим()`) — separate from
    /// [`SafeMode`] because the diagnostic checks call shape, not lifetime.
    SafeModeQuery,
    /// Querying the privileged-mode flag (`ПривилегированныйРежим()`).
    /// Symmetric with `SafeModeQuery`; used by consumers that want to
    /// detect a guard like `Если ПривилегированныйРежим() Тогда …`.
    PrivilegedModeQuery,
    /// Logging APIs. Used by `MissingCodeTryCatchEx` to recognize
    /// `LogsOnly` catch-bodies in §2.
    Logging,
    /// Transaction-rollback APIs (`ОтменитьТранзакцию` /
    /// `RollbackTransaction`). The §2 catch-body classifier treats
    /// rollback as a legitimate recovery action — a catch body
    /// containing only a rollback call is not silently swallowing the
    /// exception, it's reverting state before propagating or
    /// otherwise handling the failure.
    Transaction,
}

/// Curated severity. Reflects the *intrinsic* risk of the API, not the
/// computed severity of any particular call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Severity {
    /// Code-execution / privilege escalation. Always reviewed.
    Critical,
    /// Resource exposure (FS / network / external process). Reviewed in
    /// security-audit profiles.
    Major,
    /// Informational hotspot, e.g. logging APIs that the catch-body
    /// classifier needs to recognize.
    Minor,
}

/// Semantic role of a positional parameter. Used by future taint
/// analysis (`Role::Path` / `Role::Url`) and by the privileged/safe-mode
/// counter lattice (`Role::ModeBool`, see §1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Filesystem path argument. Future taint sink target.
    Path,
    /// URL or host:port argument.
    Url,
    /// Shell command-line argument.
    Cmd,
    /// Boolean toggle whose value opens or closes a privileged/unsafe
    /// frame. The polarity is captured in `opens_unsafe_when`:
    ///
    /// - `SetPrivilegedMode(True)` opens privilege → `opens_unsafe_when: true`
    /// - `SetSafeMode(False)` opens unsafe state → `opens_unsafe_when: false`
    /// - `SetSafeModeDisabled(True)` opens unsafe state → `opens_unsafe_when: true`
    ///
    /// `dataflow::security_state` uses this to drive the saturating
    /// counter: each call where the argument constant-folds to the
    /// `opens_unsafe_when` value is an `inc`, the opposite value is a
    /// `dec`, and unknown arguments fork to may/must.
    ModeBool { opens_unsafe_when: bool },
    /// Catch-all for parameters whose role is not yet modelled.
    Other,
}

/// Lifetime marker for paired begin/end APIs. Today only used by future
/// trust-boundary analysis; current entries leave `lifetime = None`
/// because privileged/safe-mode use a single self-toggling method whose
/// direction is encoded in the `Role::ModeBool` parameter, not in two
/// separate begin/end methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lifetime {
    /// Opens a privileged / unsafe scope.
    Begin,
    /// Closes a previously-opened scope.
    End,
}

/// Where the API surface appears in BSL syntax. Determines which
/// recognizer in the lowering / handler layer consults this entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// Global function call: `ОткрытьФайл(...)` / `OpenFile(...)`.
    GlobalMethod,
    /// `Новый <Type>(...)` / `New <Type>(...)`. The type name is encoded
    /// in `SecurityEntry::{ru, en}`; this variant is structural.
    Constructor,
}

/// Per-parameter role attached to a `SecurityEntry`. `index` is the
/// **positional** argument index (0-based) regardless of named-argument
/// syntax — BSL has no real keyword arguments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamRole {
    pub index: u8,
    pub role: Role,
}

/// A single curated entry. Bilingual: `ru` / `en` are matched
/// case-insensitively in the public lookup API. An empty string in `en`
/// signals that the API has no English alias — the bilingual index will
/// skip the empty side rather than collide with other empty-keyed
/// entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityEntry {
    /// Russian name (canonical spelling, case-preserved for diagnostics
    /// and hover text). Lookup keys are derived by lowercasing.
    pub ru: &'static str,
    /// English name. Empty `""` means the API is RU-only in BSL; the
    /// lookup API skips empty keys.
    pub en: &'static str,
    pub kind: EntryKind,
    pub category: Category,
    pub severity: Severity,
    /// Parameter roles. Empty when no positional argument is interesting.
    pub params: &'static [ParamRole],
    pub lifetime: Option<Lifetime>,
}
