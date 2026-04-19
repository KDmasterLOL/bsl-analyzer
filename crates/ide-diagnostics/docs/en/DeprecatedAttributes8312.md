# Deprecated Platform Features Introduced in 8.3.12 (DeprecatedAttributes8312)

## Description

Starting with platform version `8.3.12`, several chart-related properties,
methods, and enumeration values became deprecated. The same release also marked
some older global APIs as obsolete.

This diagnostic reports those deprecated names and suggests the replacement
that should be used in newer code. The affected items include:

- chart and plot-area properties such as `ShowScale`, `ShowLegend`,
  `ShowTitle`, and related scale-label settings;
- chart palette properties and methods such as `ColorPalette`,
  `GetPalette()`, and `SetPalette()`;
- deprecated enumeration names and values such as
  `ChartLabelsOrientation` / `ОриентацияМетокДиаграммы` and
  `ChildFormItemsGroup.Horizontal` / `ГруппировкаПодчиненныхЭлементовФормы.Горизонтальная`;
- the global method `ClearEventLog()` / `ОчиститьЖурналРегистрации()`.

The goal is not just compatibility cleanup but migration to the newer chart API
model introduced by the platform.

## Sources

- [1C:Enterprise 8.3.12 platform changelog (RU)](https://dl04.1c.ru/content/Platform/8_3_12_1714/1cv8upd_8_3_12_1714.htm)
