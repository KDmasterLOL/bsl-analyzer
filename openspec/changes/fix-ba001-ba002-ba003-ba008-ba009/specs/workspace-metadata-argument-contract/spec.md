## ADDED Requirements

### Requirement: Контракт object_type отражает выбранный источник метаданных
MCP-инструмент `metadata` MUST описывать `object_type` отдельно для source и infobase путей. Для `mode=source` контракт SHALL публиковать singular типы анализатора, включая `Справочник`, `Документ` и `ОбщийМодуль`; для `mode=infobase` и живого пути `mode=auto` с `connection` контракт SHALL публиковать plural имена коллекций сервиса, включая `Справочники` и `Документы`. Сервер MUST NOT обещать и MUST NOT выполнять эвристическую нормализацию между этими формами.

#### Scenario: Подсказка для source mode
- **WHEN** клиент читает описание `metadata object` для работы с `mode=source`
- **THEN** описание приводит singular `object_type` и связывает его с source mode

#### Scenario: Подсказка для infobase mode
- **WHEN** клиент читает описание `metadata object` для `mode=infobase` или `mode=auto` с `connection`
- **THEN** описание приводит plural имя коллекции и не предлагает `Справочник` как допустимый живой аргумент

#### Scenario: Неверная форма не угадывается
- **WHEN** клиент передаёт source-форму в infobase путь или неподдерживаемое имя типа
- **THEN** инструмент возвращает существующую ошибку проверки живого сервиса без автоматической замены аргумента

### Requirement: Опубликованный tools/list сохраняет режим-зависимую подсказку
Описание `metadata`, возвращаемое через `tools/list`, MUST содержать те же mode-dependent правила и примеры, что и репозиторная документация. Изменение текста MUST NOT менять входную JSON Schema, набор действий или формат успешных ответов инструмента.

#### Scenario: Контрактный снимок metadata
- **WHEN** интеграционный тест получает `tools/list` профиля `workspace`
- **THEN** описание `object_type` различает source и infobase формы
- **AND** прежние параметры и действия `metadata` остаются доступны
