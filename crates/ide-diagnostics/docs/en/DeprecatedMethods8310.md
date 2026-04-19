# Deprecated Client Application Methods (DeprecatedMethods8310)

## Description

Starting with platform version `8.3.10`, several global context methods related
to the client application were deprecated.

Instead of those global methods, use the `ClientApplication` /
`КлиентскоеПриложение` object and its properties or methods. This makes the API
more explicit and groups client-application behavior under a dedicated object.

Deprecated methods covered by this diagnostic include:

- `SetShortApplicationCaption()` / `УстановитьКраткийЗаголовокПриложения()`
- `GetShortApplicationCaption()` / `ПолучитьКраткийЗаголовокПриложения()`
- `SetClientApplicationCaption()` / `УстановитьЗаголовокКлиентскогоПриложения()`
- `GetClientApplicationCaption()` / `ПолучитьЗаголовокКлиентскогоПриложения()`
- `ClientApplicationBaseFontCurrentVariant()` /
  `ТекущийВариантОсновногоШрифтаКлиентскогоПриложения()`
- `ClientApplicationInterfaceCurrentVariant()` /
  `ТекущийВариантИнтерфейсаКлиентскогоПриложения()`

## Sources

- [1C:Enterprise 8.3.10 platform changelog (RU)](https://dl03.1c.ru/content/Platform/8_3_10_2699/1cv8upd.htm)
