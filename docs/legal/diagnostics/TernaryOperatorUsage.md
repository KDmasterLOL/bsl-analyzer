# TernaryOperatorUsage provenance

## Status

Candidate for `MIT OR Apache-2.0`.

## Why

This is a generic readability rule about preferring explicit branching over ternary expressions. The idea is common static-analysis guidance and is not specific to any upstream project.

## Public sources

- General readability guidance for conditional expressions.
- `v8std.ru` page for this diagnostic, used only as a secondary public reference.

## Implementation notes

The current implementation is local and HIR-based. It:

- consumes lowering-time diagnostics for ternary `?(...)` expressions;
- reports every ternary occurrence, not only nested or obviously redundant cases;
- reports nested ternaries separately;
- stays disabled by default unless explicitly enabled in configuration.

## Audit notes

- Rule idea: clean and generic.
- Docs were corrected to match the real behavior: this rule flags any ternary operator usage and is disabled by default.
- Existing tests are local and cover multiline ternaries, nested ternaries, simple single ternaries, and default-disabled behavior.
