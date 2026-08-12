# Configurable MCP Cache Directory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Добавить `bsl-analyzer mcp serve --cache-dir <PATH>`, чтобы все производные файлы workspace можно было вынести из `source-dir` без нарушения broker/daemon reuse и старого поведения по умолчанию.

**Architecture:** `mcp-server::WorkspaceCacheLayout` становится единственным объектом, знающим имена и корневой каталог workspace-кешей. CLI один раз разрешает явный путь, broker включает нормализованный cache root в `BackendKey` и передаёт его daemon; `SharedState`, graph и lease получают один и тот же layout. Существующие публичные/default-конструкторы остаются тонкими обёртками для обратной совместимости тестов и пользователей библиотеки.

**Tech Stack:** Rust 2021, Clap, Tokio, tempfile, rusqlite, существующие crates `bsl-analyzer` и `mcp-server`.

## Global Constraints

- Без `--cache-dir` эффективный путь остаётся `<source-dir>/.build`.
- `--cache-dir` допустим только для `--profile workspace`.
- Относительный явный путь разрешается от текущего рабочего каталога.
- Явный cache dir создаётся и канонизируется до построения server state; ошибка является ошибкой запуска без fallback.
- Cache root входит в broker backend identity и передаётся daemon абсолютным путём.
- `mcp install`, reference search cache, форматы SQLite/lease и миграция старого кеша не изменяются.
- Не добавлять новые зависимости без необходимости.
- Каждый production-путь, выбирающий workspace-кеш, получает `WorkspaceCacheLayout`; `.build` не собирается вручную вне default-конструктора layout.

---

## File Map

- `crates/mcp-server/src/cache.rs` — тип layout, нормализация и имена всех workspace cache-файлов.
- `crates/mcp-server/src/lib.rs` — публичный экспорт `WorkspaceCacheLayout` и сохранение совместимого `graph_db_path`.
- `crates/mcp-server/src/workspace_lease.rs` — lease/lock в эффективном cache root.
- `crates/mcp-server/src/graph/state.rs` — хранение layout в `GraphState`.
- `crates/mcp-server/src/graph/build.rs` — чтение, сборка и публикация graph DB по layout.
- `crates/mcp-server/src/graph/snapshot.rs` — открытие snapshot graph DB по layout.
- `crates/mcp-server/src/state/mod.rs` — хранение layout в workspace `SharedState`.
- `crates/mcp-server/src/state/bootstrap.rs` — инициализация search DB и передача layout фоновым задачам.
- `crates/mcp-server/src/state/embed.rs` — чтение graph DB при обновлении контекста и embeddings.
- `crates/mcp-server/src/broker/name.rs` — cache root как ось `BackendKey`.
- `crates/bsl-analyzer/src/bin/cli/mcp.rs` — CLI, валидация, разрешение пути, broker→daemon propagation, fallback и daemon log.
- `docs/mcp/README.md` — пользовательский контракт и пример.

### Task 1: WorkspaceCacheLayout

**Files:**
- Modify: `crates/mcp-server/src/cache.rs`
- Modify: `crates/mcp-server/src/lib.rs`

**Interfaces:**
- Produces: `WorkspaceCacheLayout::for_workspace(&Path) -> Self`
- Produces: `WorkspaceCacheLayout::from_root(PathBuf) -> Self`
- Produces: `WorkspaceCacheLayout::prepare_explicit(&Path, &Path) -> io::Result<Self>`
- Produces: `root`, `ensure`, `graph_db_path`, `search_db_path`, `lease_path`, `lease_lock_path`, `stall_report_path`, `daemon_log_path`
- Preserves: `graph_db_path(&Path) -> PathBuf` and internal default helper functions as wrappers.

- [ ] **Step 1: Write failing cache layout tests**

Add tests in `crates/mcp-server/src/cache.rs` covering default, external absolute,
relative, Unicode/spaces, all file names, and explicit creation:

