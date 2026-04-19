# Cyclomatic Complexity (CyclomaticComplexity)

## Description

Cyclomatic complexity is a classic software metric that estimates how many
independent execution paths a method contains. In practice, higher values
usually mean more branches to understand, more paths to test, and more places
for defects to hide.

This diagnostic reports methods whose cyclomatic complexity exceeds the
configured threshold. The most practical ways to reduce it are decomposition,
guard clauses, and simplification of branching logic.

In this implementation, the metric grows for branch points such as `If`,
`ElsIf`, `Else`, loops, `Except`, `Goto`, logical `AND`/`OR`, and ternary
operators.

## Examples

```bsl
Function ResolveCategory(Amount, ClientType)
    If Amount > 100000 Then
        If ClientType = "Wholesale" Or ClientType = "Dealer" Then
            Return "A";
        ElsIf ClientType = "Retail" Then
            Return "B";
        Else
            Return "C";
        EndIf;
    ElsIf Amount > 50000 Then
        Return "D";
    Else
        Return "E";
    EndIf;
EndFunction
```

## Sources

- [PDepend: Cyclomatic Complexity](https://pdepend.org/documentation/software-metrics/cyclomatic-complexity.html)
- [Cyclomatic complexity](https://en.wikipedia.org/wiki/Cyclomatic_complexity)
