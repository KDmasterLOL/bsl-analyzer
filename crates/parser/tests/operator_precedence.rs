//! Порядок вычисления выражений совпадает с таблицей 4.5.4.
//!
//! Таблица перечисляет операции **в порядке увеличения приоритета** и говорит,
//! что операции с одинаковым приоритетом вычисляются **слева направо**.
//! Направление здесь закрепляется утверждениями, а не формой цепочки вызовов:
//! раздел 4.5.3 даёт свою таблицу уровней логических операций в обратную
//! сторону («Уровень 1 — скобки … Уровень 4 — Или»), а `v8std` перевёрнут
//! относительно 4.5.4 третьим способом. Гейт, не называющий направление, зелен
//! при перевёрнутой цепочке.
//!
//! Утверждение выбрано так, чтобы оно не проходило вхолостую: проверяется, КАКАЯ
//! операция стоит снаружи, а не то, что дерево вообще ветвится. Снаружи стоит
//! самая слабая — в этом и состоит приоритет.
//!
//! Перестановка пары соседних уровней меняет ответ у своего входа — для семи
//! границ из восьми, и это предъявлено. Восьмая, «унарные `+`/`-` против `.` и
//! `()`», НЕ наблюдаема: унарный знак узла не строит, поэтому оба порядка дают
//! буквально одно дерево `EXPR { MINUS, FIELD_EXPR { … } }`, и различать нечего.
//! Мутант, меняющий эти два уровня местами, оставляет проверку зелёной целиком.
//! Оговорка снимается вместе с задачей о собственном узле унарного знака:
//! `https://github.com/itrous/bsl-analyzer/issues/51`.
//!
//! Provenance: `docs/legal/bsl-clean-room-slice-b3.md`.

use syntax::{SyntaxKind, SyntaxNode};

/// Знаки операций выражения. Знак присваивания сюда не входит: поиск начинается
/// с правой части, где его уже нет.
const OPERATORS: &[SyntaxKind] = &[
    SyntaxKind::KW_OR,
    SyntaxKind::KW_AND,
    SyntaxKind::KW_NOT,
    SyntaxKind::EQ,
    SyntaxKind::NEQ,
    SyntaxKind::LT,
    SyntaxKind::LE,
    SyntaxKind::GT,
    SyntaxKind::GE,
    SyntaxKind::PLUS,
    SyntaxKind::MINUS,
    SyntaxKind::STAR,
    SyntaxKind::SLASH,
    SyntaxKind::PERCENT,
    SyntaxKind::DOT,
    SyntaxKind::L_BRACKET,
];

/// Правая часть присваивания — то самое выражение, о котором идёт речь.
fn right_hand_side(input: &str) -> SyntaxNode {
    let root = parser::parse(input).syntax_node();
    let assign = root
        .descendants()
        .find(|node| node.kind() == SyntaxKind::ASSIGN_STMT)
        .unwrap_or_else(|| panic!("во входе {input:?} нет присваивания"));

    assign
        .children()
        .filter(|child| child.kind() == SyntaxKind::EXPR)
        .last()
        .unwrap_or_else(|| panic!("у присваивания во входе {input:?} нет правой части"))
}

/// Знак самой внешней операции выражения.
///
/// Обход в ширину: самый мелкий узел, у которого есть свой знак, и есть самая
/// слабо связывающая операция. Унарные плюс и минус узла не строят и держат
/// знак прямо в `EXPR`, поэтому ищется знак среди СОБСТВЕННЫХ токенов узла, а
/// не вид узла.
fn outermost_operator(input: &str) -> SyntaxKind {
    let mut level = vec![right_hand_side(input)];

    while !level.is_empty() {
        let mut next = Vec::new();

        for node in &level {
            for element in node.children_with_tokens() {
                if let Some(token) = element.as_token() {
                    if OPERATORS.contains(&token.kind()) {
                        return token.kind();
                    }
                }
            }
        }

        for node in &level {
            next.extend(node.children());
        }

        level = next;
    }

    panic!("во входе {input:?} не нашлось ни одного знака операции")
}

/// Каждая пара соседних уровней 4.5.4 различима входом, и разбор согласен с
/// таблицей.
///
/// Слева в паре — операция более слабая по таблице; она и обязана оказаться
/// снаружи. Каждый вход содержит обе операции пары, иначе утверждение было бы
/// зелено при любой реализации.
#[test]
fn the_precedence_ladder_matches_section_4_5_4() {
    let cases: &[(&str, SyntaxKind, &str)] = &[
        ("Х = а Или б И в;", SyntaxKind::KW_OR, "Или слабее И"),
        ("Х = Не а И б;", SyntaxKind::KW_AND, "И слабее Не"),
        ("Х = Не а = б;", SyntaxKind::KW_NOT, "Не слабее сравнения"),
        ("Х = а = б + в;", SyntaxKind::EQ, "сравнение слабее сложения"),
        ("Х = а + б * в;", SyntaxKind::PLUS, "сложение слабее умножения"),
        ("Х = -а * б;", SyntaxKind::STAR, "умножение слабее унарного минуса"),
        ("Х = -а.б;", SyntaxKind::MINUS, "унарный минус слабее разыменования"),
    ];

    let mut breaches = Vec::new();

    for (input, expected, note) in cases {
        let actual = outermost_operator(input);
        if actual != *expected {
            breaches.push(format!(
                "{note}: во входе {input:?} снаружи ожидался {expected:?}, а стоит {actual:?}"
            ));
        }
    }

    assert!(
        breaches.is_empty(),
        "порядок разошёлся с таблицей 4.5.4:\n  {}",
        breaches.join("\n  ")
    );
}

/// Операции одного приоритета вычисляются слева направо.
///
/// Проверяется на вычитании, где две скобковки дают разный результат и порядок
/// поэтому наблюдаем. Цепочка сравнений сюда не берётся: она сама по себе
/// вопрос к `comparison_expr`, и вход, служащий и оракулом, и вопросом, не
/// годится ни тем, ни другим.
#[test]
fn operations_of_equal_precedence_associate_to_the_left() {
    let rhs = right_hand_side("Х = а - б - в;");

    let outer = rhs
        .descendants()
        .find(|node| node.kind() == SyntaxKind::BINARY_EXPR)
        .expect("во входе нет двоичного выражения");

    let left_operand = outer.children().next().expect("у двоичного выражения нет левой части");

    assert!(
        left_operand.descendants().any(|node| node.kind() == SyntaxKind::BINARY_EXPR),
        "левая ассоциативность нарушена: вложенное вычитание оказалось не слева"
    );

    let right_operand = outer.children().last().expect("у двоичного выражения нет правой части");

    assert!(
        !right_operand.descendants().any(|node| node.kind() == SyntaxKind::BINARY_EXPR),
        "левая ассоциативность нарушена: вложенное вычитание оказалось справа"
    );
}
