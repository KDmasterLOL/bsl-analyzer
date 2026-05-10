//! RegionTree - hierarchical representation of #Область/#Region directives.
//!
//! RegionTree provides a structured view of preprocessor regions in a BSL module.
//! It is separate from ItemTree to maintain clean separation of concerns:
//! - ItemTree = semantic structure (procedures, functions, variables)
//! - RegionTree = organizational structure (regions for code folding and navigation)
//!
//! ## Architecture
//!
//! ```text
//! AST (syntax) → RegionTree (hir-def) → Diagnostics + IDE Features
//!                     │
//!                     ├── regions: Arena<RegionData>
//!                     ├── root_regions: Vec<RegionIdx>
//!                     └── API: region_at(), parent(), children()
//! ```
//!
//! ## Performance
//!
//! RegionTree is cached via Salsa and only recomputed when file content changes.
//! The structure uses `la_arena` for efficient indexing.

use la_arena::{Arena, Idx};
use rustc_hash::FxHashMap;
use syntax::{ast, ast::AstNode, SyntaxKind, SyntaxNode};
use text_size::TextRange;

use crate::Name;

/// Index into RegionTree's arena.
pub type RegionIdx = Idx<RegionData>;

/// Data about a single region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionData {
    /// Region name (e.g., "ПрограммныйИнтерфейс", "Public").
    pub name: Name,

    /// Full range of the region (from #Область to #КонецОбласти).
    pub range: TextRange,

    /// Track 2 Phase C §3 Slice 2: range of just the directive line
    /// (`#Область ИмяОбласти` / `#Region Public` — opening directive,
    /// excluding the body and `#КонецОбласти`). DuplicateRegion and
    /// NonStandardRegion emit on this geometry.
    pub directive_range: TextRange,

    /// Range of just the region name (for renaming, highlighting).
    pub name_range: TextRange,

    /// Parent region index (None for top-level regions).
    pub parent: Option<RegionIdx>,

    /// Child region indices.
    pub children: Vec<RegionIdx>,

    /// Depth in the region hierarchy (0 for top-level).
    pub depth: u32,

    /// Track 2 Phase C §3 Slice 2: `true` when the region sits inside
    /// a procedure or function body (i.e. its containing syntax has
    /// crossed a `PROCEDURE_DEF`/`FUNCTION_DEF` boundary). Module-level
    /// region diagnostics filter these out via
    /// [`RegionTree::module_level_regions`].
    pub is_method_local: bool,

    /// Track 2 Phase C §3.4: `true` when the region contains only
    /// comments / whitespace / nested empty regions — i.e. nothing
    /// meaningful. Computed at AST→`RegionTree` lowering time using
    /// the same logic the retired
    /// `body/lower/preproc::is_empty_region` helper used. The §3.4
    /// `EmptyRegion` migration consumes this through
    /// [`RegionTree::is_region_empty`] — handler reads the bit
    /// instead of re-walking the AST.
    pub is_empty: bool,
}

/// Hierarchical tree of regions in a module.
///
/// Provides O(1) access to regions by index and O(log n) lookup by position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionTree {
    /// All regions in the module.
    regions: Arena<RegionData>,

    /// Top-level regions (not nested in any other region).
    root_regions: Vec<RegionIdx>,

    /// Map from text position to containing region.
    /// Key is the start of the region's range.
    position_map: FxHashMap<u32, RegionIdx>,
}

impl Default for RegionTree {
    fn default() -> Self {
        Self::new()
    }
}

impl RegionTree {
    /// Create an empty RegionTree.
    pub fn new() -> Self {
        Self { regions: Arena::new(), root_regions: Vec::new(), position_map: FxHashMap::default() }
    }

    /// Get all regions.
    pub fn regions(&self) -> impl Iterator<Item = (RegionIdx, &RegionData)> {
        self.regions.iter()
    }

    /// Get number of regions.
    pub fn len(&self) -> usize {
        self.regions.len()
    }

    /// Check if there are no regions.
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// Get top-level regions.
    pub fn root_regions(&self) -> &[RegionIdx] {
        &self.root_regions
    }

