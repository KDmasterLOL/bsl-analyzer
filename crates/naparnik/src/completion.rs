use crate::session::SessionManager;
use crate::types::{CompletionContext, CompletionResult, SessionConfig, TypeHint};
use crate::{NaparnikApi, NaparnikError};

const HINT_PREFIX: &str = "//# ";
const HINT_BLOCK_START: &str = "//# НАЧАЛО ИИ";
const HINT_BLOCK_END: &str = "//# КОНЕЦ ИИ";

pub struct InlineCompletionUseCase<A: NaparnikApi> {
    session_manager: SessionManager<A>,
}

impl<A: NaparnikApi> InlineCompletionUseCase<A> {
    pub fn new(session_manager: SessionManager<A>) -> Self {
        Self { session_manager }
    }

    pub async fn complete(
        &self,
        config: &SessionConfig,
        ctx: &CompletionContext,
    ) -> Result<CompletionResult, NaparnikError> {
        let session = self.session_manager.get_or_create(config).await?;

        let prefix = build_prefix_with_hints(&ctx.prefix, &ctx.type_hints, session.prefix_length);
        let suffix = truncate_suffix(&ctx.suffix, session.suffix_length);

        let api_ctx = CompletionContext {
            prefix,
            suffix,
            path: ctx.path.clone(),
            offset: ctx.offset,
            script_language: ctx.script_language.clone(),
            cursor_object: ctx.cursor_object.clone(),
            current_method: ctx.current_method.clone(),
            cursor_environments: ctx.cursor_environments.clone(),
            type_hints: Vec::new(),
        };

        let result = self.session_manager.api().complete(&session, &api_ctx).await?;
        let filtered = filter_hint_lines(&result.text);

        Ok(CompletionResult { text: filtered, finish_reason: result.finish_reason })
    }
}

fn build_prefix_with_hints(prefix: &str, hints: &[TypeHint], max_len: usize) -> String {
    if hints.is_empty() {
        return truncate_prefix(prefix, max_len);
    }

    let mut hint_block = String::new();
    hint_block.push_str(HINT_BLOCK_START);
    hint_block.push('\n');
    for hint in hints {
        hint_block.push_str(HINT_PREFIX);
        hint_block.push_str("Выражение \"");
        hint_block.push_str(&hint.variable_name);
        hint_block.push_str("\"\n");
        if !hint.properties.is_empty() {
            hint_block.push_str(HINT_PREFIX);
            hint_block.push_str("-Свойства: ");
            hint_block.push_str(&hint.properties.join(","));
            hint_block.push('\n');
        }
    }
    hint_block.push_str(HINT_BLOCK_END);
    hint_block.push('\n');

    let available = max_len.saturating_sub(hint_block.len());
    let truncated_prefix = truncate_prefix(prefix, available);
    format!("{hint_block}{truncated_prefix}")
}

fn truncate_prefix(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        return s.to_string();
    }
    s.chars().skip(char_count - max_len).collect()
}

fn truncate_suffix(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        return s.to_string();
    }
    s.chars().take(max_len).collect()
}

fn filter_hint_lines(text: &str) -> String {
    text.lines().filter(|line| !line.starts_with(HINT_PREFIX)).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_hint_lines_removes_prefixed() {
        let input = "Результат = Новый Массив;\n//# hint\nРезультат.Добавить(1);";
        let result = filter_hint_lines(input);
        assert_eq!(result, "Результат = Новый Массив;\nРезультат.Добавить(1);");
    }

    #[test]
    fn filter_hint_lines_preserves_normal() {
        let input = "// comment\nCode();";
        let result = filter_hint_lines(input);
        assert_eq!(result, input);
    }

    #[test]
    fn truncate_prefix_takes_tail() {
        assert_eq!(truncate_prefix("abcdef", 3), "def");
        assert_eq!(truncate_prefix("ab", 5), "ab");
    }

    #[test]
    fn truncate_suffix_takes_head() {
        assert_eq!(truncate_suffix("abcdef", 3), "abc");
        assert_eq!(truncate_suffix("ab", 5), "ab");
    }

    #[test]
    fn build_prefix_no_hints() {
        let result = build_prefix_with_hints("code", &[], 100);
        assert_eq!(result, "code");
    }

    #[test]
    fn build_prefix_with_type_hints() {
        let hints = vec![TypeHint {
            variable_name: "Результат".into(),
            properties: vec!["Количество".into(), "Добавить".into()],
        }];
        let result = build_prefix_with_hints("code", &hints, 500);
        assert!(result.contains(HINT_BLOCK_START));
        assert!(result.contains("Выражение \"Результат\""));
        assert!(result.contains("-Свойства: Количество,Добавить"));
        assert!(result.contains(HINT_BLOCK_END));
        assert!(result.ends_with("code"));
    }
}
