# Method Documentation Architecture

## Problem Statement

Currently, method documentation (doc comments) is parsed **ad-hoc in diagnostics** using a simple regex-based parser (`method_description.rs`). This approach has limitations:

1. **No reuse**: Each diagnostic or LSP feature that needs documentation must parse it independently
2. **No caching**: Comments are reparsed on every diagnostic run (no Salsa caching)
3. **Limited LSP support**: Cannot easily provide hover, signature help, or completion with doc info
4. **Inconsistency**: Different parts of code may interpret documentation differently

## Proposed Solution

**Parse documentation once and store it in HIR**, following the rust-analyzer pattern:

```
Parse → ItemTree (with doc ranges) → MethodDescription (Salsa query) → Use in Diagnostics & LSP
```

## Architecture

### 1. Data Structures (in `hir-def`)

```rust
// crates/hir-def/src/docs.rs

/// Parsed documentation for a BSL method.
///
/// Analogous to MethodDescription in bsl-language-server (Java).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodDocs {
    /// Full raw text of all doc comments
    raw: String,

    /// Purpose/description section (before any keywords)
    purpose: Option<String>,

    /// Parameters with types and descriptions
    parameters: Vec<ParameterDoc>,

    /// Return value types and descriptions
    returned_value: Vec<TypeDoc>,

    /// Examples section
    examples: Vec<String>,

    /// Call options section
    call_options: Vec<String>,

    /// Deprecation info (if "Устарела:" present)
    deprecation: Option<String>,

    /// Hyperlink reference (if "См. Method()")
    link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterDoc {
    pub name: String,
    pub types: Vec<TypeDoc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDoc {
    /// Type name (e.g., "Строка", "Число", "Структура")
    pub name: String,

    /// Description of this type (may be None)
    pub description: Option<String>,

    /// Sub-parameters for structured types (fields with *)
    /// Example: For "Структура:", this contains the "* Field - Type - desc" entries
    pub parameters: Vec<ParameterDoc>,

    /// Is this a hyperlink reference? (e.g., "См. Method()")
    pub is_hyperlink: bool,
}
```

### 2. Salsa Query (in `hir-def/src/db.rs`)

```rust
#[salsa::query_group(DefDatabaseStorage)]
pub trait DefDatabase: ... {
    // Existing queries...

    /// Parse documentation comments for a method.
    ///
    /// Returns `None` if the method has no documentation.
    /// Cached by Salsa - only reparsed when comments change.
    fn method_docs(&self, method: MethodId) -> Option<Arc<MethodDocs>>;
}
```

### 3. Implementation (new file `hir-def/src/docs.rs`)

```rust
use crate::{DefDatabase, MethodId};
use syntax::extract_leading_comments;
use std::sync::Arc;

/// Parse method documentation from source comments.
pub(crate) fn method_docs_query(db: &dyn DefDatabase, method: MethodId) -> Option<Arc<MethodDocs>> {
    let parse = db.parse(method.module.file_id);
    let tree = db.item_tree(method.module.file_id);

    // Find the method's AST node
    let method_node = find_method_node(&parse, &tree, method)?;

    // Extract leading comments
    let file_text = db.file_text_input(method.module.file_id);
    let source_text = file_text.text(db);
    let comments = extract_leading_comments(&method_node, source_text)?;

    // Parse using enhanced parser
    let docs = parse_method_docs(&comments)?;

    Some(Arc::new(docs))
}

/// Enhanced parser that creates MethodDocs from comment lines.
fn parse_method_docs(comments: &[String]) -> Option<MethodDocs> {
    // Implementation using improved version of current parse_return_block_simple
    // but parsing ALL sections (params, returns, examples, etc.)
    // ...
}
```

### 4. HIR API (in `hir/src/lib.rs`)

