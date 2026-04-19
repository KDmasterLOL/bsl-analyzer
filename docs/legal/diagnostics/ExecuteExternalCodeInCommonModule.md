# ExecuteExternalCodeInCommonModule provenance

## Assessment

`ExecuteExternalCodeInCommonModule` is a good candidate for `MIT OR Apache-2.0`.

The rule follows directly from the public 1C security guidance in `#std770`, applied to common modules through their metadata flags. The security concern itself is public and not specific to `bsl-language-server`.

The current implementation in `bsl-analyzer` is local:

- it resolves the current common module through the project's own metadata helpers;
- it decides applicability through local checks of `server`, `external_connection`, and `client_ordinary_application` flags;
- it scans the local syntax tree for `EXECUTE_STMT` and global `Eval`/`Вычислить` calls;
- it ignores qualified method calls and respects local configuration such as `ordinary_app_support`.

## Source basis

- 1C standard on restrictions for `Выполнить` and `Вычислить` on the server: <https://its.1c.ru/db/v8std/content/770/hdoc>
- public mirror: <https://v8std.ru/std/770/>

These sources justify the security concern. The module-selection logic is an implementation choice of this project.

## Residual risk

Residual risk is low.

- the rule is grounded in a public security standard;
- the behavior specific to common modules is implemented through local metadata logic;
- the main cleanup needed here was documentation wording to match actual behavior.

## Conclusion

Keep this diagnostic in the `permissive candidate` bucket.
