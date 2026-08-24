use crate::{Diagnostic, DiagnosticCode, DiagnosticsConfig, DiagnosticsContext};
use syntax::{SyntaxKind, SyntaxNode, TextRange};

/// Takes the configuration rather than a [`DiagnosticsContext`]: every SDBL rule needs the
/// project's severities, tags and parameters, and none of them needs the file. That is what
/// lets the same 18 rules serve a query embedded in a module and a bare query text.
pub type SdblDispatchFn = fn(
    config: &DiagnosticsConfig,
    diag: &sdbl_hir::SdblDiagnostic,
    mapper: &SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
);

pub fn dispatch_simple(
    config: &DiagnosticsConfig,
    code: DiagnosticCode,
    message: &str,
    range: TextRange,
    mapper: &SdblPositionMapper,
    query_text: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(Diagnostic {
        code,
        message: message.to_string(),
        severity: config.severity(code),
        range: mapper.map_range(range, query_text),
        tags: config.tags(code),
        fixes: vec![],
    });
}

pub fn collect_sdbl_via_dispatch(
    ctx: &DiagnosticsContext,
    code: DiagnosticCode,
    dispatch_fn: SdblDispatchFn,
) -> Vec<Diagnostic> {
    if ctx.is_disabled_with_metadata(code) {
        return Vec::new();
    }

    let sdbl_hirs = ctx.sdbl_hir_in_file();
    let bsl_source = ctx.file_text();
    let sdbl_queries = ctx.all_sdbl_in_file();
    let line_starts = build_line_index_shared(&bsl_source);

    let mut diagnostics = Vec::new();

    for ((_expr_id, sdbl_package), (_query_expr_id, query_info)) in
        sdbl_hirs.iter().zip(sdbl_queries.iter())
    {
        let mapper = SdblPositionMapper::from_query_info(query_info, &bsl_source, &line_starts);

        for hir_diag in sdbl_package.all_diagnostics() {
            dispatch_fn(ctx.config, hir_diag, &mapper, &query_info.query_text, &mut diagnostics);
        }
    }

    diagnostics
}

/// Where a query's own coordinates have to land.
///
/// Two modes, because there are two kinds of query and only one of them sits inside
/// something else. `Standalone` is not `Embedded` with zeroed offsets: the embedded
/// projection adds one for the opening quote and re-anchors every continuation line on its
/// `|`, so feeding it the query text as its own source would still shift every range.
#[derive(Debug, Clone)]
pub enum SdblPositionMapper<'a> {
    /// The query is a BSL string literal in a module; ranges project into the module's text.
    Embedded(EmbeddedMapper<'a>),
    /// The query IS the document; its ranges are already the answer.
    Standalone,
}

#[derive(Debug, Clone)]
pub struct EmbeddedMapper<'a> {
    bsl_source: &'a str,

    bsl_literal_line: u32,
    bsl_literal_col: u32,

    line_starts: Vec<usize>,

    quote_corrections: Vec<(usize, usize)>,
}

impl<'a> SdblPositionMapper<'a> {
    pub fn new_from_range(
        bsl_literal_range: TextRange,
        bsl_source: &'a str,
        quote_corrections: Vec<(usize, usize)>,
    ) -> Self {
        let (bsl_literal_line, bsl_literal_col) =
            byte_offset_to_line_col(bsl_source, u32::from(bsl_literal_range.start()));

        let line_starts = build_line_index(bsl_source);

        Self::Embedded(EmbeddedMapper {
            bsl_source,
            bsl_literal_line,
            bsl_literal_col,
            line_starts,
            quote_corrections,
        })
    }

    pub fn new_from_range_with_line_index(
        bsl_literal_range: TextRange,
        bsl_source: &'a str,
        line_starts: &'a [usize],
        quote_corrections: Vec<(usize, usize)>,
    ) -> Self {
        let (bsl_literal_line, bsl_literal_col) = byte_offset_to_line_col_fast(
            bsl_source,
            line_starts,
            u32::from(bsl_literal_range.start()),
        );

        let line_starts = line_starts.to_vec();

        Self::Embedded(EmbeddedMapper {
            bsl_source,
            bsl_literal_line,
            bsl_literal_col,
            line_starts,
            quote_corrections,
        })
    }

    pub fn from_query_info(
        query_info: &syntax::SdblQueryInfo,
        bsl_source: &'a str,
        line_starts: &'a [usize],
    ) -> Self {
        Self::new_from_range_with_line_index(
            query_info.bsl_literal_range,
            bsl_source,
            line_starts,
            query_info.quote_corrections.clone(),
        )
    }

