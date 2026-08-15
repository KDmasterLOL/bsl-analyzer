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
    collect(&root, &mut files);
    files.sort();

    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else { continue };
        let name = path.strip_prefix(&root).unwrap_or(path).display();

        report(&format!("{name}"), &text, sdbl);
        if truncations {
            for (label, cut) in [("1/3", text.len() / 3), ("2/3", text.len() * 2 / 3)] {
                let cut = floor_char_boundary(&text, cut);
                report(&format!("{name}~{label}"), &text[..cut], sdbl);
            }
        }
    }
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

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.is_file() {
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
