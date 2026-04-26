# Using method OSUsers (OSUsersMethod)

## Description

This diagnostic reports calls to the global method `OSUsers()` /
`ПользователиОС()`.

The method exposes information about operating-system user accounts. In many
projects such access is treated as a security-sensitive operation because it may
reveal environment details that are not needed for ordinary business logic and
should therefore be reviewed explicitly.

This is a security hotspot rather than a syntax or style error: every such call
deserves manual review.

## Sources

- Background: Pass-the-hash attack (Wikipedia)  
  https://ru.wikipedia.org/wiki/Атака_Pass-the-hash
- Secondary reference: [v8std.ru: OSUsersMethod](https://v8std.ru/diagnostics/bslls/OSUsersMethod/)
