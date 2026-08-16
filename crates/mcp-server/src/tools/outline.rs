//! `outline`: the map of ONE file — its regions, its declarations, their parameters — built
//! from one parse of that file and nothing else.
//!
//! Every other tool of this crate answers from something that had to be built first: the
//! resident analysis database, the call graph, the search index. On a large configuration
//! those take minutes, and while they build every question about a single file gets a retry
//! envelope. A map of one module needs none of them — the parse of that module IS the answer —
//! so this tool holds no resident, takes no lifecycle and returns no `loading`. That is the
//! whole reason it exists as a tool of its own, and it is why [`answer`] takes no argument
//! that could reach a resident, a graph or an index: the promise is enforced by the signature
//! rather than asserted in prose.
//!
//! What it does share is the addressing: the `(root_id, path)` pair, the reason vocabulary and
//! the freshness envelope all come from [`crate::tools::file_request`] and
//! [`crate::tools::location`], so a pair this tool answers under is a pair `diagnostics file`
//! accepts.

use std::path::{Path, PathBuf};

use bsl_search::WorkspaceRoots;
use ide::{
    Analysis, DocumentSymbol, OutlineMode, ParamDefault, ParamDetail, RootDatabaseImpl,
    SymbolDetail, TextRange,
};
use line_index::LineIndex;
use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;
use serde_json::{json, Map, Value};

use crate::tools::file_request::{answer_location, resolve_rooted_path, FileError};
use crate::tools::location as loc;
use crate::tools::response::structured;

/// Version of THIS tool's response shape. Separate from the location block's own version:
/// the block travels across tools and cannot be versioned by one of them.
pub(crate) const SCHEMA_VERSION: &str = "1";

/// Ceiling on a published default-value text, in characters.
///
/// About response size, nothing else. On real code it never fires — the widest default in a
/// 1 MB module is 20 characters — but a declaration the parser could not delimit (an unclosed
/// string literal, say) drags an unbounded run of the file into one field, and one such
/// parameter would otherwise eat the whole budget.
const DEFAULT_TEXT_CHARS: usize = 200;

const MODE_FULL: &str = "full";
const MODE_REGIONS: &str = "regions";

/// Parse the `mode` enum. An unknown value is refused rather than defaulted, so a caller is
/// never served a different question than it asked.
pub(crate) fn parse_mode(mode: Option<&str>) -> Result<OutlineMode, String> {
    match mode {
        None | Some(MODE_FULL) => Ok(OutlineMode::Full),
        Some(MODE_REGIONS) => Ok(OutlineMode::RegionsOnly),
        Some(other) => Err(format!("unknown mode '{other}'; expected {MODE_FULL}|{MODE_REGIONS}")),
    }
}

fn mode_str(mode: OutlineMode) -> &'static str {
    match mode {
        OutlineMode::Full => MODE_FULL,
        OutlineMode::RegionsOnly => MODE_REGIONS,
    }
}

/// Whether a parameter is optional, and whether the text of its default could be named.
///
/// A closed dictionary of three, not an optional string: "optional, text unknown" and
/// "required" are different answers to "may a caller omit this argument", and folding them
/// together changes the arity a consumer derives from the signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DefaultState {
    Required,
    Value,
    Unknown,
}

impl DefaultState {
    fn of(default: &ParamDefault) -> Self {
        match default {
            ParamDefault::Required => Self::Required,
            ParamDefault::Value(_) => Self::Value,
            ParamDefault::Unknown => Self::Unknown,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Value => "value",
            Self::Unknown => "unknown",
        }
    }
}

/// A file this tool agreed to answer about: where its bytes are, and the pair the answer
/// carries.
struct RequestedFile {
    abs: PathBuf,
    location: loc::Location,
}

/// A request that names no file this tool can serve. Carries its own prose: the code is
/// shared with `diagnostics file`, the sentence is not — see [`crate::tools::file_request`].
struct Refusal {
    error: FileError,
    detail: String,
    path: PathBuf,
}

impl Refusal {
    fn not_in_workspace(path: &Path, detail: impl Into<String>) -> Self {
        Self { error: FileError::NotInWorkspace, detail: detail.into(), path: path.to_path_buf() }
    }

    fn into_result(self, mode: OutlineMode) -> CallToolResult {
        let mut body = self.error.to_value(&self.detail, &self.path);
        let map = body.as_object_mut().expect("object literal");
        map.insert("schema_version".into(), json!(SCHEMA_VERSION));
        map.insert("mode".into(), json!(mode_str(mode)));
        map.insert(
            "freshness".into(),
            loc::Freshness::new(loc::FreshnessSource::FileParse, self.error.completeness())
                .to_value(),
        );
        structured(body)
    }
}

/// Turn the request's `(root_id, path)` into a file on disk plus the pair the answer will
/// carry.
///
/// A path with no `root_id` is read against the WORKSPACE root, the way `diagnostics file`
/// reads one — not against the configuration root. Handing it to the root table instead would
/// read it against the configuration, and with the configuration in a subdirectory that is a
/// different file.
fn resolve(
    roots: &WorkspaceRoots,
    workspace_root: &Path,
    root_id: Option<&str>,
    path: &Path,
) -> Result<RequestedFile, Refusal> {
    let resolved = resolve_rooted_path(roots, root_id, path).map_err(|error| {
        let detail = error.to_string();
        Refusal { error: FileError::Rooted(error), detail, path: path.to_path_buf() }
    })?;
    // Canonicalization failing is NOT an error: a path that names nothing still has to reach
    // the classification below, where it is answered `not_in_workspace` — the same answer
    // `diagnostics file` gives it. Refusing here would make the two tools disagree about one
    // input, and then the shared code vocabulary stops being shared.
    let abs = if resolved.is_absolute() { resolved } else { workspace_root.join(resolved) };
    let abs = abs.canonicalize().unwrap_or(abs);

    let location = answer_location(roots, root_id, path, &abs).map_err(|_| {
        Refusal::not_in_workspace(&abs, "the path lies outside every registered source root")
    })?;
    // The shared predicate, not a second extension check: this one compares without regard to
    // case and already serves the LSP server and the CLI, and a second procedure would differ
    // from it on exactly the inputs nobody tests.
    if !project_model::is_bsl_source_path(&abs) {
        return Err(Refusal::not_in_workspace(&abs, "the path does not name a `.bsl` source file"));
    }
    // Asked BEFORE the read, and about a REGULAR file. `read_to_string` reports a missing file
    // and a directory through the same error kind as an unreadable one, and answering either
    // `unreadable` would disagree with `diagnostics file`, which knows nothing of them and
    // says `not_in_workspace`.
    if !abs.is_file() {
        return Err(Refusal::not_in_workspace(
            &abs,
            "no regular file lies at that path: nothing is there, or it is a directory",
        ));
    }
    Ok(RequestedFile { abs, location })
}

