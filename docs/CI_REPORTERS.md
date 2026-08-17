# Репортеры для CI

`bsl-analyzer analyze` умеет писать отчёты в нескольких форматах через флаг
`-r/--reporters` (можно перечислить несколько через запятую). Файлы пишутся в
каталог `-o/--output-dir`.

| Ключ          | Файл                            | Назначение                                  |
|---------------|---------------------------------|---------------------------------------------|
| `console`     | — (stdout)                      | Сводка по числу файлов и замечаний          |
| `json`        | `bsl-json.json`                 | Полный машиночитаемый отчёт                  |
| `sarif`       | `bsl-analyzer.sarif`            | SARIF 2.1.0 (GitHub code scanning и др.)     |
| `codequality` | `gl-code-quality-report.json`   | GitLab Code Quality (виджет в MR)           |
| `junit`       | `bsl-analyzer.junit.xml`        | JUnit XML (вкладка «tests» в CI)            |

## GitLab Code Quality

GitLab показывает такой отчёт виджетом прямо в merge request, аннотируя
новые замечания в диффе. Дифф между base- и head-пайплайном считается по
`fingerprint`, который у нас устойчив к сдвигу строк (хеш от пути, кода правила
и нормализованной строки-источника, а не от номера строки) — вставка кода выше
замечания не помечает его как новое.

```yaml
bsl-analyze:
  stage: test
  script:
    - bsl-analyzer-app analyze -s src -r codequality -o reports -q
  artifacts:
    reports:
      codequality: reports/gl-code-quality-report.json
    paths:
      - reports/gl-code-quality-report.json
    when: always
```

## JUnit XML

Там, где Code Quality недоступен, JUnit-отчёт рендерится нативной вкладкой
«tests»: каждый файл — `<testsuite>`, каждое замечание — упавший `<testcase>` с
кодом правила в `type` и `path:line` в теле `<failure>`.

```yaml
bsl-analyze:
  stage: test
  script:
    - bsl-analyzer-app analyze -s src -r junit -o reports -q
  artifacts:
    reports:
      junit: reports/bsl-analyzer.junit.xml
    when: always
```

Оба репортера можно включить одновременно: `-r codequality,junit`.

Оба формата совместимы с потоковым и инкрементальным режимами `analyze`
(`--incremental`, `--changed-files`, `--git-diff`).

## Шлюз по базовой линии

После добавления `[diagnostics.baseline]` рекомендуемый порядок миграции такой:

```bash
bsl-analyzer-app diagnostics baseline create -s .
# проверить и добавить файл базовой линии в систему контроля версий
bsl-analyzer-app diagnostics baseline check -s .
```

В CI запускайте `check`: команда завершается неуспешно при новых или исчезнувших
замечаниях и не изменяет файл. После ревью намеренного изменения разработчик
обновляет его локально и снова проверяет:

```bash
bsl-analyzer-app diagnostics baseline update -s .
bsl-analyzer-app diagnostics baseline check -s .
```

Инкрементальный и diff-анализ подходят для отчётов, но не доказывают отсутствие
замечаний во всём проекте и потому не допускаются командами изменения базовой линии.
GitLab Code Quality остаётся корневым массивом активных замечаний; сводка базовой
линии добавляется в JSON, JSONL, SARIF и JUnit, но не как фиктивная находка GitLab.
