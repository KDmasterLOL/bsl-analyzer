use crate::item_tree::ModItem;
use crate::{DefDatabase, MethodId, VariableId};
use std::sync::Arc;
use stdx::case::CaseExt;
use syntax::{
    extract_leading_comments_at_offset, extract_variable_comments_at_offset, Parse, SyntaxKind,
    SyntaxNode,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodDocs {
    pub raw: String,

    pub purpose: Option<String>,

    pub parameters: Vec<ParameterDoc>,

    pub returned_value: Vec<TypeDoc>,

    pub examples: Vec<String>,

    pub call_options: Vec<String>,

    pub deprecation: Option<String>,

    pub link: Option<String>,
}

impl MethodDocs {
    pub fn empty() -> Self {
        Self {
            raw: String::new(),
            purpose: None,
            parameters: Vec::new(),
            returned_value: Vec::new(),
            examples: Vec::new(),
            call_options: Vec::new(),
            deprecation: None,
            link: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.purpose.is_none()
            && self.parameters.is_empty()
            && self.returned_value.is_empty()
            && self.examples.is_empty()
            && self.call_options.is_empty()
            && self.deprecation.is_none()
            && self.link.is_none()
    }

    pub fn is_hyperlink(&self) -> bool {
        self.link.is_some()
    }

    pub fn is_deprecated(&self) -> bool {
        self.deprecation.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterDoc {
    pub name: String,

    pub types: Vec<TypeDoc>,
}

impl ParameterDoc {
    pub fn new(name: String, types: Vec<TypeDoc>) -> Self {
        Self { name, types }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDoc {
    pub name: String,

    pub description: Option<String>,

    pub parameters: Vec<ParameterDoc>,

    pub is_hyperlink: bool,
}

impl TypeDoc {
    pub fn simple(name: String, description: Option<String>) -> Self {
        Self { name, description, parameters: Vec::new(), is_hyperlink: false }
    }

    pub fn structured(
        name: String,
        description: Option<String>,
        parameters: Vec<ParameterDoc>,
    ) -> Self {
        Self { name, description, parameters, is_hyperlink: false }
    }

    pub fn hyperlink(link: String) -> Self {
        Self { name: link, description: None, parameters: Vec::new(), is_hyperlink: true }
    }
}

pub fn method_docs_query(db: &dyn DefDatabase, method: MethodId) -> Option<Arc<MethodDocs>> {
    let tree = db.item_tree(method.module.file_id);

    let items = tree.top_level_items();
    let item = items.get(method.local_id as usize)?;

    let source_range = match item {
        ModItem::Procedure(idx) => tree.procedure(*idx).source_range,
        ModItem::Function(idx) => tree.function(*idx).source_range,
        ModItem::Variable(_) => return None,
    };

    let source_text = db.file_text(method.module.file_id);
    let offset: usize = source_range.start().into();
    let comments = extract_leading_comments_at_offset(offset, &source_text)?;

    let docs = parse_method_docs(&comments)?;

    Some(Arc::new(docs))
}

pub fn compute_method_docs(
    _parse: &syntax::Parse<SyntaxNode>,
    tree: &crate::item_tree::ItemTree,
    method_id: MethodId,
    file_text: &str,
) -> Option<Arc<MethodDocs>> {
    let items = tree.top_level_items();
    let item = items.get(method_id.local_id as usize)?;

    let source_range = match item {
        ModItem::Procedure(idx) => tree.procedure(*idx).source_range,
        ModItem::Function(idx) => tree.function(*idx).source_range,
        ModItem::Variable(_) => return None,
    };

    let offset: usize = source_range.start().into();
    let comments = extract_leading_comments_at_offset(offset, file_text)?;

    let docs = parse_method_docs(&comments)?;

    Some(Arc::new(docs))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableDocs {
    pub raw: Vec<String>,
    pub purpose: Option<String>,
    pub types: Vec<TypeDoc>,
    pub deprecation: Option<String>,
    pub link: Option<String>,
}

impl VariableDocs {
    pub fn is_hyperlink(&self) -> bool {
        self.link.is_some()
    }

    pub fn is_deprecated(&self) -> bool {
        self.deprecation.is_some()
    }
}

pub fn compute_variable_docs(
    parse: &Parse<SyntaxNode>,
    tree: &crate::item_tree::ItemTree,
    variable_id: VariableId,
    file_text: &str,
) -> Option<Arc<VariableDocs>> {
    let items = tree.top_level_items();
    let item = items.get(variable_id.local_id as usize)?;
    let var_idx = match item {
        ModItem::Variable(idx) => *idx,
        _ => return None,
    };
    let variable = tree.variable(var_idx);

    let root = parse.syntax_node();
    let var_node = root
        .descendants()
        .find(|n| n.kind() == SyntaxKind::VAR_DEF && n.text_range() == variable.source_range)?;

    compute_variable_docs_with_node(&var_node, variable, file_text)
}

pub fn compute_variable_docs_with_node(
    var_node: &SyntaxNode,
    variable: &crate::item_tree::Variable,
    file_text: &str,
) -> Option<Arc<VariableDocs>> {
    debug_assert_eq!(
        var_node.kind(),
        SyntaxKind::VAR_DEF,
        "compute_variable_docs_with_node expects a VAR_DEF node",
    );
    debug_assert_eq!(
        var_node.text_range(),
        variable.source_range,
        "var_node range must match Variable::source_range",
    );

    let var_keyword_offset: usize = var_node
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::KW_VAR)
        .map(|t| t.text_range().start().into())?;

    let var_end_offset: usize = variable.source_range.end().into();
    let first_annotation_offset: Option<usize> =
        variable.annotations.first().map(|ann| ann.range.start().into());

    let comments = extract_variable_comments_at_offset(
        file_text,
        var_keyword_offset,
        var_end_offset,
        first_annotation_offset,
    )?;

    parse_variable_docs(&comments).map(Arc::new)
}

pub fn parse_variable_docs(comments: &[String]) -> Option<VariableDocs> {
    if comments.is_empty() {
        return None;
    }

    let raw: Vec<String> = comments.to_vec();

    if let Some(first_non_empty) = comments.iter().find(|c| !c.trim().is_empty()) {
        let trimmed = first_non_empty.trim();
        if is_hyperlink_line(trimmed) {
            return Some(VariableDocs {
                raw,
                purpose: None,
                types: Vec::new(),
                deprecation: None,
                link: Some(trimmed.to_string()),
            });
        }
    }
    let has_deprecation_marker =
        comments.iter().any(|line| is_deprecated_keyword(&line.trim().fold_lower()));
    if !has_deprecation_marker {
        if let Some(last_non_empty) = comments.iter().rev().find(|c| !c.trim().is_empty()) {
            let trimmed = last_non_empty.trim();
            if is_hyperlink_line(trimmed) {
                return Some(VariableDocs {
                    raw,
                    purpose: None,
                    types: Vec::new(),
                    deprecation: None,
                    link: Some(trimmed.to_string()),
                });
            }
        }
    }

    let deprecated_idx =
        comments.iter().position(|line| is_deprecated_keyword(&line.trim().fold_lower()));

    let (purpose_lines, deprecated_slice): (&[String], &[String]) = match deprecated_idx {
        Some(idx) => (&comments[..idx], &comments[idx..]),
        None => (comments, &[]),
    };

    let purpose_collected: Vec<&str> =
        purpose_lines.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    let purpose =
        if purpose_collected.is_empty() { None } else { Some(purpose_collected.join("\n")) };

    let deprecation = match deprecated_slice.first() {
        Some(keyword_line) => parse_deprecated_section(keyword_line, &deprecated_slice[1..]),
        None => None,
    };

    Some(VariableDocs { raw, purpose, types: Vec::new(), deprecation, link: None })
}

fn parse_method_docs(comments: &[String]) -> Option<MethodDocs> {
    if comments.is_empty() {
        return None;
    }

    let raw = comments.join("\n");

    let mut docs = MethodDocs {
        raw,
        purpose: None,
        parameters: Vec::new(),
        returned_value: Vec::new(),
        examples: Vec::new(),
        call_options: Vec::new(),
        deprecation: None,
        link: None,
    };

    if let Some(first_non_empty) = comments.iter().find(|c| !c.trim().is_empty()) {
        if is_hyperlink_line(first_non_empty.trim()) {
            docs.link = Some(first_non_empty.trim().to_string());
            return Some(docs);
        }
    }
    if let Some(last_non_empty) = comments.iter().rev().find(|c| !c.trim().is_empty()) {
        if is_hyperlink_line(last_non_empty.trim()) && !has_structural_section(comments) {
            docs.link = Some(last_non_empty.trim().to_string());
            return Some(docs);
        }
    }

    let mut section_indices = Vec::new();
    for (i, line) in comments.iter().enumerate() {
        let lower = line.trim().fold_lower();

        let returns_header = returns_section_header(line.trim());
        if is_parameters_keyword(&lower) {
            section_indices.push(SectionMarker::new(i, Section::Parameters, None));
        } else if returns_header != ReturnsHeader::NotReturns {
            let inline_payload = match returns_header {
                ReturnsHeader::WithPayload(payload) => Some(payload),
                _ => None,
            };
            section_indices.push(SectionMarker::new(i, Section::Returns, inline_payload));
        } else if is_example_keyword(&lower) {
            section_indices.push(SectionMarker::new(i, Section::Examples, None));
        } else if is_call_options_keyword(&lower) {
            section_indices.push(SectionMarker::new(i, Section::CallOptions, None));
        } else if is_deprecated_keyword(&lower) {
            section_indices.push(SectionMarker::new(i, Section::Deprecated, None));
        }
    }

    let purpose_end = section_indices.first().map(|marker| marker.index).unwrap_or(comments.len());
    if purpose_end > 0 {
        let purpose_lines: Vec<_> =
            comments[..purpose_end].iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();

        if !purpose_lines.is_empty() {
            docs.purpose = Some(purpose_lines.join("\n"));
        }
    }

    for (idx, marker) in section_indices.iter().enumerate() {
        let end = section_indices
            .get(idx + 1)
            .map(|next_marker| next_marker.index)
            .unwrap_or(comments.len());

        let mut section_lines = Vec::new();
        if marker.section == Section::Returns {
            if let Some(payload) = &marker.inline_payload {
                section_lines.push(payload.clone());
            }
        }
        section_lines.extend(comments[marker.index + 1..end].iter().cloned());

        match marker.section {
            Section::Parameters => {
                docs.parameters = parse_parameters(&section_lines);
            }
            Section::Returns => {
                docs.returned_value = parse_returns(&section_lines);
            }
            Section::Examples => {
                docs.examples = parse_simple_section(&section_lines);
            }
            Section::CallOptions => {
                docs.call_options = parse_simple_section(&section_lines);
            }
            Section::Deprecated => {
                docs.deprecation =
                    parse_deprecated_section(&comments[marker.index], &section_lines);
            }
        }
    }

    Some(docs)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Parameters,
    Returns,
    Examples,
    CallOptions,
    Deprecated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SectionMarker {
    index: usize,
    section: Section,
    inline_payload: Option<String>,
}

impl SectionMarker {
    fn new(index: usize, section: Section, inline_payload: Option<String>) -> Self {
        Self { index, section, inline_payload }
    }
}

fn is_parameters_keyword(lower_line: &str) -> bool {
    lower_line.starts_with("параметры:") || lower_line.starts_with("parameters:")
}

fn has_structural_section(comments: &[String]) -> bool {
    comments.iter().any(|line| is_structural_section_line(line.trim()))
}

fn is_structural_section_line(line: &str) -> bool {
    let lower = line.fold_lower();
    is_parameters_keyword(&lower)
        || returns_section_header(line) != ReturnsHeader::NotReturns
        || is_example_keyword(&lower)
        || is_call_options_keyword(&lower)
        || is_deprecated_keyword(&lower)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReturnsHeader {
    NotReturns,
    NoPayload,
    WithPayload(String),
}

fn returns_section_header(line: &str) -> ReturnsHeader {
    let trimmed = line.trim();
    let lower = trimmed.fold_lower();

    for keyword in ["возвращаемое значение", "return value", "returns", "результат", "result"]
    {
        if !lower.starts_with(keyword) {
            continue;
        }
        let header = match parse_section_payload_after_keyword(trimmed, keyword.len()) {
            Some(header) => header,
            None => return ReturnsHeader::NotReturns,
        };
        if let ReturnsHeader::WithPayload(text) = &header {
            if is_ambiguous_returns_keyword(keyword) && !payload_looks_like_type_section(text) {
                return ReturnsHeader::NotReturns;
            }
        }
        return header;
    }

    ReturnsHeader::NotReturns
}

fn is_ambiguous_returns_keyword(keyword: &str) -> bool {
    matches!(keyword, "результат" | "result")
}

fn payload_looks_like_type_section(payload: &str) -> bool {
    let stripped = payload.trim_end_matches(['.', ',', ';', ':', '!', '?']).trim();
    if stripped.is_empty() {
        return false;
    }

    let type_part = [" -- ", " — ", " – ", " - "]
        .iter()
        .find_map(|sep| stripped.find(*sep).map(|pos| stripped[..pos].trim()))
        .unwrap_or(stripped);

    is_likely_type_name(type_part)
}

fn parse_section_payload_after_keyword(line: &str, keyword_len: usize) -> Option<ReturnsHeader> {
    let mut rest = line[keyword_len..].trim_start();

    if rest.starts_with('(') {
        let closing_paren = rest.find(')')?;
        rest = rest[closing_paren + 1..].trim_start();
    }

    if rest.is_empty() {
        return Some(ReturnsHeader::NoPayload);
    }

    if let Some(payload) = rest.strip_prefix(':') {
        return Some(returns_header_from_payload(payload));
    }

    if let Some(payload) = rest.strip_prefix('-') {
        return Some(returns_header_from_payload(payload.trim_start_matches('-')));
    }

    if let Some(payload) = rest.strip_prefix(';') {
        return Some(returns_header_from_payload(payload));
    }

    None
}

fn returns_header_from_payload(payload: &str) -> ReturnsHeader {
    let payload = payload.trim();
    if payload.is_empty() {
        ReturnsHeader::NoPayload
    } else {
        ReturnsHeader::WithPayload(payload.to_string())
    }
}

fn is_example_keyword(lower_line: &str) -> bool {
    lower_line.contains("пример:") || lower_line.contains("example:")
}

fn is_call_options_keyword(lower_line: &str) -> bool {
    lower_line.contains("варианты вызова:") || lower_line.contains("call options:")
}

fn is_deprecated_keyword(lower_line: &str) -> bool {
    lower_line.contains("устарела") || lower_line.contains("deprecated")
}

fn is_hyperlink_line(line: &str) -> bool {
    let lower = line.fold_lower();
    lower.starts_with("см.") || lower.starts_with("see ")
}

fn parse_parameters(lines: &[String]) -> Vec<ParameterDoc> {
    let mut parameters = Vec::new();
    let mut current_param: Option<(String, Vec<TypeDoc>)> = None;

    for line in lines {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('*') {
            if let Some((_, types)) = &mut current_param {
                if let Some(last_type) = types.last_mut() {
                    if let Some(sub_param) = parse_sub_parameter(trimmed) {
                        last_type.parameters.push(sub_param);
                    }
                }
            }
            continue;
        }

        if current_param.is_some() && trimmed.starts_with('-') {
            if let Some((type_name, description)) = parse_type_line(trimmed) {
                if is_likely_type_name(&type_name) {
                    if let Some((_, types)) = &mut current_param {
                        types.push(TypeDoc::simple(type_name, description));
                    }
                    continue;
                }
            }
        }

        // A multi-line union type may continue on the next line without a leading
        // `-`, e.g. `Письмо - Спр.Входящее,` then `Спр.Исходящее - текст`. A dotted
        // type reference is never a valid parameter name, so attach it to the current
        // parameter instead of registering a phantom parameter "not in the signature".
        if current_param.is_some() {
            if let Some((type_name, description)) = parse_type_line(trimmed) {
                if is_dotted_type_reference(&type_name) {
                    if let Some((_, types)) = &mut current_param {
                        types.push(TypeDoc::simple(type_name, description));
                    }
                    continue;
                }
            }
        }

        if let Some((param_name, types)) = parse_parameter_line(trimmed) {
            if let Some((name, types)) = current_param.take() {
                parameters.push(ParameterDoc { name, types });
            }
            current_param = Some((param_name, types));
        }
    }

    if let Some((name, types)) = current_param {
        parameters.push(ParameterDoc { name, types });
    }

    parameters
}

fn parse_parameter_line(line: &str) -> Option<(String, Vec<TypeDoc>)> {
    // Tabs are frequently used to align the separator (`Имя\t\t- Тип`); the split
    // below only recognizes a space-flanked dash, so normalize tabs to spaces first.
    let normalized = line.replace('\t', " ");
    let parts: Vec<&str> = normalized.splitn(3, " - ").collect();

    if parts.len() < 2 {
        return None;
    }

    let param_name = parts[0].trim().to_string();
    if !is_likely_parameter_doc_name(&param_name) {
        return None;
    }

    let type_part = parts[1].trim();
    let description = parts.get(2).map(|s| s.trim().to_string());

    let types = if type_part.contains(',') {
        type_part
            .split(',')
            .map(|t| TypeDoc::simple(t.trim().to_string(), description.clone()))
            .collect()
    } else {
        vec![TypeDoc::simple(type_part.to_string(), description)]
    };

    Some((param_name, types))
}

fn is_likely_parameter_doc_name(name: &str) -> bool {
    is_likely_parameter_name(name) || is_dotted_type_reference(name)
}

fn is_likely_parameter_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first_char) = chars.next() else {
        return false;
    };

    if !(first_char.is_alphabetic() || first_char == '_') {
        return false;
    }

    chars.all(|c| c.is_alphanumeric() || c == '_')
}

pub fn is_dotted_type_reference(name: &str) -> bool {
    name.contains('.') && is_likely_type_name(name)
}

fn parse_sub_parameter(line: &str) -> Option<ParameterDoc> {
    let without_star = line.strip_prefix('*')?.trim();
    let (name, types) = parse_parameter_line(without_star)?;
    Some(ParameterDoc { name, types })
}

fn parse_returns(lines: &[String]) -> Vec<TypeDoc> {
    let mut types = Vec::new();
    let mut current_type: Option<TypeDoc> = None;

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_hyperlink_line(trimmed) {
            return vec![TypeDoc::hyperlink(trimmed.to_string())];
        }
        break;
    }

    for line in lines {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('*') {
            if let Some(ref mut type_doc) = current_type {
                if let Some(sub_param) = parse_sub_parameter(trimmed) {
                    type_doc.parameters.push(sub_param);
                }
            }
            continue;
        }

        if let Some((type_name, description)) = parse_type_line(trimmed) {
            if let Some(type_doc) = current_type.take() {
                types.push(type_doc);
            }
            current_type = Some(TypeDoc::simple(type_name, description));
        } else if current_type.is_none() && types.is_empty() {
            let stripped = trimmed.trim_end_matches(['.', ',', ';', ':', '!', '?']).trim();
            if !stripped.is_empty() && is_likely_type_name(stripped) {
                current_type = Some(TypeDoc::simple(stripped.to_string(), None));
            }
        }
    }

    if let Some(type_doc) = current_type {
        types.push(type_doc);
    }

    types
}

fn parse_type_line(line: &str) -> Option<(String, Option<String>)> {
    // Mirror parse_parameter_line: tabs frequently flank the `-` separator on type
    // (continuation) lines; normalize them so the separator is recognized.
    let normalized = line.replace('\t', " ");
    let trimmed = normalized.trim();

    if trimmed.starts_with('*') {
        return None;
    }

    let type_line = trimmed.strip_prefix('-').map(str::trim).unwrap_or(trimmed);
    if type_line.is_empty() {
        return None;
    }

    if let Some((type_part, description)) = split_type_description(type_line) {
        let (type_name, type_description) = parse_return_type_name(type_part)?;
        return Some((
            type_name,
            merge_type_descriptions(type_description, Some(description.to_string())),
        ));
    }

    parse_return_type_name(type_line)
}

fn split_type_description(line: &str) -> Option<(&str, &str)> {
    for separator in [" -- ", " — ", " – ", " - "] {
        if let Some(separator_pos) = line.find(separator) {
            return Some((
                line[..separator_pos].trim(),
                line[separator_pos + separator.len()..].trim(),
            ));
        }
    }

    None
}

fn parse_return_type_name(type_part: &str) -> Option<(String, Option<String>)> {
    let type_part = type_part.trim().trim_end_matches(':').trim();

    if type_part.is_empty() {
        return None;
    }

    if let Some((collection_type, description)) = parse_collection_type(type_part) {
        return Some((collection_type, Some(description)));
    }

    if is_likely_type_name(type_part) {
        return Some((type_part.to_string(), None));
    }

    None
}

fn parse_collection_type(type_part: &str) -> Option<(String, String)> {
    let lower = type_part.fold_lower();
    let marker = " из ";
    let marker_pos = lower.find(marker)?;
    let collection_type = type_part[..marker_pos].trim();
    let element_type = type_part[marker_pos + marker.len()..].trim();

    if collection_type.is_empty() || element_type.is_empty() {
        return None;
    }

    if !is_likely_type_name(collection_type) {
        return None;
    }

    Some((collection_type.to_string(), format!("из {element_type}")))
}

fn merge_type_descriptions(
    type_description: Option<String>,
    explicit_description: Option<String>,
) -> Option<String> {
    match (type_description, explicit_description) {
        (Some(type_description), Some(explicit_description))
            if !explicit_description.trim().is_empty() =>
        {
            Some(format!("{type_description} - {explicit_description}"))
        }
        (Some(type_description), _) => Some(type_description),
        (None, Some(explicit_description)) => Some(explicit_description),
        (None, None) => None,
    }
}

fn is_likely_type_name(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    if !s.chars().all(is_type_name_char) {
        return false;
    }

    let first_char = s.chars().next().unwrap();
    if first_char.is_uppercase() {
        return true;
    }

    let lower = s.fold_lower();
    matches!(
        lower.as_str(),
        "строка"
            | "string"
            | "число"
            | "number"
            | "булево"
            | "boolean"
            | "дата"
            | "date"
            | "неопределено"
            | "undefined"
            | "null"
            | "произвольный"
            | "arbitrary"
            | "структура"
            | "structure"
            | "массив"
            | "array"
            | "соответствие"
            | "map"
    )
}

fn is_type_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

fn parse_simple_section(lines: &[String]) -> Vec<String> {
    lines.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string()).collect()
}

fn parse_deprecated_section(keyword_line: &str, following_lines: &[String]) -> Option<String> {
    let lower = keyword_line.fold_lower();

    let after_keyword = if let Some(pos) = lower.find("устарела") {
        &keyword_line[pos + "устарела".len()..]
    } else if let Some(pos) = lower.find("deprecated") {
        &keyword_line[pos + "deprecated".len()..]
    } else {
        ""
    };

    let info_on_same_line = after_keyword
        .trim_start_matches(|c: char| c == '.' || c == ':' || c.is_whitespace())
        .trim();

    if !info_on_same_line.is_empty() {
        return Some(info_on_same_line.to_string());
    }

    let following_info = parse_simple_section(following_lines);
    if !following_info.is_empty() {
        return Some(following_info.join("\n"));
    }

    Some(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_docs_empty() {
        let docs = MethodDocs::empty();
        assert!(docs.is_empty());
        assert!(!docs.is_hyperlink());
        assert!(!docs.is_deprecated());
    }

    #[test]
    fn test_type_doc_simple() {
        let type_doc = TypeDoc::simple("Строка".to_string(), Some("описание".to_string()));
        assert_eq!(type_doc.name, "Строка");
        assert_eq!(type_doc.description, Some("описание".to_string()));
        assert!(type_doc.parameters.is_empty());
        assert!(!type_doc.is_hyperlink);
    }

    #[test]
    fn test_type_doc_structured() {
        let params = vec![ParameterDoc::new(
            "Поле".to_string(),
            vec![TypeDoc::simple("Число".to_string(), Some("значение".to_string()))],
        )];
        let type_doc = TypeDoc::structured("Структура".to_string(), None, params);
        assert_eq!(type_doc.name, "Структура");
        assert_eq!(type_doc.parameters.len(), 1);
        assert!(!type_doc.is_hyperlink);
    }

    #[test]
    fn test_type_doc_hyperlink() {
        let type_doc = TypeDoc::hyperlink("См. ДругойМетод()".to_string());
        assert!(type_doc.is_hyperlink);
        assert_eq!(type_doc.name, "См. ДругойМетод()");
    }

    #[test]
    fn test_parse_empty_comments() {
        let docs = parse_method_docs(&[]);
        assert!(docs.is_none());
    }

    #[test]
    fn test_parse_minimal_comments() {
        let comments = vec!["Описание метода".to_string()];
        let docs = parse_method_docs(&comments);
        assert!(docs.is_some());
        let docs = docs.unwrap();
        assert_eq!(docs.raw, "Описание метода");
        assert_eq!(docs.purpose, Some("Описание метода".to_string()));
    }

    #[test]
    fn test_parse_complete_documentation() {
        let comments = vec![
            "Вычисляет сумму двух чисел.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  А - Число - первое слагаемое".to_string(),
            "  Б - Число - второе слагаемое".to_string(),
            "".to_string(),
            "Возвращаемое значение:".to_string(),
            "  Число - результат сложения".to_string(),
            "".to_string(),
            "Пример:".to_string(),
            "  Результат = Сумма(2, 3); // Результат = 5".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.purpose, Some("Вычисляет сумму двух чисел.".to_string()));

        assert_eq!(docs.parameters.len(), 2);
        assert_eq!(docs.parameters[0].name, "А");
        assert_eq!(docs.parameters[0].types[0].name, "Число");
        assert_eq!(docs.parameters[0].types[0].description, Some("первое слагаемое".to_string()));
        assert_eq!(docs.parameters[1].name, "Б");
        assert_eq!(docs.parameters[1].types[0].name, "Число");

        assert_eq!(docs.returned_value.len(), 1);
        assert_eq!(docs.returned_value[0].name, "Число");
        assert_eq!(docs.returned_value[0].description, Some("результат сложения".to_string()));

        assert_eq!(docs.examples.len(), 1);
        assert!(docs.examples[0].contains("Результат = Сумма(2, 3)"));
    }

    #[test]
    fn test_parse_structured_return_value() {
        let comments = vec![
            "Возвращает информацию о пользователе.".to_string(),
            "".to_string(),
            "Возвращаемое значение:".to_string(),
            "  Структура:".to_string(),
            "    * Имя - Строка - имя пользователя".to_string(),
            "    * Возраст - Число - возраст пользователя".to_string(),
            "    * Email - Строка - адрес электронной почты".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.returned_value.len(), 1);
        assert_eq!(docs.returned_value[0].name, "Структура");

        let sub_params = &docs.returned_value[0].parameters;
        assert_eq!(sub_params.len(), 3);
        assert_eq!(sub_params[0].name, "Имя");
        assert_eq!(sub_params[0].types[0].name, "Строка");
        assert_eq!(sub_params[1].name, "Возраст");
        assert_eq!(sub_params[2].name, "Email");
    }

    #[test]
    fn test_parse_return_value_variants_header() {
        let comments = vec![
            "Возвращает дату начала периода.".to_string(),
            "".to_string(),
            "Возвращаемое значение (варианты):".to_string(),
            "  Дата - дата начала периода.".to_string(),
            "  Неопределено - если период не применим.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.returned_value.len(), 2);
        assert_eq!(docs.returned_value[0].name, "Дата");
        assert_eq!(docs.returned_value[1].name, "Неопределено");
    }

    #[test]
    fn test_parse_return_value_english_header() {
        let comments = vec![
            "Calculates total amount.".to_string(),
            "".to_string(),
            "Return value:".to_string(),
            "  Number - total amount.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.returned_value.len(), 1);
        assert_eq!(docs.returned_value[0].name, "Number");
    }

    #[test]
    fn test_parse_return_value_inline_dash_header() {
        let comments = vec![
            "Возвращает соответствие настроек.".to_string(),
            "".to_string(),
            "Возвращаемое значение - соответствие.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.returned_value.len(), 1);
        assert_eq!(docs.returned_value[0].name, "соответствие");
        assert_eq!(docs.returned_value[0].description, None);
    }

    #[test]
    fn test_parse_result_freetext_is_not_returns_section() {
        let comments =
            vec!["Описание метода.".to_string(), "Результат: упрощает работу.".to_string()];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(
            docs.returned_value.is_empty(),
            "Free-text \"Результат: ...\" must not be detected as a Returns section, got: {:?}",
            docs.returned_value
        );
        let purpose = docs.purpose.as_deref().unwrap_or("");
        assert!(
            purpose.contains("упрощает работу"),
            "Expected purpose to include the prose, got: {purpose:?}"
        );
    }

    #[test]
    fn test_parse_english_result_freetext_is_not_returns_section() {
        let comments =
            vec!["Method description.".to_string(), "Result: simplifies the workflow.".to_string()];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(
            docs.returned_value.is_empty(),
            "Free-text \"Result: ...\" must not be detected as a Returns section, got: {:?}",
            docs.returned_value
        );
        let purpose = docs.purpose.as_deref().unwrap_or("");
        assert!(
            purpose.contains("simplifies the workflow"),
            "Expected purpose to include the prose, got: {purpose:?}"
        );
    }

    #[test]
    fn test_parse_return_collection_of_structure() {
        let comments = vec![
            "Возвращает список правил.".to_string(),
            "".to_string(),
            "Возвращаемое значение:".to_string(),
            "  Массив из Структура:".to_string(),
            "    * Ссылка - СправочникСсылка.Правила - правило.".to_string(),
            "    * Представление - Строка - представление правила.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.returned_value.len(), 1);
        assert_eq!(docs.returned_value[0].name, "Массив");
        assert_eq!(docs.returned_value[0].description.as_deref(), Some("из Структура"));
        assert_eq!(docs.returned_value[0].parameters.len(), 2);
    }

    #[test]
    fn test_parse_return_map_of_key_and_value() {
        let comments = vec![
            "Возвращает шаблоны выражений.".to_string(),
            "".to_string(),
            "Возвращаемое значение:".to_string(),
            "  Соответствие из КлючИЗначение:".to_string(),
            "    * Ключ - Строка - имя шаблона.".to_string(),
            "    * Значение - Строка - выражение на встроенном языке.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.returned_value.len(), 1);
        assert_eq!(docs.returned_value[0].name, "Соответствие");
        assert_eq!(docs.returned_value[0].description.as_deref(), Some("из КлючИЗначение"));
        assert_eq!(docs.returned_value[0].parameters.len(), 2);
    }

    #[test]
    fn test_parse_return_collection_of_see_reference() {
        let comments = vec![
            "Возвращает хранимые файлы.".to_string(),
            "".to_string(),
            "Возвращаемое значение:".to_string(),
            "  Массив из см. РаботаСФайлами.ДанныеФайла".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.returned_value.len(), 1);
        assert_eq!(docs.returned_value[0].name, "Массив");
        assert_eq!(
            docs.returned_value[0].description.as_deref(),
            Some("из см. РаботаСФайлами.ДанныеФайла")
        );
    }

    #[test]
    fn test_parse_return_structure_double_dash() {
        let comments = vec![
            "Возвращает результат фонового задания.".to_string(),
            "".to_string(),
            "Возвращаемое значение:".to_string(),
            "  Структура -- содержит следующие параметры:".to_string(),
            "    * ЗаданиеВыполнено - Булево - Истина, если задание выполнено.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.returned_value.len(), 1);
        assert_eq!(docs.returned_value[0].name, "Структура");
        assert_eq!(
            docs.returned_value[0].description.as_deref(),
            Some("содержит следующие параметры:")
        );
        assert_eq!(docs.returned_value[0].parameters.len(), 1);
    }

    #[test]
    fn test_parse_returns_does_not_swallow_description_continuation() {
        let comments = vec![
            "Возвращает значения реквизита.".to_string(),
            "".to_string(),
            "Возвращаемое значение:".to_string(),
            "  Произвольный - если передана пустая ссылка, возвращается Неопределено.".to_string(),
            "                 Если передана ссылка несуществующего объекта (битая ссылка),"
                .to_string(),
            "                 то возвращается Неопределено.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(
            docs.returned_value.len(),
            1,
            "Description continuation must not be parsed as additional return types, got: {:?}",
            docs.returned_value
        );
        assert_eq!(docs.returned_value[0].name, "Произвольный");
    }

    #[test]
    fn test_parse_hyperlink() {
        let comments = vec!["См. ДругойМетод()".to_string()];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(docs.is_hyperlink());
        assert_eq!(docs.link, Some("См. ДругойМетод()".to_string()));
    }

    #[test]
    fn test_parse_hyperlink_with_service_prefix() {
        let comments = vec![
            "СтандартныеПодсистемы.УправлениеДоступом".to_string(),
            "".to_string(),
            "См. УправлениеДоступомПереопределяемый.ПриЗаполненииСписковСОграничениемДоступа."
                .to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(docs.is_hyperlink());
        assert_eq!(
            docs.link.as_deref(),
            Some(
                "См. УправлениеДоступомПереопределяемый.ПриЗаполненииСписковСОграничениемДоступа."
            )
        );
    }

    #[test]
    fn test_parse_result_section_ends_parameters() {
        let comments = vec![
            "Для переданной организации определяет, является ли она юридическим лицом".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  Организация - СправочникСсылка.Организации - организация.".to_string(),
            "".to_string(),
            "Результат:".to_string(),
            "  Булево - Истина, если организация - юридическое лицо.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 1);
        assert_eq!(docs.parameters[0].name, "Организация");
        assert_eq!(docs.returned_value.len(), 1);
        assert_eq!(docs.returned_value[0].name, "Булево");
    }

    #[test]
    fn test_parse_english_result_section_ends_parameters() {
        let comments = vec![
            "Checks whether the organization is legal entity.".to_string(),
            "".to_string(),
            "Parameters:".to_string(),
            "  Organization - CatalogRef.Organizations - organization.".to_string(),
            "".to_string(),
            "Result:".to_string(),
            "  Boolean - true when organization is legal entity.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 1);
        assert_eq!(docs.parameters[0].name, "Organization");
        assert_eq!(docs.returned_value.len(), 1);
        assert_eq!(docs.returned_value[0].name, "Boolean");
    }

    #[test]
    fn test_parse_parameter_description_continuation_not_extra_parameter() {
        let comments = vec![
            "Получает оформленное накладными по заказам количество.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  ОтборПоИзмерениям - Структура - Ключ структуры определяет имя измерения,"
                .to_string(),
            "                      а значение структуры - искомое значение.".to_string(),
            "  ИсключитьЗаказ - Булево - признак исключения заказа.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 2);
        assert_eq!(docs.parameters[0].name, "ОтборПоИзмерениям");
        assert_eq!(docs.parameters[1].name, "ИсключитьЗаказ");
    }

    #[test]
    fn test_parse_nested_parameter_fields_still_attached() {
        let comments = vec![
            "Заполняет настройки.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  Настройки - Структура - настройки заполнения:".to_string(),
            "    * Организация - СправочникСсылка.Организации - организация.".to_string(),
            "    * Дата - Дата - дата заполнения.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 1);
        assert_eq!(docs.parameters[0].name, "Настройки");
        assert_eq!(docs.parameters[0].types[0].parameters.len(), 2);
        assert_eq!(docs.parameters[0].types[0].parameters[0].name, "Организация");
        assert_eq!(docs.parameters[0].types[0].parameters[1].name, "Дата");
    }

    #[test]
    fn test_parse_deprecated() {
        let comments = vec![
            "Старый метод.".to_string(),
            "".to_string(),
            "Устарела:".to_string(),
            "Используйте НовыйМетод() вместо этого метода.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(docs.is_deprecated());
        assert_eq!(
            docs.deprecation,
            Some("Используйте НовыйМетод() вместо этого метода.".to_string())
        );
    }

    #[test]
    fn test_parse_deprecated_with_dot() {
        let comments = vec!["Устарела.".to_string()];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(docs.is_deprecated());
        assert_eq!(docs.deprecation, Some("".to_string()));
    }

    #[test]
    fn test_parse_deprecated_with_dot_and_info() {
        let comments = vec!["Устарела. Используйте НовыйМетод().".to_string()];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(docs.is_deprecated());
        assert_eq!(docs.deprecation, Some("Используйте НовыйМетод().".to_string()));
    }

    #[test]
    fn test_parse_deprecated_english() {
        let comments = vec!["Deprecated.".to_string()];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(docs.is_deprecated());
    }

    #[test]
    fn test_parse_parameters_with_multiple_types() {
        let comments = vec![
            "Обрабатывает значение.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  Значение - Число, Строка - значение для обработки".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 1);
        assert_eq!(docs.parameters[0].name, "Значение");
        assert_eq!(docs.parameters[0].types.len(), 2);
        assert_eq!(docs.parameters[0].types[0].name, "Число");
        assert_eq!(docs.parameters[0].types[1].name, "Строка");
    }

    #[test]
    fn test_parse_call_options() {
        let comments = vec![
            "Выполняет операцию.".to_string(),
            "".to_string(),
            "Варианты вызова:".to_string(),
            "  Вариант 1: Выполнить(Параметр1)".to_string(),
            "  Вариант 2: Выполнить(Параметр1, Параметр2)".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.call_options.len(), 2);
        assert!(docs.call_options[0].contains("Вариант 1"));
        assert!(docs.call_options[1].contains("Вариант 2"));
    }

    #[test]
    fn test_parse_english_documentation() {
        let comments = vec![
            "Calculates the sum.".to_string(),
            "".to_string(),
            "Parameters:".to_string(),
            "  A - Number - first addend".to_string(),
            "  B - Number - second addend".to_string(),
            "".to_string(),
            "Returns:".to_string(),
            "  Number - sum result".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.purpose, Some("Calculates the sum.".to_string()));
        assert_eq!(docs.parameters.len(), 2);
        assert_eq!(docs.returned_value.len(), 1);
        assert_eq!(docs.returned_value[0].name, "Number");
    }

    #[test]
    fn test_parse_multiline_purpose() {
        let comments = vec![
            "Первая строка описания.".to_string(),
            "Вторая строка описания.".to_string(),
            "Третья строка описания.".to_string(),
            "".to_string(),
            "Возвращаемое значение:".to_string(),
            "  Булево".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert!(docs.purpose.is_some());
        let purpose = docs.purpose.unwrap();
        assert!(purpose.contains("Первая строка"));
        assert!(purpose.contains("Вторая строка"));
        assert!(purpose.contains("Третья строка"));
    }

    #[test]
    fn test_parse_parameters_with_multiline_union_types() {
        let comments = vec![
            "Возвращает значения реквизита.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  Ссылка       - ЛюбаяСсылка - объект, значения реквизитов которого получить."
                .to_string(),
            "               - Строка      - полное имя предопределенного элемента.".to_string(),
            "  ИмяРеквизита - Строка      - имя получаемого реквизита.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 2, "Expected exactly 2 parameters, no phantom '-'");

        assert_eq!(docs.parameters[0].name, "Ссылка");
        assert_eq!(docs.parameters[0].types.len(), 2);
        assert_eq!(docs.parameters[0].types[0].name, "ЛюбаяСсылка");
        assert_eq!(
            docs.parameters[0].types[0].description.as_deref(),
            Some("объект, значения реквизитов которого получить.")
        );
        assert_eq!(docs.parameters[0].types[1].name, "Строка");
        assert_eq!(
            docs.parameters[0].types[1].description.as_deref(),
            Some("полное имя предопределенного элемента.")
        );

        assert_eq!(docs.parameters[1].name, "ИмяРеквизита");
        assert_eq!(docs.parameters[1].types.len(), 1);
        assert_eq!(docs.parameters[1].types[0].name, "Строка");
    }

    #[test]
    fn test_parse_parameters_continuation_does_not_swallow_bullet_descriptions() {
        let comments = vec![
            "Описание.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  Параметр - Число - значение, особенности:".to_string(),
            "             - дополнительное примечание ниже описания".to_string(),
            "  Другой - Строка - имя".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 2);
        assert_eq!(docs.parameters[0].name, "Параметр");
        assert_eq!(
            docs.parameters[0].types.len(),
            1,
            "Bullet line must NOT be absorbed as an extra type, got: {:?}",
            docs.parameters[0].types
        );
        assert_eq!(docs.parameters[0].types[0].name, "Число");
        assert_eq!(docs.parameters[1].name, "Другой");
    }

    #[test]
    fn test_parse_parameter_tab_separated_separator() {
        // The first parameter aligns the `-` with tabs (no space before the dash);
        // it must still be recognized as a described parameter.
        let comments = vec![
            "Проверяет условия триггера.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  ДанныеТриггера\t\t\t- Структура, см. ОбщийМодуль.Структура".to_string(),
            "  ОбъектПроверки\t - Ссылка - объект".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 2);
        assert_eq!(docs.parameters[0].name, "ДанныеТриггера");
        assert_eq!(docs.parameters[0].types[0].name, "Структура");
        assert_eq!(docs.parameters[1].name, "ОбъектПроверки");
    }

    #[test]
    fn test_parse_parameter_bare_dotted_union_continuation_not_phantom() {
        // A union type continued on the next line without a leading `-`: the dotted
        // type must attach to `Письмо`, not become a phantom parameter.
        let comments = vec![
            "Обрабатывает письмо.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  Письмо - ДокументСсылка.ЭлектронноеПисьмоВходящее,".to_string(),
            "           ДокументСсылка.ЭлектронноеПисьмоИсходящее - письмо для оценки.".to_string(),
            "  ТекстHTML - Строка - обрабатываемый текст.".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 2, "no phantom parameter from union continuation");
        assert_eq!(docs.parameters[0].name, "Письмо");
        assert!(
            docs.parameters[0]
                .types
                .iter()
                .any(|t| t.name == "ДокументСсылка.ЭлектронноеПисьмоИсходящее"),
            "continuation type must attach to Письмо, got: {:?}",
            docs.parameters[0].types
        );
        assert_eq!(docs.parameters[1].name, "ТекстHTML");
    }

    #[test]
    fn test_parse_parameter_dotted_union_continuation_with_tab_dash() {
        // The continuation's inline description is separated by a tab-flanked dash;
        // it must still attach to `Письмо` and not spawn a phantom parameter.
        let comments = vec![
            "Обрабатывает письмо.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  Письмо - ДокументСсылка.ЭлектронноеПисьмоВходящее,".to_string(),
            "           ДокументСсылка.ЭлектронноеПисьмоИсходящее\t-\tписьмо для оценки"
                .to_string(),
            "  ТекстHTML - Строка - текст".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 2, "no phantom parameter from tab-dash continuation");
        assert_eq!(docs.parameters[0].name, "Письмо");
        assert!(
            docs.parameters[0]
                .types
                .iter()
                .any(|t| t.name == "ДокументСсылка.ЭлектронноеПисьмоИсходящее"),
            "continuation type must attach to Письмо, got: {:?}",
            docs.parameters[0].types
        );
        assert_eq!(docs.parameters[1].name, "ТекстHTML");
    }

    #[test]
    fn test_parse_parameter_with_structured_type() {
        let comments = vec![
            "Обрабатывает настройки.".to_string(),
            "".to_string(),
            "Параметры:".to_string(),
            "  Настройки - Структура - настройки подключения".to_string(),
            "    * Сервер - Строка - адрес сервера".to_string(),
            "    * Порт - Число - номер порта".to_string(),
        ];

        let docs = parse_method_docs(&comments).unwrap();

        assert_eq!(docs.parameters.len(), 1);
        assert_eq!(docs.parameters[0].name, "Настройки");
        assert_eq!(docs.parameters[0].types[0].name, "Структура");

        let nested = &docs.parameters[0].types[0].parameters;
        assert_eq!(nested.len(), 2);
        assert_eq!(nested[0].name, "Сервер");
        assert_eq!(nested[1].name, "Порт");
    }

    #[test]
    fn variable_docs_empty_input_returns_none() {
        assert_eq!(parse_variable_docs(&[]), None);
    }

    #[test]
    fn variable_docs_plain_purpose() {
        let comments = vec!["описание переменной".to_string()];
        let docs = parse_variable_docs(&comments).unwrap();
        assert_eq!(docs.purpose.as_deref(), Some("описание переменной"));
        assert!(docs.deprecation.is_none());
        assert!(docs.link.is_none());
        assert!(!docs.is_hyperlink());
        assert!(!docs.is_deprecated());
    }

    #[test]
    fn variable_docs_multiline_purpose_joined_by_newline() {
        let comments = vec!["первая строка".to_string(), "вторая строка".to_string()];
        let docs = parse_variable_docs(&comments).unwrap();
        assert_eq!(docs.purpose.as_deref(), Some("первая строка\nвторая строка"));
    }

    #[test]
    fn variable_docs_hyperlink_only() {
        let comments = vec!["См. ОбщегоНазначения.ИмяПеременной".to_string()];
        let docs = parse_variable_docs(&comments).unwrap();
        assert!(docs.is_hyperlink());
        assert_eq!(docs.link.as_deref(), Some("См. ОбщегоНазначения.ИмяПеременной"));
        assert!(docs.purpose.is_none());
        assert!(docs.deprecation.is_none());
    }

    #[test]
    fn variable_docs_hyperlink_english() {
        let comments = vec!["See Common.SomeVariable".to_string()];
        let docs = parse_variable_docs(&comments).unwrap();
        assert!(docs.is_hyperlink());
        assert_eq!(docs.link.as_deref(), Some("See Common.SomeVariable"));
    }

    #[test]
    fn variable_docs_deprecation_section() {
        let comments = vec![
            "Старое описание.".to_string(),
            "Устарела:".to_string(),
            "используйте НовоеИмя.".to_string(),
        ];
        let docs = parse_variable_docs(&comments).unwrap();
        assert_eq!(docs.purpose.as_deref(), Some("Старое описание."));
        assert!(docs.is_deprecated());
        assert!(docs.deprecation.as_deref().unwrap_or("").contains("используйте НовоеИмя"));
    }

    #[test]
    fn variable_docs_deprecation_inline() {
        let comments = vec!["Устарела: используйте НовоеИмя.".to_string()];
        let docs = parse_variable_docs(&comments).unwrap();
        assert!(docs.is_deprecated());
        assert!(docs.purpose.is_none());
    }

    #[test]
    fn variable_docs_keeps_raw_input() {
        let comments = vec!["a".to_string(), "b".to_string()];
        let docs = parse_variable_docs(&comments).unwrap();
        assert_eq!(docs.raw, comments);
    }

    #[test]
    fn variable_docs_hyperlink_on_last_line() {
        let comments = vec!["См. ОбщегоНазначения.СомеVariable".to_string()];
        let docs = parse_variable_docs(&comments).unwrap();
        assert!(docs.is_hyperlink());
    }

    #[test]
    fn variable_docs_hyperlink_after_purpose() {
        let comments = vec!["Описание переменной.".to_string(), "См. ДругойМодуль.Имя".to_string()];
        let docs = parse_variable_docs(&comments).unwrap();
        assert!(docs.is_hyperlink());
        assert_eq!(docs.link.as_deref(), Some("См. ДругойМодуль.Имя"));
        assert!(docs.purpose.is_none());
    }

    #[test]
    fn variable_docs_deprecation_suppresses_last_line_hyperlink() {
        let comments = vec![
            "Старое описание.".to_string(),
            "Устарела: используйте НовоеИмя.".to_string(),
            "См. ДругойМодуль".to_string(),
        ];
        let docs = parse_variable_docs(&comments).unwrap();
        assert!(!docs.is_hyperlink());
        assert!(docs.is_deprecated());
    }

    #[test]
    fn variable_docs_types_field_unused_for_now() {
        let comments = vec!["ИмяПеременной - Строка - комментарий".to_string()];
        let docs = parse_variable_docs(&comments).unwrap();
        assert!(docs.types.is_empty());
        assert_eq!(docs.purpose.as_deref(), Some("ИмяПеременной - Строка - комментарий"));
    }
}
