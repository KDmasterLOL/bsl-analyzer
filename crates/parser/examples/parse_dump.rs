//! Отпечаток разбора корпуса — для дифференциального прогона двумя сборками.
//!
//! Корпус задаётся аргументом и в репозиторий не кладётся. Вывод —
//! построчный и отсортированный по пути, чтобы `diff` двух прогонов был
//! сравнением разбора, а не порядка обхода каталога.
//!
//! ```text
//! cargo run --release --example parse_dump -- <каталог> [--sdbl] [--truncations]
//! ```
//!
//! Паника — отдельный исход, а не отсутствие строки: сборка, падающая на
//! первом же файле, иначе даёт пустой вывод, который читается как «разошлось
//! всё». Строка `PANIC` отличает падение от расхождения дерева.

use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

fn main() {
    let mut args = std::env::args_os().skip(1);
    let Some(root) = args.next().map(PathBuf::from) else {
        eprintln!("usage: parse_dump <каталог> [--sdbl] [--truncations]");
        std::process::exit(2);
    };

    let mut sdbl = false;
    let mut truncations = false;
    for arg in args {
        match arg.to_str() {
            Some("--sdbl") => sdbl = true,
            Some("--truncations") => truncations = true,
            other => {
                eprintln!("parse_dump: неизвестный аргумент {other:?}");
                std::process::exit(2);
            }
        }
    }

    // Паники здесь ожидаемы и подсчитываются; без глушителя 3 000 backtrace'ов
    // тонут вывод, ради которого прогон и запущен.
    panic::set_hook(Box::new(|_| {}));

    let mut files = Vec::new();
    let mut unreadable_dirs = 0usize;
    collect(&root, &mut files, &mut unreadable_dirs);
    files.sort();

    // Корпус, которого не оказалось, даёт пустой вывод, а два пустых вывода
    // дают пустой дифф — то есть тот же знак, что и «ничего не разошлось».
    // Опечатка в пути читалась бы как выполненный инвариант.
    if files.is_empty() {
        eprintln!("parse_dump: в {} не нашлось ни одного файла", root.display());
        std::process::exit(3);
    }

    let mut read_errors = 0usize;
    for path in &files {
        let name = path.strip_prefix(&root).unwrap_or(path).display();
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            // Строка идёт в общий поток, а не в stderr: файл, который читается
            // одной сборкой и не читается другой, обязан быть виден диффом.
            Err(err) => {
                read_errors += 1;
                println!("{name}\tREAD_ERR\t{}", err.kind());
                continue;
            }
        };

        report(&format!("{name}"), &text, sdbl);
        if truncations {
            for (label, cut) in [("1/3", text.len() / 3), ("2/3", text.len() * 2 / 3)] {
                let cut = floor_char_boundary(&text, cut);
                report(&format!("{name}~{label}"), &text[..cut], sdbl);
            }
        }
    }

    // Сводка в stderr, а не в общий поток: она о прогоне, а не о разборе, и в
    // диффе двух сборок ей делать нечего. Но без неё «ноль расхождений»
    // ничего не говорит о том, на скольких файлах этот ноль получен.
    eprintln!(
        "parse_dump: файлов {}, нечитаемых {read_errors}, нечитаемых каталогов {unreadable_dirs}",
        files.len()
    );
}

fn report(name: &str, text: &str, sdbl: bool) {
    match panic::catch_unwind(AssertUnwindSafe(|| digest(text, sdbl))) {
        Ok((tree, errors, count)) => println!("{name}\t{tree:016x}\t{count}\t{errors:016x}"),
        Err(_) => println!("{name}\tPANIC"),
    }
}

/// Отпечаток дерева и отпечаток списка ошибок считаются раздельно: правка,
/// сдвигающая только диапазоны подчёркиваний, иначе неотличима от правки,
/// перестроившей дерево.
fn digest(text: &str, sdbl: bool) -> (u64, u64, usize) {
    let parse = if sdbl { parser::parse_sdbl(text) } else { parser::parse(text) };
    let tree = fnv1a(format!("{:#?}", parse.syntax_node()).as_bytes());
    let errors = parse.errors();
    let rendered = errors.iter().map(|e| format!("{e:?}")).collect::<Vec<_>>().join("\n");
    (tree, fnv1a(rendered.as_bytes()), errors.len())
}

/// Обход каталога без перехода по символическим ссылкам на каталоги.
///
/// Спрашивается тип самой записи, а не тип того, куда она указывает: `is_dir`
/// идёт по ссылке, и ссылка на предка заводит обход в петлю. Ядро обрывает её
/// на сороковом звене, поэтому она не падает, а молча повторяет корпус сорок
/// один раз — исход хуже падения, потому что выглядит как работа.
///
/// Ссылка на ФАЙЛ по-прежнему разыменовывается: корпус собирается ссылками, и
/// иначе в нём не оказалось бы ни одного файла.
fn collect(dir: &Path, out: &mut Vec<PathBuf>, unreadable: &mut usize) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => {
            *unreadable += 1;
            return;
        }
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else { continue };
        let path = entry.path();
        if file_type.is_dir() {
            collect(&path, out, unreadable);
        } else if file_type.is_file() || (file_type.is_symlink() && path.is_file()) {
            out.push(path);
        }
    }
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}