/// The map of `symbols`, split per level into regions and declarations and fitted to a
/// character budget.
struct Walk<'a> {
    text: &'a str,
    index: &'a LineIndex,
    mode: OutlineMode,
    budget: usize,
    used: usize,
    kept: usize,
    /// A node was refused for want of budget, so the answer is a prefix of the map.
    dropped: bool,
    /// A default's text was cut to [`DEFAULT_TEXT_CHARS`] — a count cap, not the output
    /// budget: raising `max_output_tokens` does not lift it.
    capped_text: bool,
}

/// One level of the published tree.
#[derive(Default)]
struct Level {
    regions: Vec<Value>,
    members: Vec<Value>,
}

impl<'a> Walk<'a> {
    fn new(text: &'a str, index: &'a LineIndex, mode: OutlineMode, budget: usize) -> Self {
        Self { text, index, mode, budget, used: 0, kept: 0, dropped: false, capped_text: false }
    }

    /// Whether the response exceeds or falls short of the whole map. Both halves matter: a
    /// node was dropped, OR the one node we always deliver is itself over budget.
    fn truncated(&self) -> bool {
        self.dropped || self.used > self.budget
    }

    /// Walk one level IN FILE ORDER, splitting it afterwards.
    ///
    /// The order is the map's, not the published shape's: at map level a region and a method
    /// declared before it are siblings in declaration order, while the response lists
    /// `regions` before `members`. Selecting over the published arrays would therefore keep a
    /// different SET of nodes under the same budget — the region's subtree instead of the
    /// method that precedes it.
    fn level(&mut self, symbols: &[DocumentSymbol]) -> Result<Level, String> {
        let mut level = Level::default();
        for symbol in symbols {
            if self.dropped {
                break;
            }
            let Some(value) = self.node(symbol)? else { break };
            match symbol.detail {
                SymbolDetail::Region => level.regions.push(value),
                _ => level.members.push(value),
            }
        }
        Ok(level)
    }

    /// One node and everything under it, or `None` when the budget ran out before it.
    ///
    /// The node is never half-built: it is priced whole (minus its children, which are priced
    /// as they are reached) and either enters the answer complete or not at all. Nothing here
    /// trims rendered JSON, which is the only way a response stays parseable at any budget.
    fn node(&mut self, symbol: &DocumentSymbol) -> Result<Option<Value>, String> {
        let (mut body, cut_a_text) = shallow(symbol, self)?;
        // `+1` is the comma this node costs its array; the framing of the arrays themselves is
        // small and constant, and pricing it per node would over-charge nested levels.
        let cost = serde_json::to_string(&body).map(|json| json.len()).unwrap_or(0) + 1;
        if self.kept > 0 && self.used + cost > self.budget {
            self.dropped = true;
            return Ok(None);
        }
        self.used += cost;
        self.kept += 1;
        // Recorded only now, after the node is certain to be published. A reason describes
        // what the ANSWER carries; a node built, priced and refused carries nothing, and
        // `result_cap` beside no `text_truncated` tells a consumer that raising the budget
        // cannot help — the opposite of true when the budget is exactly what refused it.
        self.capped_text |= cut_a_text;

        // Only a region holds anything: stage B's map has no children under a method, and the
        // corpus test below is what keeps that true rather than this comment.
        if symbol.detail == SymbolDetail::Region {
            let children = self.level(&symbol.children)?;
            body.insert("regions".into(), Value::Array(children.regions));
            if self.mode == OutlineMode::Full {
                body.insert("members".into(), Value::Array(children.members));
            }
        }
        Ok(Some(Value::Object(body)))
    }

    /// A node's span in the units the location contract publishes.
    ///
    /// The failure is an error rather than an omitted range: the span comes from a parse of
    /// the very text being measured, so a projection that fails means the two disagree, and an
    /// answer that names a node without saying where it is cannot be acted on anyway.
    fn project(&self, range: TextRange) -> Result<loc::PositionRange, String> {
        self.index
            .utf16_line_col_range(self.text, range)
            .map(loc::PositionRange::from)
            .ok_or_else(|| format!("a node's span {range:?} does not lie in the file it came from"))
    }
}

/// What to do about a map that did not fit — and only what this caller has not done already.
///
/// A skeleton deep enough in nested regions overflows the budget on its own, and there the
/// narrower question is the one being asked: offering it back sends the caller in a circle,
/// which is worse than no hint at all, since the hint is the whole point of the field.
fn budget_hint(mode: OutlineMode) -> &'static str {
    match mode {
        OutlineMode::Full => {
            "ask `mode: \"regions\"` for the module's skeleton, or raise `max_output_tokens`"
        }
        OutlineMode::RegionsOnly => {
            "raise `max_output_tokens`: this is already the narrowest question, and the \
             region skeleton alone does not fit"
        }
    }
}

