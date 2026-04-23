# Using hardcode ip addresses in code (UsingHardcodeNetworkAddress)

## Description

Hardcoded network addresses should not be stored directly in source code.

This applies to both IPv4 and IPv6 literals. Such values are infrastructure settings and usually need to be changed independently from application code.

Recommended storage options:

* constants
* information registers
* catalogs, exchange plan nodes, or other metadata objects
* a dedicated module with this rule disabled as a last resort

## Examples

Incorrect:
```bsl
NetworkAddress = "192.168.0.1";
```

Correct:
```bsl
NetworkAddress = MyModuleReUse.ServerNetworkAddress();
```

## Sources

* [v8std: UsingHardcodeNetworkAddress](https://v8std.ru/diagnostics/bslls/UsingHardcodeNetworkAddress/)