    /// Track 2 Phase C §3 Slice 2: iterate the regions that sit at
    /// module level — i.e. the roots that are NOT inside any
    /// procedure / function body. Used by `DuplicateRegion` and
    /// `NonStandardRegion`, which only care about the module's
    /// top-level region structure. Method-local regions remain
    /// available through [`RegionTree::regions`] /
    /// [`RegionTree::root_regions`] for handlers that want them
    /// (e.g. `EmptyRegion`).
    pub fn module_level_regions(&self) -> impl Iterator<Item = RegionIdx> + '_ {
        self.root_regions.iter().copied().filter(move |&idx| !self.regions[idx].is_method_local)
    }

    /// Track 2 Phase C §3.4: `true` when the region at `idx` is
    /// "empty" — contains only comments / whitespace / nested empty
    /// regions. Used by the §3.4 `EmptyRegion` handler to replace the
    /// retired `body/lower/preproc::is_empty_region` AST walk with a
    /// pre-computed RegionTree bit.
    pub fn is_region_empty(&self, idx: RegionIdx) -> bool {
        self.regions[idx].is_empty
    }

    /// Get a region by its index.
    pub fn region(&self, idx: RegionIdx) -> &RegionData {
        &self.regions[idx]
    }

    /// Get parent region.
    pub fn parent(&self, idx: RegionIdx) -> Option<RegionIdx> {
        self.regions[idx].parent
    }

    /// Get child regions.
    pub fn children(&self, idx: RegionIdx) -> &[RegionIdx] {
        &self.regions[idx].children
    }

    /// Find region containing a text position.
    ///
    /// Returns the innermost (most specific) region containing the position.
    pub fn region_at(&self, offset: text_size::TextSize) -> Option<RegionIdx> {
        // Find the innermost region containing this offset
        let mut best: Option<(RegionIdx, u32)> = None; // (idx, depth)

        for (idx, region) in self.regions.iter() {
            if region.range.contains(offset) {
                match best {
                    None => best = Some((idx, region.depth)),
                    Some((_, best_depth)) if region.depth > best_depth => {
                        best = Some((idx, region.depth));
                    }
                    _ => {}
                }
            }
        }

        best.map(|(idx, _)| idx)
    }

    /// Find region containing a text range.
    ///
    /// Returns the innermost region that fully contains the range.
    pub fn region_containing(&self, range: TextRange) -> Option<RegionIdx> {
        let mut best: Option<(RegionIdx, u32)> = None;

        for (idx, region) in self.regions.iter() {
            if region.range.contains_range(range) {
                match best {
                    None => best = Some((idx, region.depth)),
                    Some((_, best_depth)) if region.depth > best_depth => {
                        best = Some((idx, region.depth));
                    }
                    _ => {}
                }
            }
        }

        best.map(|(idx, _)| idx)
    }

    /// Check if a position is inside any region.
    pub fn is_inside_region(&self, offset: text_size::TextSize) -> bool {
        self.region_at(offset).is_some()
    }

    /// Check if a range is fully inside any region.
    pub fn is_range_inside_region(&self, range: TextRange) -> bool {
        self.region_containing(range).is_some()
    }

    /// Get all regions with a specific name (case-insensitive).
    pub fn regions_by_name(&self, name: &str) -> Vec<RegionIdx> {
        let name_lower = name.to_lowercase();
        self.regions
            .iter()
            .filter(|(_, r)| r.name.as_str().to_lowercase() == name_lower)
            .map(|(idx, _)| idx)
            .collect()
    }

    /// Get region names at root level.
    pub fn root_region_names(&self) -> Vec<&str> {
        self.root_regions.iter().map(|&idx| self.regions[idx].name.as_str()).collect()
    }

    /// Get the root ancestor of a region (depth=0).
    ///
    /// Follows parent chain until reaching a top-level region.
    /// If the region is already at depth 0, returns the same index.
    pub fn root_ancestor(&self, idx: RegionIdx) -> RegionIdx {
        let mut current = idx;
        while let Some(parent) = self.regions[current].parent {
            current = parent;
        }
        current
    }

    /// Check if a region name is an API region.
    ///
    /// API regions are: ПрограммныйИнтерфейс, Public, СлужебныйПрограммныйИнтерфейс, Internal.
    pub fn is_api_region_name(name: &str) -> bool {
        const API_REGIONS: &[&str] =
            &["программныйинтерфейс", "public", "служебныйпрограммныйинтерфейс", "internal"];
        API_REGIONS.contains(&name.to_lowercase().as_str())
    }

    /// Find the root API region containing a text range, if any.
    ///
    /// Returns the root region (depth=0) name if the range is inside an API region.
    pub fn root_api_region_for_range(&self, range: TextRange) -> Option<&str> {
        let region_idx = self.region_containing(range)?;
        let root_idx = self.root_ancestor(region_idx);
        let root_region = &self.regions[root_idx];

        if Self::is_api_region_name(root_region.name.as_str()) {
            Some(root_region.name.as_str())
        } else {
            None
        }
    }
}

