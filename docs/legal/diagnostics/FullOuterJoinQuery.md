# FullOuterJoinQuery provenance

## Assessment

`FullOuterJoinQuery` is a good candidate for `MIT OR Apache-2.0` at the rule level, but it should stay in the `needs extra review` bucket because it depends on the SDBL parser and `sdbl-hir` layers that are still under provenance audit.

The rule concept itself is explicitly public and standard-based: `#std435` directly recommends avoiding `FULL OUTER JOIN` in many cases because of performance problems, especially on PostgreSQL.

## Source basis

- 1C standard on restricting `FULL OUTER JOIN` in queries: <https://its.1c.ru/db/v8std/content/435/hdoc>
- Administrator's Guide on PostgreSQL specifics: <https://its.1c.ru/db/metod8dev/content/1556/hdoc>
- public mirror: <https://v8std.ru/std/435/>

These sources are sufficient to justify the performance rationale and the suggested rewrite patterns.

## Implementation notes

The current implementation in `bsl-analyzer` is local at the diagnostic layer:

- `sdbl_hir` emits a `FullOuterJoin` diagnostic with a range;
- the IDE layer maps that range back into the BSL source and formats the message.

There is also an important behavioral caveat: the standard explicitly allows exceptions when a query cannot reasonably be rewritten without `FULL OUTER JOIN`, but the current diagnostic reports every detected occurrence and does not model that exception.

## Residual risk

Residual risk is medium.

- the rule concept is explicitly public and strong;
- however, the implementation depends on SDBL infrastructure that is not fully provenance-cleared yet;
- and the current behavior is stricter than the full wording of the standard because it does not recognize justified exceptions.

## Conclusion

Keep this diagnostic in the `rule is clean, implementation depends on SDBL audit` bucket for now.
