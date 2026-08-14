# Preprocessor instruction split across lines

The parts of a preprocessor instruction — the condition, its operands, the
closing `Then` — must stand on the line the instruction starts on. The platform
refuses such a split, even though the construct is syntactically unambiguous and
reads without difficulty.

The check covers `#If`, `#ElsIf` and the name after `#Region`. The body of the
instruction is full of line breaks by construction and is not a split: the
boundary is the closing `Then`.

## Incorrect

```bsl
#Если
Сервер Тогда
	А = 1;
#КонецЕсли

#Если Сервер
И Клиент Тогда
	Б = 2;
#КонецЕсли

#Если Сервер
Тогда
	В = 3;
#КонецЕсли
```

## Correct

```bsl
#Если Сервер Тогда
	А = 1;
#КонецЕсли

#Если Сервер И Клиент Тогда
	Б = 2;
#КонецЕсли
```

## The region name

```bsl
// Incorrect
#Область
Служебные

// Correct
#Область Служебные
```

Here the break changes the parse as well: a name past the line break is not
taken by the region, or the directive would steal the first word of the next
statement. Such a name is left standing as a word of its own, and the parser
reports it as an invalid statement. That message is true, but it names the
consequence; this check names the cause.

A carried name is recognised by three signs at once: the directive has no name,
exactly one significant word follows it, and no semicolon stands after that
word. A parenless call (`Метод;`), a statement (`А = 1`) and a stray word after
an already named region therefore do not land here.

## Notes

Nothing is claimed here about a region with no name at all.

An instruction with no `Then` is skipped: it is already broken, and the parser
says so. A second message about the same place adds nothing.
