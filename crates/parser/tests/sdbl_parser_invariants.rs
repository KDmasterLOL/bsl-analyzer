//! Properties the SDBL parser must hold for *any* input, checked by
//! generating inputs rather than by naming them.
//!
//! The package loop and its recovery grew by accretion, and hand-written
//! cases only cover the edges someone thought of. These properties are the
//! ones the recovery work is for: nothing leaves the parser unaccounted for,
//! nothing is invented that the input does not contain, every complaint
//! points somewhere real, and no input makes the parser stop responding.
//!
//! Provenance: `docs/legal/sdbl-clean-room-slice12.md`.

use parser::parse_sdbl;
use syntax::SyntaxKind;

/// xorshift64*, so a failure is reproducible from its seed alone.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Pieces of real queries, pieces of broken ones, and pure noise. Mixing
/// them is what produces the shapes nobody writes down: a clause inside a
/// group, a separator inside an unclosed brace, a keyword after a dot.
const FRAGMENTS: &[&str] = &[
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

fn generate(rng: &mut Rng, pieces: usize) -> String {
    let mut out = String::new();
    for _ in 0..pieces {
        out.push_str(FRAGMENTS[rng.below(FRAGMENTS.len())]);
        if rng.below(3) == 0 {
            out.push(' ');
        }
    }
    out
}

/// Every property, checked on one input. Returns the first breach found.
fn breach(input: &str) -> Option<String> {
    let parse = parse_sdbl(input);
    let root = parse.syntax_node();

    // The tree must hold everything the lexer handed over. It is compared
    // against the tokens rather than the input because the lexer itself
    // drops bytes on a run of quotes — a defect of its own, tracked
    // separately, and not one the parser can make good.
    let covered = usize::from(root.text_range().len());
    let handed_over: usize = lexer::sdbl::tokenize_sdbl(input).iter().map(|t| t.text.len()).sum();
    if covered != handed_over {
        return Some(format!("tree covers {covered} of the {handed_over} bytes it was given"));
    }

    // Every reported range must be inside the input and land between
    // characters — a range that splits one panics whoever slices by it.
    for e in parse.errors() {
        let (start, end) = (usize::from(e.range().start()), usize::from(e.range().end()));
        if start > end || end > input.len() {
            return Some(format!("error range {start}..{end} outside 0..{}", input.len()));
        }
        if !input.is_char_boundary(start) || !input.is_char_boundary(end) {
            return Some(format!("error range {start}..{end} splits a character"));
        }
    }

    // A query node comes either from a query keyword in the text or from a
    // member that a clause keyword began, and a member is begun by a
    // separator or by a run-on query. That sum is the ceiling; anything
    // above it is a node the parser invented.
    let keywords = count_query_keywords(input);
    let separators = input.matches(';').count();
    let ceiling = keywords + separators + 1;

    let queries = root.descendants().filter(|n| n.kind() == SyntaxKind::SDBL_QUERY).count();
    if queries > ceiling {
        return Some(format!(
            "{queries} query nodes, but at most {ceiling} could be justified \
             ({keywords} query keywords, {separators} separators)"
        ));
    }

    // Each member is complained about at most once for holding no query, so
    // the same ceiling bounds those complaints.
    let complaints = parse
        .errors()
        .iter()
        .filter(|e| {
            let m = e.message().to_lowercase();
            m.contains("'выбрать' / 'select'") || m.contains("между разделителями")
        })
        .count();
    if complaints > ceiling {
        return Some(format!("{complaints} missing-member complaints, ceiling {ceiling}"));
    }

    // Parsing is a function of the input.
    let again = parse_sdbl(input);
    if again.errors().len() != parse.errors().len()
        || usize::from(again.syntax_node().text_range().len()) != covered
    {
        return Some("parsing the same input twice disagreed".to_string());
    }

    None
}

fn count_query_keywords(input: &str) -> usize {
    let upper = input.to_uppercase();
    ["ВЫБРАТЬ", "SELECT", "УНИЧТОЖИТЬ", "DROP"].iter().map(|k| upper.matches(k).count()).sum()
}

#[test]
fn properties_hold_across_generated_input() {
    let mut failures = Vec::new();

    for seed in 1..=400u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        for pieces in [1usize, 2, 3, 5, 8, 13] {
            let input = generate(&mut rng, pieces);
            if let Some(why) = breach(&input) {
                failures.push(format!("seed {seed}, {pieces} pieces: {why}\n  input: {input:?}"));
                if failures.len() >= 5 {
                    break;
                }
            }
        }
    }

    assert!(failures.is_empty(), "properties broken:\n{}", failures.join("\n"));
}

#[test]
fn properties_hold_on_every_truncation_of_a_real_query() {
    // Truncation is what an editor shows the parser on every keystroke, and
    // it reaches states no hand-written case does.
    let queries = [
        "ВЫБРАТЬ РАЗЛИЧНЫЕ ПЕРВЫЕ 10 Т.Код КАК К ИЗ Справочник.Товары КАК Т \
         ГДЕ Т.Активен СГРУППИРОВАТЬ ПО Т.Код ИМЕЮЩИЕ КОЛИЧЕСТВО(*) > 1 \
         УПОРЯДОЧИТЬ ПО Т.Код ИЕРАРХИЯ УБЫВ \
         ИТОГИ СУММА(Т.Код) ПО Т.Код ПЕРИОДАМИ(ДЕНЬ, &Н, &К) КАК Г",
        "ВЫБРАТЬ А ПОМЕСТИТЬ Врем ИЗ Т ЛЕВОЕ СОЕДИНЕНИЕ У ПО Т.А = У.А; \
         ВЫБРАТЬ Б ИЗ Врем {ГДЕ Б.Поле} ОБЪЕДИНИТЬ ВСЕ ВЫБРАТЬ В ИЗ W",
        "ВЫБРАТЬ Т.Номенклатура ИЗ РегистрНакопления.Т.Остатки(&Период, \
         Номенклатура В (ВЫБРАТЬ С.Ссылка ИЗ Справочник.Номенклатура КАК С)) КАК Т",
    ];

    for query in queries {
        for cut in 0..=query.len() {
            if !query.is_char_boundary(cut) {
                continue;
            }
            let prefix = &query[..cut];
            if let Some(why) = breach(prefix) {
                panic!("truncated at {cut}: {why}\n  input: {prefix:?}");
            }
        }
    }
}

#[test]
fn a_long_malformed_package_does_not_stall() {
    // Not a timing assertion — a quadratic path simply never returns, so
    // reaching the assert at all is the property.
    let mut rng = Rng(0xC0FF_EE00_1234_5678);
    let mut input = String::new();
    for _ in 0..20_000 {
        input.push_str(FRAGMENTS[rng.below(FRAGMENTS.len())]);
        input.push(' ');
    }

    assert!(breach(&input).is_none(), "{:?}", breach(&input));
}
