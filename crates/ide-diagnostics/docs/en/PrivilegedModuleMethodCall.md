# Accessing privileged module methods (PrivilegedModuleMethodCall)

## Description

This diagnostic reports calls to exported methods of privileged common modules.

Privileged modules execute with elevated rights and may bypass ordinary access
checks. Because of that, every call to their public API deserves manual review:
the caller might accidentally or intentionally gain access to operations that
should stay behind an explicit security boundary.

The diagnostic also supports a local `validateNestedCalls` option. When it is
disabled, self-calls from inside the same privileged module are ignored.

## Examples

Assume the configuration contains a privileged module `SystemTasks`:

```bsl
Function ExecuteOperation(OperationName, Params) Export
EndFunction
```

A call from another module:

```bsl
SystemTasks.ExecuteOperation("DeleteData", Params);
```

Such calls should be reviewed to make sure the exposed privileged operation is
really safe for the calling context.

## Sources

- Secondary reference: [v8std.ru: PrivilegedModuleMethodCall](https://v8std.ru/diagnostics/bslls/PrivilegedModuleMethodCall/)
