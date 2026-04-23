# UsageWriteLogEvent

## Status

Strong candidate for `MIT OR Apache-2.0`.

## Why this rule is probably clean

The rule is based on public 1C guidance for writing to the event log and for handling exceptions. The general idea is not unique to `bsl-language-server`: event-log records should contain enough context, and exception paths should not silently hide failures behind low-quality logging.

## Public sources

- ITS / v8std `#std498`: use of the event log.
- ITS / v8std `#std499`: exception handling in code.

## Implementation audit notes

Current implementation is local and rule-specific. It does not merely search for the method name. It receives semantic flags from the HIR layer and checks:

- argument count;
- presence of the log-level argument;
- presence of the comment argument;
- exception-context requirements for `Error` log level;
- presence of `DetailErrorDescription(ErrorInfo())` or a re-raise in the same exception flow.

This is a local policy implementation layered on top of public 1C recommendations.

## Conclusion

`UsageWriteLogEvent` looks like a strong permissive candidate. The rule basis is public, and the current implementation is local and practical rather than copied expression.
