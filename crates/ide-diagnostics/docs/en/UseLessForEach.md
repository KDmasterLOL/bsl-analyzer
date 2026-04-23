# Useless collection iteration (UseLessForEach)

## Description

If the iterator variable is not used inside a `For Each` loop body, the iteration is usually pointless. In practice this often means either:

- the developer forgot to use the iterator;
- the loop is unnecessary and collection processing should be moved outside the loop.

The current implementation is narrower than a full semantic loop analysis:

- it reports loops already marked during HIR lowering as having an unused iterator;
- property access, passing the iterator as an argument, assignments involving the iterator, and method calls on the iterator count as usage;
- the handler suppresses the diagnostic if the iterator name matches a module-level variable name, to avoid a known false-positive pattern.

## Examples

### Incorrect

```bsl
For Each Iterator From Collection Loop
    ProcessCollection(Collection);
EndLoop;
```

### Correct

```bsl
For Each Iterator From Collection Loop
    ProcessElement(Iterator);
EndLoop;
```

### Also correct if per-item processing is not needed

```bsl
ProcessCollection(Collection);
```

## Sources

- Generic maintainability guidance for loop bodies.
- [v8std.ru: UseLessForEach](https://v8std.ru/diagnostics/bslls/UseLessForEach/)