```rust
#[test]
fn explicit_relative_cache_is_created_and_canonicalized() {
    let cwd = tempfile::tempdir().unwrap();
    let layout = WorkspaceCacheLayout::prepare_explicit(
        Path::new("кеш с пробелом"),
        cwd.path(),
    )
    .unwrap();

    assert_eq!(layout.root(), cwd.path().join("кеш с пробелом").canonicalize().unwrap());
    assert_eq!(layout.graph_db_path(), layout.root().join("bsl-graph.db"));
    assert_eq!(layout.search_db_path(), layout.root().join("bsl-search.db"));
    assert_eq!(layout.lease_path(), layout.root().join("writer.lease"));
    assert_eq!(layout.lease_lock_path(), layout.root().join("writer.lease.lock"));
    assert_eq!(layout.stall_report_path(), layout.root().join("bsl-graph-stall-report.txt"));
    assert_eq!(layout.daemon_log_path(), layout.root().join("bsl-analyzer-daemon.log"));
}

#[test]
fn default_layout_stays_under_workspace_build() {
    let workspace = tempfile::tempdir().unwrap();
    let layout = WorkspaceCacheLayout::for_workspace(workspace.path());
    assert_eq!(layout.root(), workspace.path().join(".build"));
    assert!(!layout.root().exists(), "default construction stays lazy");
}
```

- [ ] **Step 2: Run tests and verify RED**

Run:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p mcp-server cache::tests -- --nocapture
```

Expected: compile failure because `WorkspaceCacheLayout` is not defined.

- [ ] **Step 3: Implement the layout and compatibility wrappers**

Use this API shape in `cache.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCacheLayout {
    root: PathBuf,
}

impl WorkspaceCacheLayout {
    pub fn for_workspace(workspace_root: &Path) -> Self {
        let root = workspace_root.join(".build");
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        Self { root }
    }

