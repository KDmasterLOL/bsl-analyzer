# Deprecated Platform 8.3.17 Global Methods (DeprecatedMethods8317)

## Description

Starting with platform version `8.3.17`, several global error-handling methods
were deprecated in favor of the dedicated `ErrorProcessingManager` /
`МенеджерОбработкиОшибок` object.

Deprecated methods covered by this diagnostic include:

- `BriefErrorRepresentation()` / `КраткоеПредставлениеОшибки()`
- `DetailedErrorRepresentation()` / `ПодробноеПредставлениеОшибки()`
- `ShowErrorInformation()` / `ПоказатьИнформациюОбОшибке()`

In the current implementation, this diagnostic family also reports
`GetForm()` / `ПолучитьФорму()` and suggests `OpenForm()` / `ОткрытьФорму()`.
That part overlaps with the separate `GetFormMethod` rule and should be kept in
mind during broader cleanup.

## Sources

- [1C:Enterprise 8.3.17 platform changelog (RU)](https://dl03.1c.ru/content/Platform/8_3_17_1386/1cv8upd_8_3_17_1386.htm#27f2dc70-f0cf-11e9-8371-0050569f678a)
- [v8std: #std404 Opening forms](https://v8std.ru/std/404/)
