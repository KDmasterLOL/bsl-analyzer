# Assignment to a Global-context property

Fires on an assignment to a bare name that denotes a Global-context property — for example a metadata manager collection (`Справочники`, `Документы`, `Перечисления`, `РегистрыСведений`, …), in either the Russian or the English spelling.

## Why this is a problem

The name belongs to the global context rather than to your module, and its properties are read-only. Assigning to one **does not declare a local variable**: the platform refuses the write, and throughout the method body the name keeps denoting the same collection.

Two consequences follow. The assignment itself has no effect a reader would expect. And every later read of the name works against the platform collection rather than your value — environment restrictions included: a collection accessed from a client method stays unavailable there no matter how much you assign to it.

## Examples

Wrong:

```bsl
Процедура Тест()
    Справочники = Новый Структура("Код", 1); // <-- writes to a Global-context property
    Справочники.Вставить("Имя", "Товар");    // does not operate on your structure
КонецПроцедуры
```

Correct — pick a name the platform does not own:

```bsl
Процедура Тест()
    ДанныеСправочника = Новый Структура("Код", 1);
    ДанныеСправочника.Вставить("Имя", "Товар");
КонецПроцедуры
```

The assignment is legal when your own code declares the name — as a parameter, a `Перем`, a form attribute or a loop variable. The declared owner then holds the name and the diagnostic stays silent:

```bsl
Процедура Тест(Справочники) // declared as a parameter — the name is yours
    Справочники = Новый Структура("Код", 1);
КонецПроцедуры
```

## Suppressing

If such an assignment is deliberate in your configuration, the diagnostic can be turned off the usual way in the analyzer settings.