    pub fn from_root(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn prepare_explicit(path: &Path, current_dir: &Path) -> std::io::Result<Self> {
        let requested = if path.is_absolute() { path.to_path_buf() } else { current_dir.join(path) };
        std::fs::create_dir_all(&requested).map_err(|error| {
            std::io::Error::new(error.kind(), format!("failed to create --cache-dir {}: {error}", requested.display()))
        })?;
        let root = requested.canonicalize().map_err(|error| {
            std::io::Error::new(error.kind(), format!("failed to canonicalize --cache-dir {}: {error}", requested.display()))
        })?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path { &self.root }
    pub fn ensure(&self) -> std::io::Result<()> { std::fs::create_dir_all(&self.root) }
    pub fn graph_db_path(&self) -> PathBuf { self.root.join("bsl-graph.db") }
    pub fn search_db_path(&self) -> PathBuf { self.root.join("bsl-search.db") }
    pub fn lease_path(&self) -> PathBuf { self.root.join("writer.lease") }
    pub fn lease_lock_path(&self) -> PathBuf { self.root.join("writer.lease.lock") }
    pub fn stall_report_path(&self) -> PathBuf { self.root.join("bsl-graph-stall-report.txt") }
    pub fn daemon_log_path(&self) -> PathBuf { self.root.join("bsl-analyzer-daemon.log") }
}
```

Export it from `lib.rs`:

```rust
pub use cache::{graph_db_path, WorkspaceCacheLayout};
```

- [ ] **Step 4: Run focused tests and formatting**

Run:

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" fmt --all -- --check
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p mcp-server cache::tests
```

Expected: PASS.

- [ ] **Step 5: Commit Task 1**

```powershell
git add crates/mcp-server/src/cache.rs crates/mcp-server/src/lib.rs
git commit -m "feat(mcp): add workspace cache layout"
```

### Task 2: Cache-aware lease and graph

**Files:**
- Modify: `crates/mcp-server/src/workspace_lease.rs`
- Modify: `crates/mcp-server/src/graph/state.rs`
- Modify: `crates/mcp-server/src/graph/build.rs`
- Modify: `crates/mcp-server/src/graph/snapshot.rs`

**Interfaces:**
- Consumes: `WorkspaceCacheLayout` from Task 1.
- Produces: `WorkspaceLease::claim_cache(&WorkspaceCacheLayout) -> WorkspaceLease`.
- Produces: `GraphState::for_workspace_with_cache(PathBuf, WorkspaceCacheLayout) -> GraphState`.
- Preserves: default `claim(&Path)` and `for_workspace(PathBuf)` wrappers for existing tests.

- [ ] **Step 1: Write failing external-layout graph and lease tests**

Add a lease test proving the record is external:

```rust
#[test]
fn explicit_cache_layout_holds_lease_outside_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let layout = crate::cache::WorkspaceCacheLayout::from_root(cache.path().to_path_buf());

    let lease = WorkspaceLease::claim_cache(&layout);

    assert!(lease.owns_caches());
    assert!(layout.lease_path().exists());
    assert!(!workspace.path().join(".build").exists());
}
```

Add this graph test beside `loads_workspace_and_serves_graph`, reusing the
module's existing `sample_workspace` and `wait_ready` helpers:

```rust
#[test]
fn explicit_cache_layout_builds_graph_outside_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let root = workspace.path();
    sample_workspace(root);
    let layout = crate::cache::WorkspaceCacheLayout::from_root(cache.path().to_path_buf());

    let graph = GraphState::for_workspace_with_cache(root.to_path_buf(), layout.clone());
    graph.ensure_loading();
    wait_ready(&graph);

assert!(layout.graph_db_path().exists());
assert!(!root.join(".build").exists());
}
```

- [ ] **Step 2: Run focused tests and verify RED**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p mcp-server explicit_cache_layout -- --nocapture
```

Expected: compile failure for missing `claim_cache` and `for_workspace_with_cache`.

- [ ] **Step 3: Route lease through layout**

Keep the compatibility wrapper and make production use the new entry point:

```rust
pub(crate) fn claim(workspace_root: &Path) -> Self {
    Self::claim_cache(&crate::cache::WorkspaceCacheLayout::for_workspace(workspace_root))
}

pub(crate) fn claim_cache(cache: &crate::cache::WorkspaceCacheLayout) -> Self {
    match Self::try_claim_cache(cache) {
        Ok(lease) => lease,
        Err(error) => {
            tracing::warn!(error = %error, root = %cache.root().display(), "could not claim the workspace cache lease; this daemon will not coordinate with another generation over the same caches");
            Self::unmanaged()
        }
    }
}
```

Use `cache.ensure()`, `cache.lease_path()` and `cache.lease_lock_path()`; remove
manual lease file constants from path construction while retaining their names
only if tests/doc comments need them.

- [ ] **Step 4: Store layout in GraphState and replace production graph paths**

Add the field and constructors:

```rust
pub(super) cache: Option<crate::cache::WorkspaceCacheLayout>,

pub(crate) fn for_workspace(workspace_root: PathBuf) -> Self {
    let cache = crate::cache::WorkspaceCacheLayout::for_workspace(&workspace_root);
    Self::for_workspace_with_cache(workspace_root, cache)
}

pub(crate) fn for_workspace_with_cache(
    workspace_root: PathBuf,
    cache: crate::cache::WorkspaceCacheLayout,
) -> Self {
    let mut state = Self::with_status(GraphStatus::Idle, Some(workspace_root));
    state.cache = Some(cache);
    state
}

pub(super) fn cache(&self) -> Option<&crate::cache::WorkspaceCacheLayout> {
    self.cache.as_ref()
}
```

In `run_load`, `try_incremental_reload`, `try_publish_cached`,
`build_and_publish_graph_file` and snapshot opening, replace
`graph_db_path(workspace_root)` with `self.cache().expect("workspace graph has cache layout").graph_db_path()`
or `graph.cache()` for the free build helper. Replace the persisted search mark
path in `graph/state.rs` with `cache.search_db_path()`.

- [ ] **Step 5: Run graph and lease tests**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p mcp-server workspace_lease -- --nocapture
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p mcp-server graph:: -- --nocapture
```

Expected: PASS, including existing default `.build` tests.

- [ ] **Step 6: Commit Task 2**

```powershell
git add crates/mcp-server/src/workspace_lease.rs crates/mcp-server/src/graph
git commit -m "feat(mcp): route graph and lease through cache layout"
```

### Task 3: Cache-aware SharedState and search lifecycle

**Files:**
- Modify: `crates/mcp-server/src/state/mod.rs`
- Modify: `crates/mcp-server/src/state/bootstrap.rs`
- Modify: `crates/mcp-server/src/state/embed.rs`

**Interfaces:**
- Consumes: `WorkspaceCacheLayout`, `WorkspaceLease::claim_cache`, `GraphState::for_workspace_with_cache`.
- Produces: `SharedState::workspace_with_cache(PathBuf, WorkspaceCacheLayout) -> Result<Self, ProjectError>`.
- Preserves: `SharedState::workspace(PathBuf)` as default-layout wrapper.

- [ ] **Step 1: Write failing SharedState external-cache test**

Add a bootstrap test using the existing minimal workspace fixture:

```rust
#[test]
fn workspace_state_uses_external_cache_without_creating_build_in_source() {
    use std::time::{Duration, Instant};

    let _env_lock = ENV_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let _embedding_url = EnvVarGuard::unset("EMBEDDING_URL");
    let _embedding_model = EnvVarGuard::unset("EMBEDDING_MODEL");
    let workspace = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    std::fs::write(
        workspace.path().join("Configuration.xml"),
        "<Configuration><Name>Конфа</Name></Configuration>",
    )
    .unwrap();
    write_common_module_tree(
        workspace.path(),
        "Сервер",
        "&НаСервере\nФункция Ч() Экспорт Возврат 1; КонецФункции\n",
    );
    let layout = crate::cache::WorkspaceCacheLayout::from_root(cache.path().to_path_buf());

    let state = SharedState::workspace_with_cache(workspace.path().to_path_buf(), layout.clone())
        .expect("valid workspace");

    let deadline = Instant::now() + Duration::from_secs(60);
    while state.search_engine().lock().unwrap().is_none() {
        assert!(Instant::now() < deadline, "the search engine never published");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(state.workspace_cache().unwrap(), &layout);
    assert!(layout.search_db_path().exists());
    assert!(layout.lease_path().exists());
    assert!(!workspace.path().join(".build").exists());
    state.shutdown();
}
```

Expose `workspace_cache()` as `pub(crate)` for assertions and internal consumers.

- [ ] **Step 2: Run the test and verify RED**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p mcp-server workspace_state_uses_external_cache -- --nocapture
```

Expected: compile failure because the constructor/accessor do not exist.

- [ ] **Step 3: Add the default wrapper and cache-aware constructor**

```rust
pub fn workspace(source_dir: PathBuf) -> Result<Self, project_model::ProjectError> {
    let cache = crate::cache::WorkspaceCacheLayout::for_workspace(&source_dir);
    Self::workspace_with_cache(source_dir, cache)
}

pub fn workspace_with_cache(
    source_dir: PathBuf,
    workspace_cache: crate::cache::WorkspaceCacheLayout,
) -> Result<Self, project_model::ProjectError> {
    // existing workspace body
}
```

Add `workspace_cache: Option<WorkspaceCacheLayout>` to `SharedState`; workspace
sets `Some`, reference/test-only non-workspace constructors set `None`.

- [ ] **Step 4: Thread layout through workspace initialization**

In `workspace_with_cache`:

```rust
let workspace_lease = WorkspaceLease::claim_cache(&workspace_cache);
let graph = GraphState::for_workspace_with_cache(source_dir.clone(), workspace_cache.clone())
    .with_change_hub(change_hub.clone())
    .with_publish_hook(publish_hook)
    .with_lease(workspace_lease.clone());
```

Add `WorkspaceCacheLayout` to `spawn_workspace_search_init` and every spawned
closure that opens local search/graph storage. Replace only production calls:

```rust
workspace_cache.ensure().ok();
let db_path = workspace_cache.search_db_path();
let graph_path = workspace_cache.graph_db_path();
```

Update `build_publish_hook`, `refresh_search_contexts_after_graph` and
`kick_context_reembed` to receive a cloned layout instead of recomputing graph
paths from `workspace_root`. Reference search keeps `reference_search_db_path()`.

- [ ] **Step 5: Run state, embed and integration tests**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p mcp-server state:: -- --nocapture
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p mcp-server --test contract
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p mcp-server --test metadata
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p mcp-server --test symbol_info
```

Expected: PASS.

- [ ] **Step 6: Commit Task 3**

```powershell
git add crates/mcp-server/src/state
git commit -m "feat(mcp): use cache layout for workspace search state"
```

### Task 4: CLI, backend identity and broker propagation

**Files:**
- Modify: `crates/mcp-server/src/broker/name.rs`
- Modify: `crates/mcp-server/tests/broker.rs`
- Modify: `crates/bsl-analyzer/src/bin/cli/mcp.rs`

**Interfaces:**
- Consumes: public `mcp_server::WorkspaceCacheLayout` and `SharedState::workspace_with_cache`.
- Changes: `BackendKey::new(source_dir, cache_dir, profile, config_fp, topology_fp)`.
- Produces: `resolve_workspace_cache(&Path, Option<&Path>, &Path) -> io::Result<WorkspaceCacheLayout>` as a pure/testable CLI seam.

- [ ] **Step 1: Write failing CLI and identity tests**

Add CLI tests:

```rust
#[test]
fn workspace_cli_accepts_cache_dir() {
    let cli = ServeCli::try_parse_from([
        "serve", "--profile", "workspace", "--source-dir", ".",
        "--cache-dir", "../кеш с пробелом",
    ]).unwrap();
    assert_eq!(cli.args.cache_dir, Some(PathBuf::from("../кеш с пробелом")));
}

#[test]
fn reference_rejects_cache_dir() {
    let mut args = serve_args(McpServeMode::Stdio, None);
    args.runtime_profile = McpProfileCli::Reference;
    args.source_dir = None;
    args.cache_dir = Some(PathBuf::from("cache"));
    assert!(validate_serve_args(&args).unwrap_err().to_string().contains("--cache-dir"));
}
```

Extend `broker/name.rs::each_identity_axis_forks_the_digest`:

```rust
let mut moved_cache = key("/srv/erp", McpProfile::Workspace, 7);
moved_cache.cache_dir = PathBuf::from("/var/cache/erp-next");
assert_ne!(base, moved_cache.digest(), "cache_dir");
```

Add a resolver test proving explicit default and implicit default yield equal
normalized roots.

- [ ] **Step 2: Run tests and verify RED**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p mcp-server broker::name::tests -- --nocapture
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p bsl-analyzer --bin bsl-analyzer-app cli::mcp::tests -- --nocapture
```

Expected: compile failures for the new field/signatures.

- [ ] **Step 3: Add cache root to BackendKey**

```rust
pub struct BackendKey {
    source_dir: PathBuf,
    cache_dir: PathBuf,
    profile: McpProfile,
    // existing fields
}

pub fn new(
    source_dir: impl Into<PathBuf>,
    cache_dir: impl Into<PathBuf>,
    profile: McpProfile,
    config_fp: u64,
    topology_fp: u64,
) -> Self { /* canonicalize both existing paths */ }
```

Hash `cache_dir.as_os_str().as_encoded_bytes()` after source dir and a NUL.
Update `crates/mcp-server/tests/broker.rs` helpers to pass
`WorkspaceCacheLayout::for_workspace(src.path()).root()`.

- [ ] **Step 4: Add and validate the CLI flag**

Add to `McpServeArgs`:

```rust
#[arg(long = "cache-dir")]
cache_dir: Option<PathBuf>,
```

In `validate_serve_args`, reject it for reference with:

```rust
if matches!(args.runtime_profile, McpProfileCli::Reference) && args.cache_dir.is_some() {
    return Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        "reference profile does not accept --cache-dir",
    ));
}
```

Implement the pure resolver:

```rust
fn resolve_workspace_cache(
    canonical_source_dir: &Path,
    cache_dir: Option<&Path>,
    current_dir: &Path,
) -> io::Result<mcp_server::WorkspaceCacheLayout> {
    match cache_dir {
        Some(path) => mcp_server::WorkspaceCacheLayout::prepare_explicit(path, current_dir),
        None => Ok(mcp_server::WorkspaceCacheLayout::for_workspace(canonical_source_dir)),
    }
}
```

- [ ] **Step 5: Propagate the resolved layout through all serve modes**

Update `run_mcp_server`, `run_mcp_http`, `run_mcp_broker`, `run_mcp_daemon` and
`build_server` signatures so workspace builds call:

```rust
mcp_server::SharedState::workspace_with_cache(source_dir, workspace_cache)
```

Broker must build its key with `workspace_cache.root()`, append to the daemon
command:

```rust
cmd.arg("--cache-dir").arg(workspace_cache.root());
```

and pass the same layout to direct stdio fallback. Daemon resolves the received
absolute path, derives the same key, and only then constructs state after bind.

- [ ] **Step 6: Route default daemon logging through cache-dir**

For truthy `BSL_MCP_DAEMON_LOG`, use the layout instead of reconstructing
`.build` in the CLI:

```rust
let source_dir = args.source_dir.as_deref().unwrap_or_else(|| Path::new("."));
let layout = args.cache_dir.clone()
    .map(mcp_server::WorkspaceCacheLayout::from_root)
    .unwrap_or_else(|| mcp_server::WorkspaceCacheLayout::for_workspace(source_dir));
