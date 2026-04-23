# Same metadata object and child name (SameMetadataObjectAndChildNames)

## Description

Child metadata objects should not have the same name as their parent object.

This applies to attributes, dimensions, resources, tabular sections, and tabular section attributes. Reusing the same name makes query expressions ambiguous and increases the risk of mistakes during maintenance.

## Examples

Incorrect names

```text
Catalog.Contractors.TabularSection.Contractors
InformationRegister.SubordinateDocuments.Dimension.SubordinateDocuments
Document.Container.TabularSection.Container.Attribute.Container
```

## Sources

* [Standard: Data storage organization (RU). Name, Synonym, Comment](https://its.1c.ru/db/v8std#content:474:hdoc:2.4)