```rust
impl<'db, DB: DefDatabase> Method<'db, DB> {
    // Existing methods...

    /// Get parsed documentation for this method.
    ///
    /// Returns `None` if method has no doc comments.
    ///
    /// # Example
    /// ```
    /// let docs = method.docs()?;
    /// println!("Purpose: {}", docs.purpose.unwrap_or_default());
    /// for param in &docs.parameters {
    ///     println!("Param {}: {:?}", param.name, param.types);
    /// }
    /// ```
    pub fn docs(&self) -> Option<Arc<MethodDocs>> {
        self.db.method_docs(self.id)
    }
}
```

### 5. Usage in Diagnostics

**Before** (current):
```rust
// In MissingReturnedValueDescription diagnostic
let comments = extract_leading_comments(func_node, source_text).unwrap_or_default();
let return_info = parse_return_block_simple(&comments);
if !return_info.has_return_keyword { /* error */ }
```

**After** (with HIR docs):
```rust
// In MissingReturnedValueDescription diagnostic
fn check_function(method: Method<'_, impl DefDatabase>) -> Option<Diagnostic> {
    if !method.is_export() { return None; }

    let docs = method.docs();

    // Check 1: Export function must have docs
    if docs.is_none() {
        return Some(diagnostic("Добавьте описание возвращаемого значения функции"));
    }

    let docs = docs.unwrap();

    // Check 2: Must have return value section
    if docs.returned_value.is_empty() {
        // Check for hyperlink first
        if docs.link.is_some() { return None; }
        return Some(diagnostic("Добавьте описание возвращаемого значения функции"));
    }

    // Check 3: Strict mode - types must have descriptions
    if !allow_short {
        let missing: Vec<_> = docs.returned_value.iter()
            .filter(|t| t.description.is_none() && t.parameters.is_empty())
            .map(|t| t.name.as_str())
            .collect();
        if !missing.is_empty() {
            return Some(diagnostic(&format!("Необходимо добавить описание типов \"{}\"", missing.join(", "))));
        }
    }

    None
}
```

### 6. Usage in LSP Features

**Hover:**
```rust
// In ide/src/hover.rs
pub fn hover(db: &RootDatabase, position: FilePosition) -> Option<HoverResult> {
    let method = find_method_at_position(db, position)?;
    let docs = method.docs()?;

    let mut content = String::new();

    // Signature
    content.push_str(&format!("```bsl\n{}\n```\n\n", method.signature()));

    // Purpose
    if let Some(purpose) = &docs.purpose {
        content.push_str(purpose);
        content.push_str("\n\n");
    }

    // Parameters
    if !docs.parameters.is_empty() {
        content.push_str("**Параметры:**\n");
        for param in &docs.parameters {
            let types: Vec<_> = param.types.iter().map(|t| &t.name).collect();
            content.push_str(&format!("- `{}`: {}\n", param.name, types.join(", ")));
        }
        content.push_str("\n");
    }

    // Return value
    if !docs.returned_value.is_empty() {
        content.push_str("**Возвращаемое значение:**\n");
        for type_doc in &docs.returned_value {
            if let Some(desc) = &type_doc.description {
                content.push_str(&format!("- `{}` - {}\n", type_doc.name, desc));
            } else {
                content.push_str(&format!("- `{}`\n", type_doc.name));
            }
        }
    }

    Some(HoverResult { content, range: method.name_range() })
}
```

**Signature Help:**
```rust
// In ide/src/signature_help.rs
pub fn signature_help(db: &RootDatabase, position: FilePosition) -> Option<SignatureHelp> {
    let call = find_call_at_position(db, position)?;
    let method = resolve_method(db, &call)?;
    let docs = method.docs()?;

    let mut parameters = Vec::new();
    for param_doc in &docs.parameters {
        let types: Vec<_> = param_doc.types.iter().map(|t| &t.name).collect();
        let label = format!("{}: {}", param_doc.name, types.join(" | "));

        let documentation = param_doc.types.iter()
            .filter_map(|t| t.description.as_ref())
            .next()
            .cloned();

        parameters.push(ParameterInfo { label, documentation });
    }

    Some(SignatureHelp { parameters, active_parameter: call.active_arg_index() })
}
```

## Benefits

1. **✅ Single source of truth**: Documentation parsed once, used everywhere
2. **✅ Salsa caching**: Only reparsed when file changes (huge performance win)
3. **✅ Rich LSP features**: Easy to implement hover, signature help, completion
4. **✅ Consistency**: All diagnostics use the same structured data
5. **✅ Testability**: Can test documentation parsing separately from diagnostics
6. **✅ Extensibility**: Easy to add new doc sections (e.g., `@internal`, `@deprecated`)

## Implementation Plan

### Phase 1: Core Infrastructure (1-2 days)
- [ ] Create `hir-def/src/docs.rs` with data structures
- [ ] Add `method_docs()` Salsa query to `DefDatabase`
- [ ] Implement basic parser (migrate from `method_description.rs`)
- [ ] Add tests for parser

### Phase 2: HIR Integration (1 day)
- [ ] Add `Method::docs()` method in `hir/src/lib.rs`
- [ ] Update ItemTree to track doc comment ranges (optional optimization)
- [ ] Add integration tests

### Phase 3: Refactor Diagnostics (1-2 days)
- [ ] Refactor `MissingReturnedValueDescription` to use HIR docs
- [ ] Refactor `MissingParameterDescription` to use HIR docs
- [ ] Update other doc-related diagnostics
- [ ] Verify all diagnostic tests pass

### Phase 4: LSP Features (2-3 days)
- [ ] Implement hover with method documentation
- [ ] Implement signature help with parameter info
- [ ] Add completion with type hints from docs
- [ ] Test in real BSL projects

### Phase 5: Polish (1 day)
- [ ] Performance testing
- [ ] Documentation
- [ ] Handle edge cases (nested structures, complex types)

**Total estimate: 6-9 days**

## Compatibility Notes

- **100% compatible with bsl-language-server**: We parse the same documentation format
- **No breaking changes**: Diagnostics behavior stays the same, just better structured internally
- **Migration path**: Keep `method_description.rs` initially, migrate incrementally

## Open Questions

1. **Should we parse docs in ItemTree or as a separate query?**
   - **Recommendation**: Separate query (more flexible, easier to cache)

2. **How to handle malformed documentation?**
   - **Recommendation**: Parse what we can, return `None` for unparseable sections

3. **Should we support nested structure fields in hover?**
   - **Recommendation**: Yes, show indented structure (Phase 4)

## References

- **Java implementation**: `bsl-language-server/src/main/java/...context/symbol/description/`
- **rust-analyzer docs**: `rust-analyzer/crates/hir-def/src/attrs.rs` (Docs struct)
- **ANTLR4 grammar**: `bsl-parser/src/main/antlr/BSLMethodDescriptionParser.g4`