layout.ensure().ok()?;
layout.daemon_log_path()
```

Keep explicit log-file values unchanged. Extend the existing daemon log test to
set `args.cache_dir = Some(external.path().to_path_buf())` and assert the log is
external and source `.build` is absent.

- [ ] **Step 7: Run CLI and broker suites**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p mcp-server --test broker -- --nocapture
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p bsl-analyzer --bin bsl-analyzer-app cli::mcp::tests -- --nocapture
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p bsl-analyzer --test mcp_install_cli
```

Expected: PASS; install snapshots stay unchanged because `mcp install` is out of scope.

- [ ] **Step 8: Commit Task 4**

```powershell
git add crates/mcp-server/src/broker/name.rs crates/mcp-server/tests/broker.rs crates/bsl-analyzer/src/bin/cli/mcp.rs
git commit -m "feat(mcp): add configurable workspace cache directory"
```

### Task 5: Documentation and final regression

**Files:**
- Modify: `docs/mcp/README.md`

**Interfaces:**
- Documents: `mcp serve --cache-dir <PATH>` only.

- [ ] **Step 1: Add user documentation**

Add after the manual workspace launch example:

```markdown
### Каталог кеша workspace

По умолчанию граф и поисковый индекс хранятся в `<source-dir>/.build`.
Чтобы source root содержал только выгрузку конфигурации, задайте внешний каталог:

```bash
bsl-analyzer mcp serve \
  --profile workspace \
  --source-dir ./my-project \
  --cache-dir ../.bsl-analyzer-cache/my-project
