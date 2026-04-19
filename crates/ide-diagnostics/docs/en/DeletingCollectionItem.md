# Deleting an Item While Iterating a Collection (DeletingCollectionItem)

## Description

Do not remove elements from a collection while iterating over that same
collection with `For each ... In ... Do`.

Removing an element changes the collection structure during enumeration. As a
result, some items can be skipped, the iteration order can become unstable, or
runtime errors can appear depending on the collection type.

If you need to delete items by condition, a safer pattern is to iterate by
index from the end of the collection or to collect elements for deletion in a
separate list first.

## Examples

Incorrect:

```bsl
For each Element In Collection Do
    Collection.Delete(Element);
EndDo;
```

Correct:

```bsl
Index = Collection.Count() - 1;
While Index >= 0 Do
    If ShouldDelete(Collection[Index]) Then
        Collection.Delete(Index);
    EndIf;
    Index = Index - 1;
EndDo;
```

## Sources

- [1C: Programming for Beginners. Development in the system "1C:Enterprise 8.3" (RU)](https://its.1c.ru/db/pubprogforbeginners#content:88:hdoc)
