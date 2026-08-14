# Preprocessor instruction split across lines

The parts of a preprocessor instruction — the condition, its operands, the
closing `Then` — must stand on the line the instruction starts on. The platform
refuses such a split, even though the construct is syntactically unambiguous and
reads without difficulty.

The check covers `#If` and `#ElsIf`. The body of the instruction is full of line
breaks by construction and is not a split: the boundary is the closing `Then`.

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

## Notes

A name after `#Область` carried to the next line is not covered here: there the
break changes the parse itself — the region does not take the name — and the
parser reports it instead.

An instruction with no `Then` is skipped: it is already broken, and the parser
says so. A second message about the same place adds nothing.