/// Builder for constructing RegionTree from AST.
struct RegionTreeBuilder {
    tree: RegionTree,
    /// Stack of parent regions during traversal.
    parent_stack: Vec<RegionIdx>,
    /// Track 2 Phase C §3 Slice 2: depth of the enclosing
    /// procedure / function chain. Incremented on `PROCEDURE_DEF` /
    /// `FUNCTION_DEF` entry; any region recorded while
    /// `procedure_depth > 0` is flagged `is_method_local = true`.
    procedure_depth: u32,
}

impl RegionTreeBuilder {
    fn new() -> Self {
        Self { tree: RegionTree::new(), parent_stack: Vec::new(), procedure_depth: 0 }
    }

    fn build(mut self, root: &SyntaxNode) -> RegionTree {
        self.descend(root);
        self.tree
    }

    /// Single descent function used both at the top level and for the
    /// interior of a `PRE_REGION_DIR` (nested regions). Tracks
    /// procedure-body containment so method-local regions can be
    /// distinguished from module-level ones.
    ///
    /// Walks **all** descendant nodes by default so a region nested
    /// arbitrarily deep — for example inside a control-flow body
    /// (`Если` / `Пока` / `Для` / `Попытка`) inside a procedure — is
    /// still picked up. Two structural cases get special treatment:
    /// - `PRE_REGION_DIR` is consumed by [`Self::process_region`]
    ///   (which performs its own recursion through `descend(node)`),
    ///   so we do NOT also recurse here — that would visit the
    ///   region's children twice.
    /// - `PROCEDURE_DEF` / `FUNCTION_DEF` bracket the recursion with
    ///   `procedure_depth +/- 1` so anything recorded inside is
    ///   classified `is_method_local = true`.
    fn descend(&mut self, node: &SyntaxNode) {
        for child in node.children() {
            match child.kind() {
                SyntaxKind::PRE_REGION_DIR => {
                    self.process_region(&child);
                }
                SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF => {
                    self.procedure_depth += 1;
                    self.descend(&child);
                    self.procedure_depth -= 1;
                }
                // Default: recurse into every other child. Regions
                // appear at statement positions, and statements live
                // inside `STMT_LIST` / preprocessor conditionals /
                // control-flow nodes — none of which need to be
                // enumerated explicitly. Walking expression subtrees
                // is harmless because they contain no
                // `PRE_REGION_DIR` nodes.
                _ => {
                    self.descend(&child);
                }
            }
        }
    }

    fn process_region(&mut self, node: &SyntaxNode) {
        let region_ast = match ast::PreRegionDir::cast(node.clone()) {
            Some(r) => r,
            None => return,
        };

        // Get region name
        let name = match region_ast.name() {
            Some(n) if !n.is_empty() => Name::new(&n),
            _ => return, // Skip regions without names
        };

        // Calculate ranges
        let range = node.text_range();
        let directive_range = first_line_range(node);

        // Name range: find the IDENT token after #Область/#Region
        let name_range = self.find_name_range(node, &name);

        // Get parent and depth
        let parent = self.parent_stack.last().copied();
        let depth = self.parent_stack.len() as u32;

        // Track 2 Phase C §3.4: classify region emptiness at
        // construction time using the same AST predicate the retired
        // `body/lower/preproc::is_empty_region` used.
        let is_empty = is_region_node_empty(node);

        // Allocate region
        let region_idx = self.tree.regions.alloc(RegionData {
            name,
            range,
            directive_range,
            name_range,
            parent,
            children: Vec::new(),
            depth,
            is_method_local: self.procedure_depth > 0,
            is_empty,
        });

        // Add to position map
        self.tree.position_map.insert(range.start().into(), region_idx);

        // Add to parent's children or root_regions
        if let Some(parent_idx) = parent {
            self.tree.regions[parent_idx].children.push(region_idx);
        } else {
            self.tree.root_regions.push(region_idx);
        }

        // Push onto stack and recurse for nested children. Sharing
        // `descend` ensures procedures / functions inside a region
        // continue to be classified as a method-local boundary.
        self.parent_stack.push(region_idx);
        self.descend(node);
        self.parent_stack.pop();
    }

