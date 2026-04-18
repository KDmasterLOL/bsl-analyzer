# All execution paths of a function must have a Return statement (AllFunctionPathMustHaveReturn)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description
In BSL, if execution reaches `EndFunction` without an explicit `Return`, the function returns `Undefined`.

This behavior is part of the language, but in practice an implicit `Undefined` is usually not intended. A missing `Return` often appears after adding a new branch, editing an `ElsIf` chain, or handling only the "main" scenario and forgetting the fallback case.

This diagnostic reports functions where at least one execution path reaches the end of the function without an explicit `Return`. If returning `Undefined` is intentional, write it explicitly as `Return Undefined;`.

## Examples

### Incorrect

```bsl
// If the category is not handled, the function implicitly returns Undefined.
Function CalculateDiscountRate(Val CustomerCategory)
    If CustomerCategory = "VIP" Then
        Return 0.15;
    ElsIf CustomerCategory = "Regular" Then
        Return 0.10;
    ElsIf CustomerCategory = "New" Then
        Return 0.05;
    EndIf;

    // implicit return Undefined
EndFunction
```

### Correct

```
// An explicit fallback makes the behavior clear.
Function CalculateDiscountRate(Val CustomerCategory)
    If CustomerCategory = "VIP" Then
        Return 0.15;
    ElsIf CustomerCategory = "Regular" Then
        Return 0.10;
    ElsIf CustomerCategory = "New" Then
        Return 0.05;
    EndIf;

    Return 0;
EndFunction
```

### Another example of incorrect code:

```bsl
Function ResolveDeliveryMode(Val Order)
    If Order.IsExpress Then
        Return "Express";
    ElsIf Order.HasPickupPoint Then
        LogPickupChoice(Order);
    Else
        Return "Courier";
    EndIf;
EndFunction
```
