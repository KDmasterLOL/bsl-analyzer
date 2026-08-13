//! Генератор входов SDBL, общий для свойств парсера.
//!
//! Живёт отдельно, потому что генератор — часть мерила, а не одного теста:
//! два набора свойств, проверяемых на разных входах, между собой несравнимы.
//!
//! Provenance: `docs/legal/sdbl-clean-room-slice12.md`.

/// xorshift64*, so a failure is reproducible from its seed alone.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Pieces of real queries, pieces of broken ones, and pure noise. Mixing
/// them is what produces the shapes nobody writes down: a clause inside a
/// group, a separator inside an unclosed brace, a keyword after a dot.
pub const FRAGMENTS: &[&str] = &[
    "ВЫБРАТЬ",
    "SELECT",
    "УНИЧТОЖИТЬ ВТ",
    "А",
    "Т.Поле",
    "Т.ИЗ",
    "Т .\tГДЕ",
    "ИЗ Справочник.Товары КАК Т",
    "ГДЕ А = 1",
    "СГРУППИРОВАТЬ ПО Н",
    "ИМЕЮЩИЕ СУММА(А) > 0",
    "УПОРЯДОЧИТЬ ПО Н ИЕРАРХИЯ УБЫВ",
    "ИТОГИ СУММА(А) ПО Н ТОЛЬКО ИЕРАРХИЯ",
    "ИТОГИ СУММА(А) ПО П ПЕРИОДАМИ(ДЕНЬ, &Н, &К)",
    "ОБЪЕДИНИТЬ ВСЕ",
    "ЛЕВОЕ СОЕДИНЕНИЕ У ПО Т.А = У.А",
    "ПОМЕСТИТЬ Врем",
    "ДЛЯ ИЗМЕНЕНИЯ",
    "{ГДЕ Т.Поле}",
    "{",
    "}",
    "(",
    ")",
    "(ВЫБРАТЬ Б ИЗ У)",
    "ПЕРИОДАМИ(",
    "КАК",
    ",",
    ";",
    "42",
    "\"строка\"",
    "\"незакрытая",
    "&Параметр",
    "&",
    "ЫЫЫ",
    "%1",
    "#Имя",
    " ",
    "\n",
    "\t",
    "// комментарий\n",
];

pub fn generate(rng: &mut Rng, pieces: usize) -> String {
    let mut out = String::new();
    for _ in 0..pieces {
        out.push_str(FRAGMENTS[rng.below(FRAGMENTS.len())]);
        if rng.below(3) == 0 {
            out.push(' ');
        }
    }
    out
}