    fn find_name_range(&self, node: &SyntaxNode, name: &Name) -> TextRange {
        // Look for IDENT or PRE_REGION token that matches the name
        for token in node.children_with_tokens().filter_map(|e| e.into_token()) {
            if token.kind() == SyntaxKind::IDENT || token.kind() == SyntaxKind::PRE_REGION {
                let text = token.text();
                // Skip the directive keyword itself
                if text.starts_with('#') {
                    continue;
                }
                if text.trim() == name.as_str() {
                    return token.text_range();
                }
            }
        }

        // Fallback: use first line range
        first_line_range(node)
    }
}

/// Track 2 Phase C §3 Slice 2: shared first-line geometry helper. The
/// `RegionData::directive_range` field and `find_name_range`'s fallback
/// both want the range of the opening line of `node` — the
/// `#Область ИмяОбласти` directive (and any whitespace/comment
/// preceding the next newline). Centralised here so the byte-range
/// arithmetic doesn't drift between callers.
fn first_line_range(node: &SyntaxNode) -> TextRange {
    let text = node.text().to_string();
    let first_line_len = text.lines().next().map(str::len).unwrap_or(0);
    TextRange::new(
        node.text_range().start(),
        node.text_range().start() + text_size::TextSize::from(first_line_len as u32),
    )
}

/// Track 2 Phase C §3.4: classify whether a `PRE_REGION_DIR` node
/// holds only comments / whitespace / nested empty regions. Mirrors
/// the retired `body/lower/preproc::is_empty_region` predicate; lifted
/// into `region_tree` so the `RegionTree` consumers (the §3.4
/// `EmptyRegion` handler in particular) can read the result through
/// `RegionData::is_empty` instead of re-walking the AST.
fn is_region_node_empty(region_node: &SyntaxNode) -> bool {
    for child in region_node.children() {
        if is_meaningful_region_content(&child) {
            return false;
        }
        if child.kind() == SyntaxKind::PRE_REGION_DIR && !is_region_node_empty(&child) {
            return false;
        }
    }
    true
}

/// Statement kinds that count as "meaningful" content for the purpose
/// of the §3.4 `EmptyRegion` classification — anything that lowers
/// to a HIR statement or declaration the user expects to find inside
/// a region.
///
/// Track 2 Phase C §3 Slice 1: delegates to the shared
/// [`crate::module_structure::significant::is_significant_for_region_emptiness`]
/// predicate. The membership matches what was historically inlined
/// here.
fn is_meaningful_region_content(node: &SyntaxNode) -> bool {
    crate::module_structure::significant::is_significant_for_region_emptiness(node.kind())
}

/// Lower AST to RegionTree.
///
/// This is the main entry point for RegionTree construction.
pub fn lower_regions(root: &SyntaxNode) -> RegionTree {
    RegionTreeBuilder::new().build(root)
}