/// The node itself, without its children, and whether building it had to cut a default's text.
///
/// A free function taking `&Walk` rather than a method taking `&mut Walk`: what it discovers
/// travels back in the return value, so a node the caller then refuses cannot leave a mark on
/// the walk. The borrow checker is what enforces that, rather than a rule someone remembers.
fn shallow(symbol: &DocumentSymbol, walk: &Walk<'_>) -> Result<(Map<String, Value>, bool), String> {
    let name_range = walk.project(symbol.selection_range)?;
    let whole_range = walk.project(symbol.range)?;

    let mut cut_a_text = false;
    let mut body = Map::new();
    body.insert("kind".into(), json!(symbol.kind().as_str()));
    body.insert("name".into(), json!(symbol.name));
    match &symbol.detail {
        SymbolDetail::Procedure(method) | SymbolDetail::Function(method) => {
            body.insert("export".into(), json!(method.is_export));
            body.insert("directives".into(), directives(&method.directives));
            let params: Vec<Value> = method
                .params
                .iter()
                .map(|param| {
                    let (value, cut) = published_param(param);
                    cut_a_text |= cut;
                    value
                })
                .collect();
            body.insert("params".into(), Value::Array(params));
        }
        SymbolDetail::Variable(variable) => {
            body.insert("export".into(), json!(variable.is_export));
            body.insert("directives".into(), directives(&variable.directives));
        }
        SymbolDetail::Region => {}
    }
    body.insert("range".into(), name_range.to_value());
    body.insert("enclosing_range".into(), whole_range.to_value());
    Ok((body, cut_a_text))
}

/// One parameter, and whether its default's text had to be cut.
fn published_param(param: &ParamDetail) -> (Value, bool) {
    let mut cut_a_text = false;
    let mut default = Map::new();
    default.insert("state".into(), json!(DefaultState::of(&param.default).as_str()));
    if let ParamDefault::Value(text) = &param.default {
        let (text, cut) = cap_chars(text, DEFAULT_TEXT_CHARS);
        default.insert("text".into(), json!(text));
        if cut {
            cut_a_text = true;
            default.insert("text_truncated".into(), json!(true));
        }
    }
    let value = json!({
        "name": param.name,
        "by_value": param.by_value,
        "default": Value::Object(default),
    });
    (value, cut_a_text)
}

fn directives(kinds: &[ide::AnnotationKind]) -> Value {
    Value::Array(kinds.iter().map(|kind| json!(kind.as_str())).collect())
}

/// Cut `text` to `limit` CHARACTERS, reporting whether it had to. Characters, not bytes: a cut
/// mid-character would not be valid UTF-8, and a byte limit means a different number of
/// Cyrillic characters than of Latin ones.
fn cap_chars(text: &str, limit: usize) -> (String, bool) {
    match text.char_indices().nth(limit) {
        Some((at, _)) => (text[..at].to_owned(), true),
        None => (text.to_owned(), false),
    }
}

/// A Salsa database holding exactly one file, whose text is the string we already read.
///
/// Registered as an overlay rather than disk-backed, so nothing re-opens the file: the bytes
/// in hand are the bytes measured, and the offsets in the map are offsets into them.
fn single_file_db(path: &Path, text: &str) -> (RootDatabaseImpl, vfs::FileId) {
    use base_db::{SourceDatabase, SourceRoot, SourceRootId};

    let mut db = RootDatabaseImpl::new();
    let file_id = vfs::FileId(0);
    let mut file_set = vfs::FileSet::new();
    file_set.insert(file_id, vfs::VfsPath::new(path.to_path_buf()));
    db.set_source_root(SourceRootId(0), SourceRoot::new_local(file_set));
    db.set_file_source_root(file_id, SourceRootId(0));
    db.set_file_text(file_id, text);
    (db, file_id)
}

