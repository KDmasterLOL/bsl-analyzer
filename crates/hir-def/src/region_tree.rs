use la_arena::{Arena, Idx};
use rustc_hash::FxHashMap;
use stdx::case::CaseExt;
use syntax::{ast, ast::AstNode, SyntaxKind, SyntaxNode};
use text_size::TextRange;

use crate::Name;

pub type RegionIdx = Idx<RegionData>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionData {
    pub name: Name,
    pub range: TextRange,

    pub directive_range: TextRange,

    pub name_range: TextRange,
    pub parent: Option<RegionIdx>,
    pub children: Vec<RegionIdx>,
    pub depth: u32,

    pub is_method_local: bool,

    pub is_empty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionTree {
    regions: Arena<RegionData>,
    root_regions: Vec<RegionIdx>,
    position_map: FxHashMap<u32, RegionIdx>,
}

impl Default for RegionTree {
    fn default() -> Self {
        Self::new()
    }
}

impl RegionTree {
    pub fn new() -> Self {
        Self { regions: Arena::new(), root_regions: Vec::new(), position_map: FxHashMap::default() }
    }

    pub fn regions(&self) -> impl Iterator<Item = (RegionIdx, &RegionData)> {
        self.regions.iter()
    }

    pub fn len(&self) -> usize {
        self.regions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    pub fn root_regions(&self) -> &[RegionIdx] {
        &self.root_regions
    }

    pub fn module_level_regions(&self) -> impl Iterator<Item = RegionIdx> + '_ {
        self.root_regions.iter().copied().filter(move |&idx| !self.regions[idx].is_method_local)
    }

    pub fn is_region_empty(&self, idx: RegionIdx) -> bool {
        self.regions[idx].is_empty
    }

    pub fn region(&self, idx: RegionIdx) -> &RegionData {
        &self.regions[idx]
    }

    pub fn parent(&self, idx: RegionIdx) -> Option<RegionIdx> {
        self.regions[idx].parent
    }

    pub fn children(&self, idx: RegionIdx) -> &[RegionIdx] {
        &self.regions[idx].children
    }

    pub fn region_at(&self, offset: text_size::TextSize) -> Option<RegionIdx> {
        let mut best: Option<(RegionIdx, u32)> = None;

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

    pub fn is_inside_region(&self, offset: text_size::TextSize) -> bool {
        self.region_at(offset).is_some()
    }

    pub fn is_range_inside_region(&self, range: TextRange) -> bool {
        self.region_containing(range).is_some()
    }

    pub fn regions_by_name(&self, name: &str) -> Vec<RegionIdx> {
        let name_lower = name.fold_lower();
        self.regions
            .iter()
            .filter(|(_, r)| r.name.as_str().fold_lower() == name_lower)
            .map(|(idx, _)| idx)
            .collect()
    }

    pub fn root_region_names(&self) -> Vec<&str> {
        self.root_regions.iter().map(|&idx| self.regions[idx].name.as_str()).collect()
    }

    pub fn root_ancestor(&self, idx: RegionIdx) -> RegionIdx {
        let mut current = idx;
        while let Some(parent) = self.regions[current].parent {
            current = parent;
        }
        current
    }

    pub fn is_api_region_name(name: &str) -> bool {
        const API_REGIONS: &[&str] =
            &["программныйинтерфейс", "public", "служебныйпрограммныйинтерфейс", "internal"];
        API_REGIONS.contains(&name.fold_lower().as_str())
    }

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

/// A region marker still waiting for its matching `#КонецОбласти`.
///
/// `idx` is `None` for an unnamed `#Область` (not emitted into the tree, but
/// still tracked so its matching end pops correctly).
struct OpenRegion {
    idx: Option<RegionIdx>,
    start: SyntaxNode,
}

struct RegionTreeBuilder {
    tree: RegionTree,
    stack: Vec<OpenRegion>,
    /// Sorted start offsets of nodes that count as region content, used to
    /// decide region emptiness without relying on a container node.
    significant_starts: Vec<u32>,
    eof: text_size::TextSize,
}

impl RegionTreeBuilder {
    fn build(root: &SyntaxNode) -> RegionTree {
        let mut significant_starts: Vec<u32> = root
            .descendants()
            .filter(|n| {
                crate::module_structure::significant::is_significant_for_region_emptiness(n.kind())
            })
            .map(|n| significant_content_start(&n).into())
            .collect();
        significant_starts.sort_unstable();

        let mut builder = RegionTreeBuilder {
            tree: RegionTree::new(),
            stack: Vec::new(),
            significant_starts,
            eof: root.text_range().end(),
        };

        // Region directives are flat markers. Visit them in source order and
        // pair start/end via a stack, independent of syntactic nesting; this is
        // what lets a region overlap a control-flow block.
        let mut markers: Vec<SyntaxNode> =
            root.descendants().filter(|n| n.kind() == SyntaxKind::PRE_REGION_DIR).collect();
        markers.sort_by_key(|n| n.text_range().start());

        for marker in &markers {
            let Some(dir) = ast::PreRegionDir::cast(marker.clone()) else { continue };
            if dir.is_end() {
                builder.close_region(marker);
            } else {
                builder.open_region(marker, &dir);
            }
        }

        builder.finish_unpaired();
        builder.tree
    }

    fn open_region(&mut self, node: &SyntaxNode, dir: &ast::PreRegionDir) {
        let name = match dir.name() {
            Some(n) if !n.is_empty() => Name::new(&n),
            _ => {
                self.stack.push(OpenRegion { idx: None, start: node.clone() });
                return;
            }
        };

        let parent = self.stack.iter().rev().find_map(|o| o.idx);
        let depth = self.stack.iter().filter(|o| o.idx.is_some()).count() as u32;
        let directive_range = first_line_range(node);
        let name_range = find_name_range(node, &name);
        let is_method_local = enclosing_method_body_end(node).is_some();

        let region_idx = self.tree.regions.alloc(RegionData {
            name,
            range: node.text_range(),
            directive_range,
            name_range,
            parent,
            children: Vec::new(),
            depth,
            is_method_local,
            is_empty: true,
        });

        self.tree.position_map.insert(node.text_range().start().into(), region_idx);

        if let Some(parent_idx) = parent {
            self.tree.regions[parent_idx].children.push(region_idx);
        } else {
            self.tree.root_regions.push(region_idx);
        }

        self.stack.push(OpenRegion { idx: Some(region_idx), start: node.clone() });
    }

    fn close_region(&mut self, end_node: &SyntaxNode) {
        // Unpaired `#КонецОбласти` (empty stack) is ignored.
        let Some(open) = self.stack.pop() else { return };
        if let Some(idx) = open.idx {
            let range =
                TextRange::new(open.start.text_range().start(), end_node.text_range().end());
            self.tree.regions[idx].range = range;
            self.tree.regions[idx].is_empty =
                !self.has_significant_in(self.tree.regions[idx].directive_range.end(), range.end());
        }
    }

    fn finish_unpaired(&mut self) {
        // Unpaired `#Область`: extend to EOF, or to the end of the enclosing
        // method for a method-local region.
        while let Some(open) = self.stack.pop() {
            if let Some(idx) = open.idx {
                let end = enclosing_method_body_end(&open.start).unwrap_or(self.eof);
                let range = TextRange::new(
                    open.start.text_range().start(),
                    end.max(open.start.text_range().end()),
                );
                self.tree.regions[idx].range = range;
                self.tree.regions[idx].is_empty = !self
                    .has_significant_in(self.tree.regions[idx].directive_range.end(), range.end());
            }
        }
    }

    fn has_significant_in(&self, lo: text_size::TextSize, hi: text_size::TextSize) -> bool {
        if hi <= lo {
            return false;
        }
        let lo_u: u32 = lo.into();
        let hi_u: u32 = hi.into();
        let first = self.significant_starts.partition_point(|&o| o < lo_u);
        self.significant_starts.get(first).is_some_and(|&o| o < hi_u)
    }
}

/// End offset of the enclosing method, but only when the marker sits inside the
/// method's *body* (reached through a `STMT_LIST`). A marker that is merely a
/// direct child of a `PROCEDURE_DEF`/`FUNCTION_DEF` — e.g. a region directive
/// between an annotation and the `Процедура` keyword — is module-level, not
/// method-local.
fn enclosing_method_body_end(node: &SyntaxNode) -> Option<text_size::TextSize> {
    let mut saw_stmt_list = false;
    for ancestor in node.ancestors().skip(1) {
        match ancestor.kind() {
            SyntaxKind::STMT_LIST => saw_stmt_list = true,
            SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF => {
                return saw_stmt_list.then(|| ancestor.text_range().end());
            }
            _ => {}
        }
    }
    None
}

/// Start offset of a significant node's actual content, skipping any leading
/// annotations, compiler directives, and region markers. A declaration like
/// `&НаКлиенте #Область X <newline> Перем Y;` parses with the directive and the
/// region marker as leading children of the `VAR_DEF`, so the node's own start
/// is before the region; the content (`Перем`) is what determines whether the
/// region is empty.
fn significant_content_start(node: &SyntaxNode) -> text_size::TextSize {
    for element in node.children_with_tokens() {
        match element.kind() {
            SyntaxKind::WHITESPACE
            | SyntaxKind::NEWLINE
            | SyntaxKind::COMMENT
            | SyntaxKind::BOM
            | SyntaxKind::COMPILER_DIRECTIVE
            | SyntaxKind::ANNOTATION
            | SyntaxKind::PRE_REGION_DIR => continue,
            _ => return element.text_range().start(),
        }
    }
    node.text_range().start()
}

fn find_name_range(node: &SyntaxNode, name: &Name) -> TextRange {
    for token in node.children_with_tokens().filter_map(|e| e.into_token()) {
        if token.kind() == SyntaxKind::IDENT || token.kind() == SyntaxKind::PRE_REGION {
            let text = token.text();
            if text.starts_with('#') {
                continue;
            }
            if text.trim() == name.as_str() {
                return token.text_range();
            }
        }
    }

    first_line_range(node)
}

fn first_line_range(node: &SyntaxNode) -> TextRange {
    let text = node.text().to_string();
    let first_line_len = text.lines().next().map(str::len).unwrap_or(0);
    TextRange::new(
        node.text_range().start(),
        node.text_range().start() + text_size::TextSize::from(first_line_len as u32),
    )
}

pub fn lower_regions(root: &SyntaxNode) -> RegionTree {
    RegionTreeBuilder::build(root)
}

/// Approximate live heap bytes for Salsa's `memory_usage` report: the region arena
/// (one [`RegionData`] per region plus each region's `children` vector and `name`'s
/// non-inlined `SmolStr`), the `root_regions` vector, and the `position_map` table.
fn region_tree_heap(v: &std::sync::Arc<RegionTree>) -> usize {
    use crate::heap_estimate::{map_table_bytes, name_bytes, vec_bytes};

    let t = &**v;
    let mut bytes = std::mem::size_of::<RegionTree>();
    bytes += vec_bytes::<RegionData>(t.regions.len());
    for (_, region) in t.regions.iter() {
        bytes += name_bytes(&region.name);
        bytes += vec_bytes::<RegionIdx>(region.children.len());
    }
    bytes += vec_bytes::<RegionIdx>(t.root_regions.len());
    bytes += map_table_bytes::<u32, RegionIdx>(t.position_map.len());
    bytes
}

#[salsa::tracked(lru = 256, heap_size = region_tree_heap)]
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

        let inside_pos = text_size::TextSize::from(20);
        assert!(tree.region_at(inside_pos).is_some());

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

        let outside_pos = text_size::TextSize::from(5);
        assert!(!tree.is_inside_region(outside_pos));

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

        let deep_regions = tree.regions_by_name("ГлубокоВнутри");
        assert_eq!(deep_regions.len(), 1);
        let deep_idx = deep_regions[0];

        let root_idx = tree.root_ancestor(deep_idx);
        let root_region = tree.region(root_idx);
        assert_eq!(root_region.name.as_str(), "Внешняя");
        assert_eq!(root_region.depth, 0);

        let outer_idx = tree.root_regions()[0];
        assert_eq!(tree.root_ancestor(outer_idx), outer_idx);
    }

    #[test]
    fn test_is_api_region_name() {
        assert!(RegionTree::is_api_region_name("ПрограммныйИнтерфейс"));
        assert!(RegionTree::is_api_region_name("программныйинтерфейс"));
        assert!(RegionTree::is_api_region_name("ПРОГРАММНЫЙИНТЕРФЕЙС"));
        assert!(RegionTree::is_api_region_name("СлужебныйПрограммныйИнтерфейс"));

        assert!(RegionTree::is_api_region_name("Public"));
        assert!(RegionTree::is_api_region_name("public"));
        assert!(RegionTree::is_api_region_name("PUBLIC"));
        assert!(RegionTree::is_api_region_name("Internal"));
        assert!(RegionTree::is_api_region_name("internal"));

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

        let proc_start = code.find("Процедура Служебная").unwrap() as u32;
        let proc_end = code.find("КонецПроцедуры").unwrap() as u32;
        let non_api_range = TextRange::new(proc_start.into(), proc_end.into());

        let non_api_region = tree.root_api_region_for_range(non_api_range);
        assert!(non_api_region.is_none(), "Non-API region should return None");
    }

    #[test]
    fn test_directive_range_is_first_line_of_region() {
        let code =
            "#Область ПрограммныйИнтерфейс\nПроцедура Тест()\nКонецПроцедуры\n#КонецОбласти\n";
        let tree = parse_and_lower(code);

        let region = tree.region(tree.root_regions()[0]);
        let dir_text = &code[region.directive_range];
        assert_eq!(dir_text, "#Область ПрограммныйИнтерфейс");
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
    fn region_crossing_if_boundary_is_captured() {
        // Region opens before `Если` and closes inside its body, before
        // `КонецЕсли` - the ranges overlap without nesting.
        let code = "Процедура П()\n\t#Область Р\n\tЕсли Истина Тогда\n\t\tА = 1;\n\t#КонецОбласти\n\tКонецЕсли;\nКонецПроцедуры\n";
        let tree = parse_and_lower(code);
        assert_eq!(tree.len(), 1, "the crossing region must be captured");

        let region = tree.region(tree.root_regions()[0]);
        assert_eq!(region.name.as_str(), "Р");
        assert!(region.is_method_local);
        assert!(!region.is_empty, "region spans the `А = 1;` assignment");

        let start = code.find("#Область").unwrap() as u32;
        let end = code.find("#КонецОбласти").unwrap() as u32 + "#КонецОбласти".len() as u32;
        assert_eq!(region.range, TextRange::new(start.into(), end.into()));
    }

    #[test]
    fn region_markers_between_branch_and_elsif() {
        let code = "Процедура П()\n\t#Область Р1\n\tЕсли А Тогда\n\t\tБ = 1;\n\t#КонецОбласти\n\t#Область Р2\n\tИначеЕсли В Тогда\n\t\tГ = 2;\n\tКонецЕсли;\n\t#КонецОбласти\nКонецПроцедуры\n";
        let tree = parse_and_lower(code);
        assert_eq!(tree.len(), 2);
        let names = tree.root_region_names();
        assert!(names.contains(&"Р1"));
        assert!(names.contains(&"Р2"));
    }

    #[test]
    fn unpaired_start_extends_to_method_end() {
        let code = "Процедура П()\n\t#Область Р\n\tА = 1;\nКонецПроцедуры\n";
        let tree = parse_and_lower(code);
        assert_eq!(tree.len(), 1);
        let region = tree.region(tree.root_regions()[0]);
        assert!(region.is_method_local);
        let proc_end = code.find("КонецПроцедуры").unwrap() as u32 + "КонецПроцедуры".len() as u32;
        assert_eq!(region.range.end(), proc_end.into());
    }

    #[test]
    fn unpaired_start_module_level_extends_to_eof() {
        let code = "#Область Р\nПроцедура П()\nКонецПроцедуры\n";
        let tree = parse_and_lower(code);
        assert_eq!(tree.len(), 1);
        let region = tree.region(tree.root_regions()[0]);
        assert!(!region.is_method_local);
        assert_eq!(region.range.end(), text_size::TextSize::from(code.len() as u32));
    }

    #[test]
    fn unpaired_end_is_ignored() {
        let code = "Процедура П()\nКонецПроцедуры\n#КонецОбласти\n";
        let tree = parse_and_lower(code);
        assert!(tree.is_empty(), "a lone #КонецОбласти emits no region");
    }

    #[test]
    fn region_between_directive_and_var_is_module_level_and_nonempty() {
        // The region marker is a leading child of the VAR_DEF (the def spans from
        // the &НаКлиенте annotation), yet the region is module-level and contains
        // the Перем declaration.
        let code = "&НаКлиенте\n#Область ОписаниеПеременных\n\nПерем П;\n#КонецОбласти\n";
        let tree = parse_and_lower(code);
        assert_eq!(tree.len(), 1);
        let region = tree.region(tree.root_regions()[0]);
        assert_eq!(region.name.as_str(), "ОписаниеПеременных");
        assert!(!region.is_method_local, "region opened at module level");
        assert!(!region.is_empty, "region contains the Перем declaration");
    }

    #[test]
    fn region_between_directive_and_procedure_is_module_level() {
        // Region marker is a leading child of the PROCEDURE_DEF but precedes the
        // body, so it is module-level, not method-local.
        let code =
            "&НаСервере\n#Область Р\nПроцедура Тест() Экспорт\nКонецПроцедуры\n#КонецОбласти\n";
        let tree = parse_and_lower(code);
        assert_eq!(tree.len(), 1);
        let region = tree.region(tree.root_regions()[0]);
        assert!(!region.is_method_local, "region precedes the procedure body");
        assert!(!region.is_empty, "region contains the procedure");
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