/// Salsa tracked query for RegionTree construction.
///
/// This query is automatically cached and invalidated by Salsa when file content changes.
///
/// ## Performance
/// - LRU: 256 files (lightweight tree, used by many diagnostics)
/// - Depends on: parse (via FileIdInput)
/// - Invalidation: Automatic when file text changes
///
/// ## Usage
/// ```ignore
/// // In DefDatabase implementation:
/// fn region_tree(&self, file_id: FileId) -> Arc<RegionTree> {
///     let file_id_input = base_db::FileIdInput::new(self, file_id);
///     hir_def::region_tree_query(self, file_id_input)
/// }
/// ```
#[salsa::tracked(lru = 256)]
pub fn region_tree_query<'db>(
    db: &'db dyn base_db::RootQueryDb,
    file_id_input: base_db::FileIdInput<'db>,
) -> std::sync::Arc<RegionTree> {
    let _span = tracing::info_span!("region_tree", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let parse = db.parse(file_id);
    std::sync::Arc::new(lower_regions(&parse.syntax_node()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_lower(code: &str) -> RegionTree {
        let parse = parser::parse(code);
        lower_regions(&parse.syntax_node())
    }

    #[test]
    fn test_empty_file() {
        let tree = parse_and_lower("");
        assert!(tree.is_empty());
        assert_eq!(tree.root_regions().len(), 0);
    }

    #[test]
    fn test_single_region() {
        let code = r#"
#Область ПрограммныйИнтерфейс

Процедура Тест()
КонецПроцедуры

#КонецОбласти
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree.root_regions().len(), 1);

        let region = tree.region(tree.root_regions()[0]);
        assert_eq!(region.name.as_str(), "ПрограммныйИнтерфейс");
        assert_eq!(region.depth, 0);
        assert!(region.parent.is_none());
        assert!(region.children.is_empty());
    }

    #[test]
    fn test_multiple_regions() {
        let code = r#"
#Область ПрограммныйИнтерфейс
Процедура Тест1()
КонецПроцедуры
#КонецОбласти

#Область СлужебныеПроцедурыИФункции
Процедура Тест2()
КонецПроцедуры
#КонецОбласти
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 2);
        assert_eq!(tree.root_regions().len(), 2);

        let names: Vec<_> = tree.root_region_names();
        assert!(names.contains(&"ПрограммныйИнтерфейс"));
        assert!(names.contains(&"СлужебныеПроцедурыИФункции"));
    }

    #[test]
    fn test_nested_regions() {
        let code = r#"
#Область Внешняя
    #Область Внутренняя
    Процедура Тест()
    КонецПроцедуры
    #КонецОбласти
#КонецОбласти
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 2);
        assert_eq!(tree.root_regions().len(), 1);

        let outer_idx = tree.root_regions()[0];
        let outer = tree.region(outer_idx);
        assert_eq!(outer.name.as_str(), "Внешняя");
        assert_eq!(outer.depth, 0);
        assert_eq!(outer.children.len(), 1);

        let inner_idx = outer.children[0];
        let inner = tree.region(inner_idx);
        assert_eq!(inner.name.as_str(), "Внутренняя");
        assert_eq!(inner.depth, 1);
        assert_eq!(inner.parent, Some(outer_idx));
    }

    #[test]
    fn test_region_at() {
        let code = r#"
#Область Тест
Процедура Тест()
КонецПроцедуры
#КонецОбласти
"#;
        let tree = parse_and_lower(code);

        // Position inside the region
        let inside_pos = text_size::TextSize::from(20);
        assert!(tree.region_at(inside_pos).is_some());

        // Position at the very start (before #Область)
        let before_pos = text_size::TextSize::from(0);
        assert!(tree.region_at(before_pos).is_none());
    }

    #[test]
    fn test_is_inside_region() {
        let code = r#"
Перем А;

#Область Тест
Процедура Тест()
КонецПроцедуры
#КонецОбласти
"#;
        let tree = parse_and_lower(code);

        // Position of Перем А (outside region)
        let outside_pos = text_size::TextSize::from(5);
        assert!(!tree.is_inside_region(outside_pos));

        // Position inside the region
        let inside_pos = text_size::TextSize::from(40);
        assert!(tree.is_inside_region(inside_pos));
    }

    #[test]
    fn test_regions_by_name() {
        let code = r#"
#Область Тест
#КонецОбласти

#Область тест
#КонецОбласти

#Область Другой
#КонецОбласти
"#;
        let tree = parse_and_lower(code);

        // Case-insensitive search
        let test_regions = tree.regions_by_name("ТЕСТ");
        assert_eq!(test_regions.len(), 2);

        let other_regions = tree.regions_by_name("Другой");
        assert_eq!(other_regions.len(), 1);
    }

    #[test]
    fn test_english_regions() {
        let code = r#"
#Region Public
Procedure Test() Export
EndProcedure
#EndRegion

#Region Private
Procedure Internal()
EndProcedure
#EndRegion
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 2);
        let names: Vec<_> = tree.root_region_names();
        assert!(names.contains(&"Public"));
        assert!(names.contains(&"Private"));
    }

    #[test]
    fn test_region_containing_range() {
        let code = r#"
#Область Внешняя
    #Область Внутренняя
    Процедура Тест()
    КонецПроцедуры
    #КонецОбласти
#КонецОбласти
"#;
        let tree = parse_and_lower(code);

        // Find innermost region containing "Процедура Тест()"
        // The procedure is around position 50-70
        let proc_range = TextRange::new(50.into(), 70.into());
        let region_idx = tree.region_containing(proc_range);
        assert!(region_idx.is_some());

        let region = tree.region(region_idx.unwrap());
        assert_eq!(region.name.as_str(), "Внутренняя");
    }

    #[test]
    fn test_root_ancestor() {
        let code = r#"
#Область Внешняя
    #Область Внутренняя
        #Область ГлубокоВнутри
        Процедура Тест()
        КонецПроцедуры
        #КонецОбласти
    #КонецОбласти
#КонецОбласти
"#;
        let tree = parse_and_lower(code);

        assert_eq!(tree.len(), 3);

        // Find deepest region
        let deep_regions = tree.regions_by_name("ГлубокоВнутри");
        assert_eq!(deep_regions.len(), 1);
        let deep_idx = deep_regions[0];

        // root_ancestor should return the top-level region
        let root_idx = tree.root_ancestor(deep_idx);
        let root_region = tree.region(root_idx);
        assert_eq!(root_region.name.as_str(), "Внешняя");
        assert_eq!(root_region.depth, 0);

        // root_ancestor of root is itself
        let outer_idx = tree.root_regions()[0];
        assert_eq!(tree.root_ancestor(outer_idx), outer_idx);
    }

    #[test]
    fn test_is_api_region_name() {
        // Russian API regions
        assert!(RegionTree::is_api_region_name("ПрограммныйИнтерфейс"));
        assert!(RegionTree::is_api_region_name("программныйинтерфейс"));
        assert!(RegionTree::is_api_region_name("ПРОГРАММНЫЙИНТЕРФЕЙС"));
        assert!(RegionTree::is_api_region_name("СлужебныйПрограммныйИнтерфейс"));

        // English API regions
        assert!(RegionTree::is_api_region_name("Public"));
        assert!(RegionTree::is_api_region_name("public"));
        assert!(RegionTree::is_api_region_name("PUBLIC"));
        assert!(RegionTree::is_api_region_name("Internal"));
        assert!(RegionTree::is_api_region_name("internal"));

        // Non-API regions
        assert!(!RegionTree::is_api_region_name("СлужебныеПроцедурыИФункции"));
        assert!(!RegionTree::is_api_region_name("Private"));
        assert!(!RegionTree::is_api_region_name("Инициализация"));
        assert!(!RegionTree::is_api_region_name("ОбработчикиСобытий"));
    }

    #[test]
    fn test_root_api_region_for_range_api() {
        let code = r#"
#Область ПрограммныйИнтерфейс
    #Область Вложенная
    Процедура ВложеннаяПроцедура()
    КонецПроцедуры
    #КонецОбласти
#КонецОбласти
"#;
        let tree = parse_and_lower(code);

        // Find position of "ВложеннаяПроцедура" in the code
        let proc_start = code.find("Процедура Вложенная").unwrap() as u32;
        let proc_end = code.find("КонецПроцедуры").unwrap() as u32;
        let api_range = TextRange::new(proc_start.into(), proc_end.into());

        let api_region = tree.root_api_region_for_range(api_range);
        assert!(api_region.is_some());
        assert_eq!(api_region.unwrap(), "ПрограммныйИнтерфейс");
    }

    #[test]
    fn test_root_api_region_for_range_non_api() {
        let code = r#"
#Область СлужебныеПроцедурыИФункции
Процедура Служебная()
КонецПроцедуры
#КонецОбласти
"#;
        let tree = parse_and_lower(code);

        // Find position of "Служебная" procedure
        let proc_start = code.find("Процедура Служебная").unwrap() as u32;
        let proc_end = code.find("КонецПроцедуры").unwrap() as u32;
        let non_api_range = TextRange::new(proc_start.into(), proc_end.into());

        let non_api_region = tree.root_api_region_for_range(non_api_range);
        assert!(non_api_region.is_none(), "Non-API region should return None");
    }

    // -- Track 2 Phase C §3 Slice 2 ----------------------------------------

    #[test]
    fn test_directive_range_is_first_line_of_region() {
        let code =
            "#Область ПрограммныйИнтерфейс\nПроцедура Тест()\nКонецПроцедуры\n#КонецОбласти\n";
        let tree = parse_and_lower(code);

        let region = tree.region(tree.root_regions()[0]);
        let dir_text = &code[region.directive_range];
        assert_eq!(dir_text, "#Область ПрограммныйИнтерфейс");
        // The full block range covers the closing directive too.
        assert!(region.range.contains_inclusive(region.directive_range.end()));
    }

    #[test]
    fn test_module_level_region_is_not_method_local() {
        let code = "#Область Public\n#КонецОбласти\n";
        let tree = parse_and_lower(code);
        let region = tree.region(tree.root_regions()[0]);
        assert!(!region.is_method_local);
        assert_eq!(tree.module_level_regions().count(), 1);
    }

    #[test]
    fn test_region_inside_procedure_is_method_local() {
        let code = r#"
Процедура Тест()
    #Область Локальная
    Сообщить("OK");
    #КонецОбласти
КонецПроцедуры
"#;
        let tree = parse_and_lower(code);
        assert_eq!(tree.len(), 1, "the local region must be captured");
        let region = tree.region(tree.root_regions()[0]);
        assert!(region.is_method_local, "region inside Procedure body should be method-local");
        assert_eq!(
            tree.module_level_regions().count(),
            0,
            "module_level_regions() must filter method-local out"
        );
    }

    #[test]
    fn test_region_inside_function_is_method_local() {
        let code = r#"
Функция Тест()
    #Область Локальная
    Возврат 1;
    #КонецОбласти
КонецФункции
"#;
        let tree = parse_and_lower(code);
        assert_eq!(tree.len(), 1);
        assert!(tree.region(tree.root_regions()[0]).is_method_local);
    }

    #[test]
    fn test_module_level_outer_with_method_local_inner() {
        // Codex round-3 MAJOR Q8: the case
        // `#Область Outer > Процедура > #Область Local` was previously
        // missed by `collect_nested_regions` (which did not descend
        // into PROCEDURE_DEF). The unified `descend` walker now records
        // Local correctly and flags it method-local while Outer stays
        // module-level.
        let code = r#"
#Область Outer
    Процедура Тест()
        #Область Local
        Сообщить("OK");
        #КонецОбласти
    КонецПроцедуры
#КонецОбласти
"#;
        let tree = parse_and_lower(code);
        assert_eq!(tree.len(), 2);

        let outer = tree.regions_by_name("Outer");
        let local = tree.regions_by_name("Local");
        assert_eq!(outer.len(), 1);
        assert_eq!(local.len(), 1);

        assert!(!tree.region(outer[0]).is_method_local, "Outer is module-level");
        assert!(tree.region(local[0]).is_method_local, "Local is method-local");

        let module_level: Vec<_> = tree.module_level_regions().collect();
        assert_eq!(module_level.len(), 1);
        assert_eq!(module_level[0], outer[0]);
    }

    #[test]
    fn test_region_inside_if_body_inside_procedure_is_method_local() {
        // Codex round-3 review of Slice 2: `descend` previously stopped
        // at the explicit kind list (`STMT_LIST` only), so a region
        // inside `Procedure > Если > Область Local` was silently
        // dropped. The unconditional descent default arm catches it.
        let code = r#"
Процедура Тест()
    Если Истина Тогда
        #Область Local
        Сообщить("OK");
        #КонецОбласти
    КонецЕсли;
КонецПроцедуры
"#;
        let tree = parse_and_lower(code);
        assert_eq!(tree.len(), 1, "region nested inside if-body must be captured");
        let region = tree.region(tree.root_regions()[0]);
        assert!(region.is_method_local);
        assert_eq!(tree.module_level_regions().count(), 0);
    }

    #[test]
    fn test_directive_range_for_method_local_region() {
        let code = "Процедура Тест()\n    #Область Local\n    Сообщить(\"OK\");\n    #КонецОбласти\nКонецПроцедуры\n";
        let tree = parse_and_lower(code);
        let region = tree.region(tree.root_regions()[0]);
        let dir_text = &code[region.directive_range];
        assert!(
            dir_text.trim_start().starts_with("#Область Local"),
            "directive_range should isolate the opening directive line, got: {dir_text:?}"
        );
    }
}