```

Относительный `--cache-dir` вычисляется от рабочего каталога процесса. Уже
существующий `.build` автоматически не переносится и может быть удалён вручную,
когда старые процессы analyzer остановлены.
```

- [ ] **Step 2: Run formatting, compile checks and full relevant tests**

```powershell
& "$env:USERPROFILE\.cargo\bin\cargo.exe" fmt --all -- --check
& "$env:USERPROFILE\.cargo\bin\cargo.exe" clippy -p mcp-server -p bsl-analyzer --all-targets -- -D warnings
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p mcp-server
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test -p bsl-analyzer
git diff --check
```

Expected: all commands PASS.

- [ ] **Step 3: Audit path ownership**

Run:

```powershell
rg -n 'join\("\.build"\)' crates/mcp-server/src crates/bsl-analyzer/src/bin/cli/mcp.rs
```

Expected: the literal `.build` path construction remains only in
`WorkspaceCacheLayout::for_workspace`; CLI logging and production graph/search/
lease code use layout methods. The external-layout tests from Tasks 2–4 provide
the executable source-root cleanliness check.

- [ ] **Step 4: Commit documentation**

```powershell
git add docs/mcp/README.md
git commit -m "docs(mcp): document external workspace cache"
```

- [ ] **Step 5: Record final evidence**

```powershell
git status --short --branch
git log --oneline origin/develop..HEAD
```

Expected: clean worktree; design, plan, implementation and documentation commits are present.
