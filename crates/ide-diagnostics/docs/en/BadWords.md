# Prohibited words (BadWords)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
This diagnostic searches module text for words and phrases that are forbidden by
project policy.

The rule is configured with a regular expression. Matching is case-insensitive.
Depending on configuration, the diagnostic may also inspect comments.

Typical uses:

- exclude slang or offensive vocabulary from code and comments;
- ban temporary markers such as `legacy`, `draft`, `temporary`;
- enforce customer- or domain-specific terminology.

**Example configuration:**

`"legacy|draft|temporary"`

`"deprecated_|tmp_|unsafe"`

## Sources

Primary source: project policy and repository-specific naming/content rules

Secondary source: [v8std.ru: BadWords](https://v8std.ru/diagnostics/bslls/BadWords/)
