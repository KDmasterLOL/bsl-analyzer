//! Изъятый тестовый материал в крейт не вернулся.
//!
//! Материал, перенесённый из чужого проекта, однажды уже переехал внутри
//! крейта — из `test_data/` в тело теста, — и удаление файла его не вывело из
//! обращения. Поэтому проверка смотрит на крейт целиком, а не на одно место.
//!
//! Хранится хэш, а не текст: держать текст здесь значило бы держать ровно ту
//! копию, ради устранения которой всё и делается.
//!
//! Provenance: `docs/legal/bsl-clean-room-slice-b2.md`.

use std::path::Path;

/// Отпечаток изъятого материала и его длина в БАЙТАХ.
///
/// Сравнение идёт по байтам, а не по строкам: в исходнике первая строка
/// материала склеена с префиксом объявления (`let code = r#"…`), поэтому
/// построчное окно не совпало бы никогда — такая проверка зелена и при
/// материале на месте. Это проверено: построчный вариант прошёл там, где
/// обязан был упасть.
const RETIRED: &[(&str, u64, usize)] =
    &[("фикстура UnknownPreprocessorSymbol", 0xd6fd_8d85_1562_f0d1, 305)];

const BASE: u64 = 257;

/// Полиномиальный хэш окна, считаемый сдвигом за один проход.
fn window_matches(haystack: &[u8], span: usize, fingerprint: u64) -> bool {
    if haystack.len() < span {
        return false;
    }

    let mut top = 1u64;
    for _ in 1..span {
        top = top.wrapping_mul(BASE);
    }

    let mut hash = 0u64;
    for byte in &haystack[..span] {
        hash = hash.wrapping_mul(BASE).wrapping_add(u64::from(*byte));
    }
    if hash == fingerprint {
        return true;
    }

    for index in span..haystack.len() {
        hash = hash
            .wrapping_sub(u64::from(haystack[index - span]).wrapping_mul(top))
            .wrapping_mul(BASE)
            .wrapping_add(u64::from(haystack[index]));
        if hash == fingerprint {
            return true;
        }
    }

    false
}

/// Файлы крейта, которые Git отслеживает, без разбора расширений.
///
/// Отбор по расширению здесь был бы дырой, а не оптимизацией: материал уже
/// однажды сменил форму — из `.bsl`-фикстуры он переехал в `.rs`, — и ничто не
/// мешает ему вернуться третьей. В крейте лежат ещё и `xml`, и `json`, и
/// словари, а текст, спрятанный в комментарии разметки, остаётся тем же
/// текстом. Нечитаемое как UTF-8 отсеется само при чтении.
///
/// Зато отбор по отслеживаемости обязателен: в поддереве крейта живут
/// игнорируемые каталоги состояния агентских сессий (`.omc/`), и процитированный
/// там старый текст красил бы гейт, ничего не вернув ни в репозиторий, ни в
/// поставку. Именно отслеживаемый файл — то, что попадает в историю и наружу.
fn tracked_files(root: &Path) -> Vec<std::path::PathBuf> {
    let listing =
        std::process::Command::new("git").arg("ls-files").arg("-z").current_dir(root).output();

    if let Ok(listing) = listing {
        if listing.status.success() {
            let paths: Vec<_> = listing
                .stdout
                .split(|byte| *byte == 0)
                .filter(|entry| !entry.is_empty())
                .map(|entry| root.join(String::from_utf8_lossy(entry).as_ref()))
                .collect();
            if !paths.is_empty() {
                return paths;
            }
        }
    }

    // Git недоступен или крейт распакован вне своего репозитория: посторонних
    // файлов в такой поставке нет по построению, поэтому берём поддерево целиком.
    let mut paths = Vec::new();
    every_file(root, &mut paths);
    paths
}

fn every_file(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            every_file(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Ни один файл крейта не содержит изъятого материала.
///
/// Граница проверки названа прямо: она ловит дословное возвращение, а не
/// пересказ — сдвиг одного пробела отпечаток не совпадёт. Пересказ остаётся
/// предметом суждения при ревью, и тест его не заменяет. Гейт закрывает то,
/// что произошло на самом деле: перенос знак в знак при смене формы тестов.
#[test]
fn retired_material_did_not_come_back() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let files = tracked_files(root);

    assert!(
        !files.is_empty(),
        "обход не нашёл ни одного файла — проверка была бы зелена вхолостую"
    );

    let mut breaches = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        for (name, fingerprint, span) in RETIRED {
            if window_matches(text.as_bytes(), *span, *fingerprint) {
                breaches.push(format!("{name} — {}", path.display()));
            }
        }
    }

    assert!(breaches.is_empty(), "изъятый материал на месте:\n  {}", breaches.join("\n  "));
}