    pub fn map_range(&self, sdbl_range: TextRange, sdbl_text: &str) -> TextRange {
        let Self::Embedded(this) = self else { return sdbl_range };

        let sdbl_line_starts = build_line_index(sdbl_text);

        let (sdbl_start_line, sdbl_start_col) = byte_offset_to_line_col_fast(
            sdbl_text,
            &sdbl_line_starts,
            u32::from(sdbl_range.start()),
        );
        let (sdbl_end_line, sdbl_end_col) =
            byte_offset_to_line_col_fast(sdbl_text, &sdbl_line_starts, u32::from(sdbl_range.end()));

        let sdbl_start = u32::from(sdbl_range.start()) as usize;
        let sdbl_end = u32::from(sdbl_range.end()) as usize;

        let start_correction: usize = this
            .quote_corrections
            .iter()
            .filter(|(pos, _)| {
                let (line, _col) =
                    byte_offset_to_line_col_fast(sdbl_text, &sdbl_line_starts, *pos as u32);
                line == sdbl_start_line && *pos < sdbl_start
            })
            .map(|(_, chars)| chars)
            .sum();

        let end_correction: usize = this
            .quote_corrections
            .iter()
            .filter(|(pos, _)| {
                let (line, _col) =
                    byte_offset_to_line_col_fast(sdbl_text, &sdbl_line_starts, *pos as u32);
                line == sdbl_end_line && *pos < sdbl_end
            })
            .map(|(_, chars)| chars)
            .sum();

        let bsl_literal_line = this.bsl_literal_line;
        let bsl_literal_col = this.bsl_literal_col;

        let bsl_start_line = bsl_literal_line + sdbl_start_line;
        let bsl_start_col = if sdbl_start_line == 0 {
            bsl_literal_col + sdbl_start_col + 1 + (start_correction as u32)
        } else {
            let bsl_line_text =
                get_line_text(this.bsl_source, &this.line_starts, bsl_start_line as usize);
            if let Some(pipe_pos) = bsl_line_text.find('|') {
                (pipe_pos as u32) + 1 + sdbl_start_col + (start_correction as u32)
            } else {
                sdbl_start_col + (start_correction as u32)
            }
        };

        let bsl_end_line = bsl_literal_line + sdbl_end_line;
        let bsl_end_col = if sdbl_end_line == 0 {
            bsl_literal_col + sdbl_end_col + 1 + (end_correction as u32)
        } else {
            let bsl_line_text =
                get_line_text(this.bsl_source, &this.line_starts, bsl_end_line as usize);
            if let Some(pipe_pos) = bsl_line_text.find('|') {
                (pipe_pos as u32) + 1 + sdbl_end_col + (end_correction as u32)
            } else {
                sdbl_end_col + (end_correction as u32)
            }
        };

        let bsl_start_offset = line_col_to_byte_offset_fast(
            this.bsl_source,
            &this.line_starts,
            bsl_start_line,
            bsl_start_col,
        );
        let bsl_end_offset = line_col_to_byte_offset_fast(
            this.bsl_source,
            &this.line_starts,
            bsl_end_line,
            bsl_end_col,
        );

        TextRange::new(bsl_start_offset.into(), bsl_end_offset.into())
    }
}

pub fn byte_offset_to_line_col(text: &str, offset: u32) -> (u32, u32) {
    let mut line = 0;
    let mut col = 0;

    for (idx, ch) in text.char_indices() {
        if idx as u32 >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }

    (line, col)
}

fn byte_offset_to_line_col_fast(text: &str, line_starts: &[usize], offset: u32) -> (u32, u32) {
    let offset = offset as usize;

    let offset = offset.min(text.len());

    let line = match line_starts.binary_search(&offset) {
        Ok(exact) => exact,
        Err(insert_pos) => insert_pos.saturating_sub(1),
    };

    let line_start = line_starts[line];
    let col = if offset > line_start {
        let mut char_count = 0;
        for (byte_idx, _) in text[line_start..].char_indices() {
            if line_start + byte_idx >= offset {
                break;
            }
            char_count += 1;
        }
        char_count
    } else {
        0
    };

    (line as u32, col as u32)
}

pub fn build_line_index_shared(text: &str) -> Vec<usize> {
    build_line_index(text)
}

fn build_line_index(text: &str) -> Vec<usize> {
    let mut line_starts = vec![0];

    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            line_starts.push(idx + 1);
        }
    }

    line_starts
}

fn get_line_text<'a>(text: &'a str, line_starts: &[usize], line: usize) -> &'a str {
    if line >= line_starts.len() {
        return "";
    }

    let start = line_starts[line];
    let end = line_starts.get(line + 1).copied().unwrap_or(text.len());

    let line_text = &text[start..end];
    line_text.strip_suffix('\n').unwrap_or(line_text)
}

fn line_col_to_byte_offset_fast(
    text: &str,
    line_starts: &[usize],
    target_line: u32,
    target_col: u32,
) -> u32 {
    let line = target_line as usize;
    if line >= line_starts.len() {
        return line_starts.last().copied().unwrap_or(0) as u32;
    }

    let line_start = line_starts[line];

    if target_col == 0 {
        return line_start as u32;
    }

    let next_line_start = line_starts.get(line + 1).copied().unwrap_or(text.len());
    let line_text = &text[line_start..next_line_start];

    for (char_count, (byte_idx, _ch)) in line_text.char_indices().enumerate() {
        if char_count as u32 == target_col {
            return (line_start + byte_idx) as u32;
        }
    }

    next_line_start as u32
}

pub fn extract_string_content(node: &SyntaxNode) -> Option<String> {
    let mut result = String::new();
    let mut tokens = node.children_with_tokens().filter_map(|it| it.into_token());

    let first_token = tokens.next()?;

    match first_token.kind() {
        SyntaxKind::STRING => {
            let text = first_token.text();
            if text.len() < 2 {
                return None;
            }
            let inner = &text[1..text.len() - 1];
            result = inner.replace("\"\"", "\"");
        }
        SyntaxKind::STRING_START => {
            let text = first_token.text();
            if text.is_empty() {
                return None;
            }
            result.push_str(&text[1..]);

            for token in tokens {
                match token.kind() {
                    SyntaxKind::NEWLINE => {
                        result.push('\n');
                    }
                    SyntaxKind::STRING_PART => {
                        let text = token.text();
                        if let Some(content) = text.strip_prefix('|') {
                            result.push_str(content);
                        }
                    }
                    SyntaxKind::STRING_TAIL => {
                        let text = token.text();
                        if let Some(content) = text.strip_prefix('|') {
                            if let Some(content) = content.strip_suffix('"') {
                                result.push_str(content);
                            }
                        }
                        break;
                    }
                    _ => {}
                }
            }

            result = result.replace("\"\"", "\"");
        }
        _ => return None,
    }

    Some(result)
}