/// The whole tool: a pair in, a map out.
///
/// Note what is NOT in the signature — no resident, no graph, no search index, no shared state
/// of any kind. The root table and the workspace root are values, both derivable from the
/// project alone.
pub(crate) fn answer(
    roots: &WorkspaceRoots,
    workspace_root: &Path,
    root_id: Option<&str>,
    path: &Path,
    mode: OutlineMode,
    max_output_tokens: usize,
) -> Result<CallToolResult, McpError> {
    let file = match resolve(roots, workspace_root, root_id, path) {
        Ok(file) => file,
        Err(refusal) => return Ok(refusal.into_result(mode)),
    };
    let text = match base_db::read_disk_text(&file.abs) {
        Ok(text) => text,
        // Gone between the check above and the read: the file the caller named is not there,
        // which is the same fact `not_in_workspace` reports, not a reading failure.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Refusal::not_in_workspace(
                &file.abs,
                "the file disappeared before it \
                 could be read",
            )
            .into_result(mode))
        }
        Err(error) => {
            return Ok(Refusal {
                error: FileError::Unreadable,
                detail: format!(
                    "the file is there but its bytes could not be read as UTF-8 text: {error}"
                ),
                path: file.abs,
            }
            .into_result(mode))
        }
    };

    let (db, file_id) = single_file_db(&file.abs, &text);
    let analysis = Analysis::from_database(db);
    let symbols = analysis.file_outline(file_id, mode);

    let index = LineIndex::new(&text);
    // The same "1 token ≈ 4 characters" convention every other tool of this crate budgets by.
    let mut walk = Walk::new(&text, &index, mode, max_output_tokens.saturating_mul(4));
    let level = walk.level(&symbols).map_err(|detail| McpError::internal_error(detail, None))?;

    let truncated = walk.truncated();
    let completeness = loc::Completeness::complete()
        .when(
            truncated,
            loc::ReasonCode::OutputBudget,
            "the map did not fit the output budget, so it stops partway through the file",
        )
        .when(
            walk.capped_text,
            loc::ReasonCode::ResultCap,
            "a default value's text was cut to its own length limit; \
             raising max_output_tokens does not lift it",
        );

    let mut body = json!({
        "schema_version": SCHEMA_VERSION,
        "mode": mode_str(mode),
        "location": file.location.to_value(),
        "regions": Value::Array(level.regions),
        "truncated": truncated,
        "freshness": loc::Freshness::new(loc::FreshnessSource::FileParse, completeness).to_value(),
    });
    let map = body.as_object_mut().expect("object literal");
    // Absent, not empty, in `regions` mode: an empty array reads as "this file declares no
    // methods", which is a lie about the file rather than a fact about the question asked.
    if mode == OutlineMode::Full {
        map.insert("members".into(), Value::Array(level.members));
    }
    if truncated {
        map.insert("budget_hint".into(), json!(budget_hint(mode)));
    }
    Ok(structured(body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const MODULE_REL: &str = "CommonModules/М/Ext/Module.bsl";

    /// A workspace that IS its own configuration, holding one module with `body`.
    fn stand(body: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().to_path_buf();
        fs::write(
            workspace.join("Configuration.xml"),
            "<Configuration><Name>Конфа</Name></Configuration>",
        )
        .unwrap();
        fs::write(workspace.join("bsl-analyzer.toml"), "[source]\nroot = \".\"\n").unwrap();
        let module = workspace.join(MODULE_REL);
        fs::create_dir_all(module.parent().unwrap()).unwrap();
        fs::write(&module, body).unwrap();
        (dir, workspace)
    }

    fn roots_of(workspace: &Path) -> WorkspaceRoots {
        let project = crate::project::at(workspace).expect("the stand is a valid project");
        crate::project::workspace_roots(&project).0
    }

    fn call(
        workspace: &Path,
        root_id: Option<&str>,
        path: &str,
        mode: OutlineMode,
        max_output_tokens: usize,
    ) -> Value {
        let roots = roots_of(workspace);
        answer(&roots, workspace, root_id, Path::new(path), mode, max_output_tokens)
            .expect("the tool answers in band")
            .structured_content
            .expect("a structured body")
    }

    /// The map of a module written from `body`, at a budget large enough to hold it whole.
    fn map_of(body: &str) -> Value {
        let (_dir, workspace) = stand(body);
        call(&workspace, None, MODULE_REL, OutlineMode::Full, 100_000)
    }

    fn names(nodes: &Value) -> Vec<String> {
        nodes
            .as_array()
            .expect("an array of nodes")
            .iter()
            .map(|node| node["name"].as_str().expect("a name").to_owned())
            .collect()
    }

    // Ф1: export and directives.
    const EXPORT_AND_DIRECTIVES: &str =
        "&НаКлиенте\n&НаСервере\nПроцедура П(А) Экспорт\nКонецПроцедуры\n";

    // Ф2: the three default states side by side, and `Знач`.
    const THREE_DEFAULTS: &str = "Процедура П(Знач А = 1, Б, В = ) КонецПроцедуры\n";

    // Ф3: regions, nesting, two variables of one `Перем`, a method declared before the region,
    // and a region inside a method body (which stage B keeps out of the map).
    const SHAPED_MODULE: &str = "&НаКлиенте\n&НаСервере\nПроцедура Внешняя(Знач А = 1, Б, В = ) Экспорт\n#Область ВнутриТела\nА = 1;\n#КонецОбласти\nКонецПроцедуры\n#Область Служебные\n&НаКлиенте\nПерем Кэш, Второй Экспорт;\nФункция Считать() Экспорт\nКонецФункции\n#Область Вложенная\n&Вместо(\"Ч\")\nПроцедура Подменённая()\nКонецПроцедуры\n#КонецОбласти\n#КонецОбласти\n";

    // Ф5: a non-BMP character and Cyrillic before the name, so bytes, code points and UTF-16
    // code units are three different numbers.
    const ENCODING_FIXTURE: &str = "/*🙂*/ Процедура Тест() Экспорт\nКонецПроцедуры\n";

    // Ф6: the same identifier in a body and in a name.
    const BODY_MARKER: &str = "Процедура П()\nМАРКЕР_ТЕЛА_МЕТОДА = 1;\nКонецПроцедуры\nПроцедура МАРКЕР_ТЕЛА_МЕТОДА()\nКонецПроцедуры\n";

    /// И2. The tree is regions recursively, declarations beside them, both in file order.
    #[test]
    fn the_map_is_a_tree_of_regions_with_declarations_beside_them() {
        let body = map_of(SHAPED_MODULE);

        // `Внешняя` is declared BEFORE the region, so a shape that lists regions first — or a
        // flat list with a "region" flag — answers differently here.
        assert_eq!(names(&body["members"]), ["Внешняя"], "{body}");
        assert_eq!(names(&body["regions"]), ["Служебные"], "{body}");

        let service = &body["regions"][0];
        assert_eq!(names(&service["members"]), ["Кэш", "Второй", "Считать"], "{service}");
        assert_eq!(names(&service["regions"]), ["Вложенная"], "{service}");
        assert_eq!(names(&service["regions"][0]["members"]), ["Подменённая"], "{service}");

        assert_eq!(body["freshness"]["completeness"]["status"], "complete", "{body}");
        assert_eq!(body["truncated"], false, "{body}");
    }

    /// И2, second half. An empty file has an empty map, and that is a complete answer — not an
    /// error, and not a partial one.
    #[test]
    fn an_empty_file_has_an_empty_map_and_nothing_to_apologise_for() {
        let body = map_of("");

        assert_eq!(body["regions"], json!([]), "{body}");
        assert_eq!(body["members"], json!([]), "{body}");
        assert_eq!(body["freshness"]["completeness"]["status"], "complete", "{body}");
        assert!(body.get("error").is_none(), "{body}");
    }

    /// И3. Kinds and directives are the canonical strings, spelled by the layers that own
    /// them. An adapter inventing `Procedure` or `&НаКлиенте` answers differently.
    #[test]
    fn kinds_and_directives_come_from_the_canonical_vocabularies() {
        let body = map_of(SHAPED_MODULE);

        assert_eq!(body["members"][0]["kind"], "procedure", "{body}");
        let service = &body["regions"][0];
        assert_eq!(service["kind"], "region", "{service}");
        assert_eq!(service["members"][0]["kind"], "variable", "{service}");
        assert_eq!(service["members"][2]["kind"], "function", "{service}");

        // A region never appears among the members, nor a declaration among the regions.
        for node in service["members"].as_array().unwrap() {
            assert_ne!(node["kind"], "region", "{service}");
        }
        for node in service["regions"].as_array().unwrap() {
            assert_eq!(node["kind"], "region", "{service}");
        }

        let directives = map_of(EXPORT_AND_DIRECTIVES);
        assert_eq!(
            directives["members"][0]["directives"],
            json!(["at_client", "at_server"]),
            "{directives}",
        );
    }

    /// И4. The declaration reaches the caller whole: export, parameter order, `Знач`, and the
    /// three default states apart.
    #[test]
    fn a_declaration_arrives_with_its_parameters_and_their_defaults() {
        let body = map_of(THREE_DEFAULTS);
        let params = &body["members"][0]["params"];

        assert_eq!(names(params), ["А", "Б", "В"], "{body}");
        assert_eq!(
            params.as_array().unwrap().iter().map(|p| p["by_value"].clone()).collect::<Vec<_>>(),
            [json!(true), json!(false), json!(false)],
            "{body}",
        );
        // The middle one is REQUIRED and the last one is OPTIONAL with an unreadable text.
        // Folding the two would change the arity a caller derives from the signature.
        assert_eq!(params[0]["default"], json!({ "state": "value", "text": "1" }), "{body}");
        assert_eq!(params[1]["default"], json!({ "state": "required" }), "{body}");
        assert_eq!(params[2]["default"], json!({ "state": "unknown" }), "{body}");

        let exported = map_of(EXPORT_AND_DIRECTIVES);
        assert_eq!(exported["members"][0]["export"], true, "{exported}");
    }

    /// И5. No method body reaches the answer.
    ///
    /// The marker stands in a body AND in a name, so the count tells "the body is absent" from
    /// "the search found nothing at all" — a check looking for zero occurrences would pass on a
    /// response that had lost the map entirely.
    #[test]
    fn no_method_body_reaches_the_answer() {
        let body = map_of(BODY_MARKER);
        let rendered = serde_json::to_string(&body).unwrap();

        assert_eq!(
            rendered.matches("МАРКЕР_ТЕЛА_МЕТОДА").count(),
            1,
            "the only occurrence must be the second method's name: {rendered}",
        );
        assert_eq!(names(&body["members"]), ["П", "МАРКЕР_ТЕЛА_МЕТОДА"], "{body}");
    }

    /// И6. `regions` is a narrower QUESTION, not a trimmed answer: the same skeleton, no
    /// `members` key anywhere, and a complete answer.
    #[test]
    fn the_regions_mode_narrows_the_question_rather_than_the_answer() {
        let (_dir, workspace) = stand(SHAPED_MODULE);
        let full = call(&workspace, None, MODULE_REL, OutlineMode::Full, 100_000);
        let skeleton = call(&workspace, None, MODULE_REL, OutlineMode::RegionsOnly, 100_000);

        assert_eq!(skeleton["mode"], "regions", "{skeleton}");
        assert_eq!(skeleton["freshness"]["completeness"]["status"], "complete", "{skeleton}");
        assert_eq!(skeleton["truncated"], false, "{skeleton}");

        // Same regions, same names, same spans, same nesting as the full map.
        assert_eq!(strip_members(full["regions"].clone()), skeleton["regions"], "{skeleton}");

        assert!(skeleton.get("members").is_none(), "{skeleton}");
        for node in walk_nodes(&skeleton["regions"]) {
            assert!(node.get("members").is_none(), "a narrowed answer has no members key: {node}");
        }
    }

    /// И9. The budget drops whole nodes and leaves a tree, never a cut string.
    #[test]
    fn truncation_drops_nodes_and_keeps_the_tree_intact() {
        // Ф7: declarations and regions alternate on BOTH levels, so a selection made over the
        // published arrays (regions first) keeps a different set than one made over the file.
        let mut source = String::from("Процедура Первая()\nКонецПроцедуры\n#Область A\nПроцедура A1()\nКонецПроцедуры\n#Область A2\n");
        for i in 0..20 {
            source.push_str(&format!("Процедура A2_{i}()\nКонецПроцедуры\n"));
        }
        source.push_str(
            "#КонецОбласти\n#КонецОбласти\nПроцедура Между()\nКонецПроцедуры\n#Область B\n",
        );
        for i in 0..20 {
            source.push_str(&format!("Процедура B_{i}()\nКонецПроцедуры\n"));
        }
        source.push_str("#КонецОбласти\n");

        let (_dir, workspace) = stand(&source);
        let whole = call(&workspace, None, MODULE_REL, OutlineMode::Full, 100_000);

        // The budget is the price of exactly two shell nodes by the walk's own formula — the
        // node WITHOUT its children plus its comma — and not the length of two finished nodes,
        // which already carry their subtrees.
        let first = shallow_len(&whole["members"][0]);
        let region = shallow_len(&whole["regions"][0]);
        // Rounded UP into tokens: the walk budgets in characters at four per token, and a
        // rounded-down budget would be a few characters short of the two nodes on purpose.
        let budget_chars = first + 1 + region + 1;
        let body = call(&workspace, None, MODULE_REL, OutlineMode::Full, budget_chars.div_ceil(4));

        assert_eq!(body["truncated"], true, "{body}");
        assert_eq!(body["freshness"]["completeness"]["status"], "partial", "{body}");
        assert_eq!(
            body["freshness"]["completeness"]["reasons"][0]["code"], "output_budget",
            "{body}",
        );
        assert!(body["budget_hint"].is_string(), "{body}");

        // File order: `Первая` and the empty shell of `A`. A walk over the published arrays
        // would have taken `A` and `A1` instead, leaving `Первая` out.
        assert_eq!(names(&body["members"]), ["Первая"], "{body}");
        assert_eq!(names(&body["regions"]), ["A"], "{body}");
        assert_eq!(body["regions"][0]["members"], json!([]), "{body}");
        assert_eq!(body["regions"][0]["regions"], json!([]), "{body}");

        // Every surviving node is whole — a cut of the rendered JSON would fail here.
        for node in walk_nodes(&body["regions"]).into_iter().chain(walk_nodes(&body["members"])) {
            assert!(node["name"].is_string(), "{node}");
            assert!(node["range"]["start_line"].is_u64(), "{node}");
            assert!(node["enclosing_range"]["end_line"].is_u64(), "{node}");
        }

        // And the same input at a large budget is NOT truncated: a gate that only ever saw the
        // truncating run would pass on an implementation that always truncates.
        assert_eq!(whole["truncated"], false, "{whole}");
        assert_eq!(whole["freshness"]["completeness"]["status"], "complete", "{whole}");
        assert_eq!(names(&whole["members"]), ["Первая", "Между"], "{whole}");
        assert_eq!(names(&whole["regions"]), ["A", "B"], "{whole}");
    }

    /// И10. A runaway default is cut, says so, and is reported as a count cap rather than as
    /// the output budget — raising `max_output_tokens` would not lift it.
    #[test]
    fn a_long_default_is_cut_and_says_so() {
        // Ф8: an unclosed string literal, which is how a default text grows without bound.
        let long = format!("Процедура П(А = \"{}\nКонецПроцедуры\n", "ы".repeat(500));
        let body = map_of(&long);
        let default = &body["members"][0]["params"][0]["default"];

        assert_eq!(default["state"], "value", "{body}");
        assert_eq!(default["text_truncated"], true, "{body}");
        assert_eq!(
            default["text"].as_str().expect("text").chars().count(),
            DEFAULT_TEXT_CHARS,
            "{body}",
        );
        let reasons = body["freshness"]["completeness"]["reasons"].to_string();
        assert!(reasons.contains("result_cap"), "{body}");

        // A one-character default carries neither the flag nor the reason: a cap that fired
        // always would be indistinguishable from one that fires by need.
        let short = map_of(THREE_DEFAULTS);
        assert!(
            short["members"][0]["params"][0]["default"].get("text_truncated").is_none(),
            "{short}",
        );
        assert_eq!(short["freshness"]["completeness"]["status"], "complete", "{short}");
    }

    /// A reason describes what the ANSWER carries. A node priced and then dropped for want of
    /// budget carries nothing, so nothing it did on the way may reach the envelope.
    ///
    /// The input is the only shape where the two facts meet: a runaway default on a node the
    /// budget then refuses. Without it the sticky flag is invisible — on every other input the
    /// node that cut a text is also a node that survives.
    #[test]
    fn a_dropped_node_leaves_no_trace_of_the_text_it_never_published() {
        let source = format!(
            "Процедура Первая()\nКонецПроцедуры\nПроцедура Вторая(А = \"{}\nКонецПроцедуры\n",
            "ы".repeat(500),
        );
        let (_dir, workspace) = stand(&source);
        let whole = call(&workspace, None, MODULE_REL, OutlineMode::Full, 100_000);

        // Room for the first procedure and nothing more, so the second — the one with the
        // runaway default — is priced and refused.
        let budget = (shallow_len(&whole["members"][0]) + 1).div_ceil(4);
        let body = call(&workspace, None, MODULE_REL, OutlineMode::Full, budget);

        assert_eq!(names(&body["members"]), ["Первая"], "{body}");
        assert_eq!(body["truncated"], true, "{body}");
        let rendered = serde_json::to_string(&body).unwrap();
        assert!(!rendered.contains("text_truncated"), "nothing was cut in this answer: {body}");
        let reasons: Vec<&str> = body["freshness"]["completeness"]["reasons"]
            .as_array()
            .expect("reasons")
            .iter()
            .map(|reason| reason["code"].as_str().expect("a code"))
            .collect();
        assert_eq!(
            reasons,
            ["output_budget"],
            "result_cap here would tell a consumer that raising the budget cannot help, \
             which is the opposite of true: {body}",
        );

        // Control: the same module at a budget that fits BOTH nodes does publish the cut, so
        // the assertion above is about the dropped node and not about the cap never firing.
        assert!(whole["members"][1]["params"][0]["default"]["text_truncated"] == true, "{whole}");
        let kept_reasons = whole["freshness"]["completeness"]["reasons"].to_string();
        assert!(kept_reasons.contains("result_cap"), "{whole}");
    }

    /// The hint names a correction the caller has not already made. In `regions` mode there is
    /// no narrower question left, so pointing at it sends the caller in a circle.
    #[test]
    fn the_budget_hint_does_not_offer_the_mode_the_caller_is_already_in() {
        let mut source = String::new();
        for i in 0..60 {
            source.push_str(&format!("#Область Область_С_Длинным_Именем_{i}\n"));
        }
        for _ in 0..60 {
            source.push_str("#КонецОбласти\n");
        }

        let (_dir, workspace) = stand(&source);
        let skeleton = call(&workspace, None, MODULE_REL, OutlineMode::RegionsOnly, 50);
        assert_eq!(skeleton["truncated"], true, "the stand must actually truncate: {skeleton}");

        let hint = skeleton["budget_hint"].as_str().expect("a hint");
        assert!(!hint.contains("regions"), "already in that mode: {hint}");
        assert!(hint.contains("max_output_tokens"), "{hint}");

        // In `full` mode the narrower question is a real correction, so it is still offered:
        // a hint stripped of it everywhere would pass the assertion above for the wrong reason.
        let full = call(&workspace, None, MODULE_REL, OutlineMode::Full, 50);
        assert_eq!(full["truncated"], true, "{full}");
        assert!(full["budget_hint"].as_str().expect("a hint").contains("regions"), "{full}");
    }

    /// И12. Columns are UTF-16 code units.
    ///
    /// The fixture puts a non-BMP character and Cyrillic before the name, so the same position
    /// is 28 in bytes, 16 in code points and 17 in UTF-16 code units — three different numbers,
    /// and neither wrong unit can pass by coincidence.
    #[test]
    fn columns_are_utf16_code_units() {
        let body = map_of(ENCODING_FIXTURE);
        let node = &body["members"][0];

        assert_eq!(node["name"], "Тест", "{body}");
        assert_eq!(
            node["range"],
            json!({ "start_line": 0, "start_character": 17, "end_line": 0, "end_character": 21 }),
            "{body}",
        );
        // The declaration starts at `Процедура`, not at the comment before it.
        assert_eq!(
            node["enclosing_range"],
            json!({ "start_line": 0, "start_character": 7, "end_line": 1, "end_character": 14 }),
            "{body}",
        );
        assert_eq!(body["location"]["position_encoding"], "utf-16", "{body}");
    }

    /// И14. The place is published once, at the root; a node carries only its two spans.
    #[test]
    fn the_place_is_named_once_and_the_nodes_carry_only_spans() {
        let body = map_of(SHAPED_MODULE);

        assert_eq!(body["location"]["root_id"], "", "{body}");
        assert_eq!(body["location"]["path"], MODULE_REL, "{body}");
        assert_eq!(body["location"]["schema_version"], "1", "{body}");
        assert!(body.get("range").is_none(), "the root is a file, not a node: {body}");

        let nodes: Vec<&Value> =
            walk_nodes(&body["regions"]).into_iter().chain(walk_nodes(&body["members"])).collect();
        assert!(nodes.len() >= 5, "the fixture must reach several depths: {body}");
        for node in nodes {
            for repeated in ["root_id", "path", "position_encoding", "schema_version", "location"] {
                assert!(node.get(repeated).is_none(), "{repeated} repeated on a node: {node}");
            }
            assert!(node["range"].is_object(), "{node}");
            assert!(node["enclosing_range"].is_object(), "{node}");
        }
    }

    /// И8(a). The answer comes from one parse of one file: [`answer`] takes no argument that
    /// could reach a resident, a graph or an index, and it says so in the envelope.
    #[test]
    fn the_answer_names_a_file_parse_and_borrows_no_identity() {
        let body = map_of(SHAPED_MODULE);

        assert_eq!(body["freshness"]["source"], "file-parse", "{body}");
        assert!(body["freshness"]["revision"].is_null(), "{body}");
        assert!(body["freshness"]["topology_fingerprint"].is_null(), "{body}");
        assert!(body["freshness"]["stale"].is_null(), "{body}");
        // A retry envelope is what a resident-backed tool answers while it builds; this tool
        // has nothing to build, so the key never appears.
        assert!(body.get("status").is_none(), "{body}");
    }

    /// И7(a). The pair addresses the file it names: two roots hold a module at the same
    /// relative path, and the answer comes from the root the request named.
    #[test]
    fn a_rooted_path_is_answered_from_its_own_root() {
        use crate::diagnostics_state::test_support::{
            extension_root_id, workspace_with_an_outside_extension, CONFIGURATION_SYMBOL,
            EXTENSION_SYMBOL, SHARED_MODULE_REL,
        };
        let (_dir, workspace, extension) = workspace_with_an_outside_extension();
        let root_id = extension_root_id(&workspace, &extension);

        let body = call(&workspace, Some(&root_id), SHARED_MODULE_REL, OutlineMode::Full, 100_000);

        assert_eq!(names(&body["members"]), [EXTENSION_SYMBOL], "{body}");
        assert!(!names(&body["members"]).contains(&CONFIGURATION_SYMBOL.to_owned()), "{body}");
        assert_eq!(body["location"]["root_id"], root_id, "{body}");
        assert_eq!(body["location"]["path"], SHARED_MODULE_REL, "{body}");
    }

    /// И7(в). A path with no `root_id` is read against the WORKSPACE root, and the pair it
    /// comes back under names the same file.
    ///
    /// The stand is the one shape where the two readings differ: the configuration sits in a
    /// subdirectory, so handing the relative path to the root table — which reads it against
    /// the CONFIGURATION — either misses or doubles the `src/cf` prefix.
    #[test]
    fn a_path_without_a_root_is_read_against_the_workspace() {
        use crate::diagnostics_state::test_support::{
            workspace_with_a_nested_configuration, CONFIGURATION_SYMBOL, SHARED_MODULE_REL,
        };
        let (_dir, workspace) = workspace_with_a_nested_configuration();
        let from_workspace = format!("src/cf/{SHARED_MODULE_REL}");

        let bare = call(&workspace, None, &from_workspace, OutlineMode::Full, 100_000);
        assert_eq!(names(&bare["members"]), [CONFIGURATION_SYMBOL], "{bare}");
        assert_eq!(bare["location"]["root_id"], "", "{bare}");
        assert_eq!(bare["location"]["path"], SHARED_MODULE_REL, "{bare}");

        // And the pair it published names that same file when fed back in.
        let paired = call(&workspace, Some(""), SHARED_MODULE_REL, OutlineMode::Full, 100_000);
        assert_eq!(paired["members"], bare["members"], "{paired}");
    }

    /// И11. Every way of naming no file has its own code, and the two that a naive
    /// implementation would merge differ in completeness as well.
    #[test]
    fn every_addressing_failure_is_named_by_its_own_code() {
        use crate::diagnostics_state::test_support::{
            extension_root_id, workspace_with_an_outside_extension, SHARED_MODULE_REL,
        };
        let (_dir, workspace, extension) = workspace_with_an_outside_extension();
        let root_id = extension_root_id(&workspace, &extension);
        let absolute = extension.join(SHARED_MODULE_REL);

        let refuse = |root: Option<&str>, path: &str| {
            call(&workspace, root, path, OutlineMode::Full, 100_000)
        };

        let unknown = refuse(Some("нет-такого-корня"), SHARED_MODULE_REL);
        assert_eq!(unknown["error"], "unknown_root", "{unknown}");
        assert!(unknown["detail"].as_str().unwrap().contains("нет-такого-корня"), "{unknown}");

        let absolute_under_root = refuse(Some(&root_id), absolute.to_str().unwrap());
        assert_eq!(
            absolute_under_root["error"], "absolute_path_under_root",
            "{absolute_under_root}"
        );

        let escaping = refuse(Some(&root_id), "../Соседний.bsl");
        assert_eq!(escaping["error"], "path_not_relative_to_root", "{escaping}");

        let outside = refuse(None, "/nowhere/Чужой.bsl");
        assert_eq!(outside["error"], "not_in_workspace", "{outside}");

        // The two a single `unreadable` branch would merge with the one below: nothing at the
        // path, and a DIRECTORY wearing a `.bsl` name. Both are `not_in_workspace`, complete —
        // the same answer `diagnostics file` gives them.
        let missing = refuse(Some(""), "CommonModules/Нет/Ext/Module.bsl");
        assert_eq!(missing["error"], "not_in_workspace", "{missing}");
        assert_eq!(missing["freshness"]["completeness"]["status"], "complete", "{missing}");

        fs::create_dir_all(workspace.join("CommonModules/Каталог.bsl")).unwrap();
        let directory = refuse(Some(""), "CommonModules/Каталог.bsl");
        assert_eq!(directory["error"], "not_in_workspace", "{directory}");
        assert_eq!(directory["freshness"]["completeness"]["status"], "complete", "{directory}");

        // Ф9: bytes that are not UTF-8. The file IS there, so this one is `unreadable` and
        // PARTIAL — a different code and a different completeness from the two above.
        let broken = workspace.join("CommonModules/Битый/Ext/Module.bsl");
        fs::create_dir_all(broken.parent().unwrap()).unwrap();
        fs::write(&broken, [0xFF, 0xFE, 0x00]).unwrap();
        let unreadable = refuse(Some(""), "CommonModules/Битый/Ext/Module.bsl");
        assert_eq!(unreadable["error"], "unreadable", "{unreadable}");
        assert_eq!(unreadable["freshness"]["completeness"]["status"], "partial", "{unreadable}");
        assert_eq!(
            unreadable["freshness"]["completeness"]["reasons"][0]["code"], "unreadable_files",
            "{unreadable}",
        );

        // Not a `.bsl` at all: a real file, and still not this tool's subject.
        fs::write(workspace.join("README.md"), "текст").unwrap();
        let not_bsl = refuse(Some(""), "README.md");
        assert_eq!(not_bsl["error"], "not_in_workspace", "{not_bsl}");
    }

    /// Publishing children on regions alone loses nothing only while nothing else has any.
    /// Stage B removed method-local regions from the map, and this is what keeps that true:
    /// the fixture declares one inside a method body.
    #[test]
    fn only_a_region_ever_holds_children_in_the_map() {
        let (_dir, workspace) = stand(SHAPED_MODULE);
        let roots = roots_of(&workspace);
        let abs = workspace.join(MODULE_REL);
        let text = std::fs::read_to_string(&abs).unwrap();
        let _ = &roots;

        let (db, file_id) = single_file_db(&abs, &text);
        let analysis = Analysis::from_database(db);
        let symbols = analysis.file_outline(file_id, OutlineMode::Full);

        fn check(symbols: &[DocumentSymbol]) {
            for symbol in symbols {
                if symbol.detail != SymbolDetail::Region {
                    assert!(
                        symbol.children.is_empty(),
                        "{} is not a region and holds {} children the answer would drop",
                        symbol.name,
                        symbol.children.len(),
                    );
                }
                check(&symbol.children);
            }
        }
        check(&symbols);
    }

    #[test]
    fn an_unknown_mode_is_refused_rather_than_defaulted() {
        assert_eq!(parse_mode(None).unwrap(), OutlineMode::Full);
        assert_eq!(parse_mode(Some("full")).unwrap(), OutlineMode::Full);
        assert_eq!(parse_mode(Some("regions")).unwrap(), OutlineMode::RegionsOnly);
        assert!(parse_mode(Some("skeleton")).unwrap_err().contains("skeleton"));
    }

    /// Every default state names itself, and no two share a spelling.
    #[test]
    fn the_default_states_are_three_distinct_names() {
        let all = [DefaultState::Required, DefaultState::Value, DefaultState::Unknown];
        let spellings: std::collections::BTreeSet<&str> =
            all.iter().map(|state| state.as_str()).collect();
        assert_eq!(spellings.len(), all.len());
    }

    /// The length of a node as the walk prices it: without its children.
    fn shallow_len(node: &Value) -> usize {
        let mut bare = node.as_object().expect("a node").clone();
        bare.remove("regions");
        bare.remove("members");
        serde_json::to_string(&Value::Object(bare)).unwrap().len()
    }

    fn strip_members(mut nodes: Value) -> Value {
        for node in nodes.as_array_mut().expect("an array") {
            let map = node.as_object_mut().expect("a node");
            map.remove("members");
            let children = map.remove("regions").unwrap_or(json!([]));
            map.insert("regions".into(), strip_members(children));
        }
        nodes
    }

    fn walk_nodes(nodes: &Value) -> Vec<&Value> {
        let mut out = Vec::new();
        for node in nodes.as_array().into_iter().flatten() {
            out.push(node);
            for key in ["regions", "members"] {
                if let Some(children) = node.get(key) {
                    out.extend(walk_nodes(children));
                }
            }
        }
        out
    }
}
