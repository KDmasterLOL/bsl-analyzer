//! Deterministic generator of large BSL modules for incrementality stands.
//!
//! A real 70 000-line module cannot ship with the tests, but the property
//! under test — "an edit inside one method body re-executes the work of that
//! method only" — needs a module with hundreds of methods whose shape is
//! known: which method calls which, where a body statement can be inserted,
//! where a parameter can be added. The generator produces exactly that and
//! reports the byte offsets alongside the text, so a test never has to
//! search the text for an edit site.
//!
//! Every knob is a period (`every`): `0` disables the feature, `1` applies it
//! to every method, `n` to every `n`-th method. Method `i` calls method
//! `i - 1`, so every method but the last has exactly one caller in the file.

use std::fmt::Write as _;

/// Shape of a generated module.
#[derive(Debug, Clone)]
pub struct SyntheticModuleSpec {
    /// Number of procedures and functions, alternating (even index → procedure).
    pub methods: usize,
    /// Assignment statements per body.
    pub statements_per_method: usize,
    /// Every n-th method builds a query from a string literal.
    pub query_every: usize,
    /// Every n-th method wraps one statement in `#Если … #КонецЕсли`.
    pub preproc_inside_every: usize,
    /// Every n-th method is itself wrapped in `#Если … #КонецЕсли`.
    pub preproc_around_every: usize,
    /// Every n-th method carries a `&НаСервере` directive.
    pub annotate_every: usize,
    /// Every n-th method has a doc comment above it.
    pub docstring_every: usize,
    /// Every n-th method also calls the NEXT method, closing a call cycle
    /// (`i` calls `i - 1`, `i - 1` calls `i`) so inference of the pair runs as
    /// a salsa fixpoint — the shape a real module has and a chain does not.
    pub call_next_every: usize,
    /// Every n-th function returns only under `Если`, leaving a path that falls
    /// off the end: the shape that makes the lowering flag a missing return
    /// and the path-termination dataflow run for that body.
    pub conditional_return_every: usize,
    /// Module-level `Перем` declarations before the first method.
    pub module_vars: usize,
    /// Line terminator; `"\r\n"` reproduces Windows-authored modules.
    pub newline: &'static str,
}

impl Default for SyntheticModuleSpec {
    fn default() -> Self {
        Self {
            methods: 200,
            statements_per_method: 8,
            query_every: 10,
            preproc_inside_every: 7,
            preproc_around_every: 0,
            annotate_every: 5,
            docstring_every: 3,
            call_next_every: 0,
            conditional_return_every: 0,
            module_vars: 2,
            newline: "\n",
        }
    }
}

/// One generated method and the edit sites a stand needs.
#[derive(Debug, Clone)]
pub struct SyntheticMethod {
    /// Position among the module's methods.
    pub index: usize,
    pub name: String,
    pub is_function: bool,
    /// Byte offset of the first body statement's leading tab: inserting a
    /// `\tИмя = 1;<newline>` here adds a statement without touching the
    /// signature.
    pub body_insert_offset: u32,
    /// Byte offset right after the opening parenthesis of the parameter list:
    /// inserting `Имя, ` here adds a parameter.
    pub signature_insert_offset: u32,
    /// Byte range of the whole declaration — its `#Если` wrapper, doc comment
    /// and directive included — up to and including the blank line after it,
    /// so inserting a method at `start` or removing `start..end` leaves the
    /// neighbours' trivia with their own methods.
    pub block: std::ops::Range<u32>,
    /// Methods in this module that call this one.
    pub callers: usize,
}

/// A generated module: its text plus the layout of every method.
#[derive(Debug, Clone)]
pub struct SyntheticModule {
    pub text: String,
    pub methods: Vec<SyntheticMethod>,
}

impl SyntheticModule {
    /// Text with `\tИмя = 1;` inserted at the body of method `index`.
    pub fn with_body_statement(&self, index: usize, name: &str, newline: &str) -> String {
        let at = self.methods[index].body_insert_offset as usize;
        let mut text = self.text.clone();
        text.insert_str(at, &format!("\t{name} = 1;{newline}"));
        text
    }

    /// Text with `Имя, ` inserted at the head of method `index`'s parameters.
    pub fn with_parameter(&self, index: usize, name: &str) -> String {
        let at = self.methods[index].signature_insert_offset as usize;
        let mut text = self.text.clone();
        text.insert_str(at, &format!("{name}, "));
        text
    }

    /// A whole new procedure `name` in front of method `index`: every method
    /// from `index` on moves one top-level position down.
    pub fn with_method_inserted_before(&self, index: usize, name: &str, newline: &str) -> String {
        let at = self.methods[index].block.start as usize;
        let mut text = self.text.clone();
        text.insert_str(at, &method_text(name, false, newline));
        text
    }

    /// A whole new procedure `name` after the last method: nobody moves.
    pub fn with_method_appended(&self, name: &str, newline: &str) -> String {
        let mut text = self.text.clone();
        text.push_str(&method_text(name, false, newline));
        text
    }

    /// A second declaration of method `index`'s own name in front of it, of
    /// the same kind: the original becomes the second of two same-named
    /// methods, every later method keeps its name and its place among its
    /// namesakes.
    pub fn with_duplicate_inserted_above(&self, index: usize, newline: &str) -> String {
        let at = self.methods[index].block.start as usize;
        let mut text = self.text.clone();
        text.insert_str(at, &self.namesake_of(index, newline));
        text
    }

    /// A minimal declaration bearing method `index`'s name and kind, for a
    /// caller composing its own edit sequence.
    pub fn namesake_of(&self, index: usize, newline: &str) -> String {
        let method = &self.methods[index];
        method_text(&method.name, method.is_function, newline)
    }

    /// Text without method `index` at all — its wrapper, doc comment and
    /// directive go with it.
    pub fn with_method_removed(&self, index: usize) -> String {
        let block = &self.methods[index].block;
        let mut text = self.text.clone();
        text.replace_range(block.start as usize..block.end as usize, "");
        text
    }
}

/// A minimal exported method with one statement, terminated by a blank line.
fn method_text(name: &str, is_function: bool, newline: &str) -> String {
    if is_function {
        format!(
            "Функция {name}() Экспорт{newline}\tВставка = 1;{newline}\tВозврат Вставка;{newline}КонецФункции{newline}{newline}"
        )
    } else {
        format!("Процедура {name}() Экспорт{newline}\tВставка = 1;{newline}КонецПроцедуры{newline}{newline}")
    }
}

fn every(period: usize, i: usize) -> bool {
    period != 0 && i.is_multiple_of(period)
}

impl SyntheticModuleSpec {
    pub fn build(&self) -> SyntheticModule {
        let nl = self.newline;
        let mut text = String::new();
        for v in 0..self.module_vars {
            let _ = write!(text, "Перем МодульнаяПеременная{v};{nl}");
        }
        if self.module_vars > 0 {
            text.push_str(nl);
        }

        let mut methods = Vec::with_capacity(self.methods);
        for i in 0..self.methods {
            let is_function = i % 2 == 1;
            let name = if is_function {
                format!("Функция{i:04}")
            } else {
                format!("Метод{i:04}")
            };
            let around = every(self.preproc_around_every, i);
            let block_start = text.len() as u32;

            if around {
                let _ = write!(text, "#Если Сервер Тогда{nl}");
            }
            if every(self.docstring_every, i) {
                let _ = write!(text, "// Описание метода {name}.{nl}");
            }
            if every(self.annotate_every, i) {
                let _ = write!(text, "&НаСервере{nl}");
            }
            let keyword = if is_function { "Функция" } else { "Процедура" };
            let _ = write!(text, "{keyword} {name}(");
            let signature_insert_offset = text.len() as u32;
            let _ = write!(text, "Пар1, Пар2 = 0) Экспорт{nl}");
            let body_insert_offset = text.len() as u32;

            for s in 0..self.statements_per_method {
                let inside = s == 0 && every(self.preproc_inside_every, i);
                if inside {
                    let _ = write!(text, "#Если Сервер Тогда{nl}");
                }
                let _ = write!(text, "\tЛокальная{s} = Пар1 + Пар2 + {s};{nl}");
                if inside {
                    let _ = write!(text, "#КонецЕсли{nl}");
                }
            }
            if every(self.query_every, i) {
                let _ = write!(
                    text,
                    "\tЗапрос = Новый Запрос(\"ВЫБРАТЬ 1 КАК Поле\");{nl}\tРезультат = Запрос.Выполнить();{nl}"
                );
            }
            if i > 0 {
                let callee: &SyntheticMethod = &methods[i - 1];
                if callee.is_function {
                    let _ = write!(text, "\tИтог = {}(Пар1, Пар2);{nl}", callee.name);
                } else {
                    let _ = write!(text, "\t{}(Пар1, Пар2);{nl}", callee.name);
                }
            }
            if every(self.call_next_every, i) && i + 1 < self.methods {
                // The next method's kind is known from its index alone.
                let next_is_function = (i + 1) % 2 == 1;
                let next_name = if next_is_function {
                    format!("Функция{:04}", i + 1)
                } else {
                    format!("Метод{:04}", i + 1)
                };
                if next_is_function {
                    let _ = write!(text, "\tИтогВперёд = {next_name}(Пар1, Пар2);{nl}");
                } else {
                    let _ = write!(text, "\t{next_name}(Пар1, Пар2);{nl}");
                }
            }
            if is_function {
                if every(self.conditional_return_every, i) {
                    let _ = write!(
                        text,
                        "\tЕсли Пар1 > 0 Тогда{nl}\t\tВозврат Пар1;{nl}\tКонецЕсли;{nl}"
                    );
                } else {
                    let _ = write!(text, "\tВозврат Пар1;{nl}");
                }
            }
            let end = if is_function {
                "КонецФункции"
            } else {
                "КонецПроцедуры"
            };
            let _ = write!(text, "{end}{nl}");
            if around {
                let _ = write!(text, "#КонецЕсли{nl}");
            }
            text.push_str(nl);

            methods.push(SyntheticMethod {
                index: i,
                name,
                is_function,
                body_insert_offset,
                signature_insert_offset,
                block: block_start..text.len() as u32,
                callers: 0,
            });
        }
        // Callers are counted from the calls actually written: the chain gives
        // every method but the last one caller; a forward call adds one more.
        for i in 1..methods.len() {
            methods[i - 1].callers += 1;
            if every(self.call_next_every, i - 1) {
                methods[i].callers += 1;
            }
        }

        SyntheticModule { text, methods }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_sites_point_where_they_claim() {
        let module = SyntheticModuleSpec { methods: 6, ..Default::default() }.build();
        assert_eq!(module.methods.len(), 6);
        for m in &module.methods {
            let sig = &module.text[m.signature_insert_offset as usize..];
            assert!(sig.starts_with("Пар1, Пар2 = 0)"), "{}: {sig:.30}", m.name);
            let body = &module.text[m.body_insert_offset as usize..];
            assert!(body.starts_with('\t') || body.starts_with("#Если"), "{}: {body:.30}", m.name);
        }
        assert_eq!(module.methods[0].callers, 1);
        assert_eq!(module.methods[5].callers, 0);
    }

    #[test]
    fn body_edit_changes_only_the_body_and_signature_edit_only_the_head() {
        let module = SyntheticModuleSpec { methods: 4, ..Default::default() }.build();
        let body = module.with_body_statement(1, "Правка", "\n");
        assert!(body.contains("\tПравка = 1;\n"));
        assert_eq!(body.len(), module.text.len() + "\tПравка = 1;\n".len());
        let sig = module.with_parameter(1, "Новый");
        assert!(sig.contains("Функция0001(Новый, Пар1, Пар2 = 0)"));
    }

    #[test]
    fn method_blocks_carry_their_trivia_and_wrapper() {
        let module = SyntheticModuleSpec {
            methods: 4,
            preproc_around_every: 2,
            docstring_every: 1,
            annotate_every: 1,
            ..Default::default()
        }
        .build();
        let block = |i: usize| {
            let b = &module.methods[i].block;
            &module.text[b.start as usize..b.end as usize]
        };
        assert!(block(0).starts_with(
            "#Если Сервер Тогда\n// Описание метода Метод0000.\n&НаСервере\nПроцедура Метод0000("
        ));
        assert!(block(0).ends_with("КонецПроцедуры\n#КонецЕсли\n\n"));
        assert!(block(1)
            .starts_with("// Описание метода Функция0001.\n&НаСервере\nФункция Функция0001("));
        assert!(block(1).ends_with("КонецФункции\n\n"));
        // The blocks tile the text after the module variables with no gaps.
        assert_eq!(module.methods[0].block.end, module.methods[1].block.start);
        assert_eq!(module.methods[3].block.end as usize, module.text.len());

        let inserted = module.with_method_inserted_before(1, "Новая", "\n");
        assert!(inserted.contains("#КонецЕсли\n\nПроцедура Новая() Экспорт\n\tВставка = 1;\nКонецПроцедуры\n\n// Описание метода Функция0001."));
        let appended = module.with_method_appended("Хвост", "\n");
        assert!(appended.starts_with(&module.text));
        assert!(appended.ends_with("Процедура Хвост() Экспорт\n\tВставка = 1;\nКонецПроцедуры\n\n"));
        let duplicated = module.with_duplicate_inserted_above(1, "\n");
        assert_eq!(duplicated.matches("Функция Функция0001(").count(), 2);
        let removed = module.with_method_removed(1);
        // The declaration is gone; the call to it from method 2 stays.
        assert!(!removed.contains("Функция Функция0001("));
        assert!(removed.contains("= Функция0001(Пар1, Пар2);"));
        assert!(removed.contains("#КонецЕсли\n\n#Если Сервер Тогда\n// Описание метода Метод0002."));
        assert_eq!(removed.len(), module.text.len() - block(1).len());
    }

    #[test]
    fn forward_calls_are_counted_as_callers() {
        let module =
            SyntheticModuleSpec { methods: 4, call_next_every: 1, ..Default::default() }.build();
        // Method 1 is called by 2 (chain) and by 0 (forward); the last one by 2 only.
        assert_eq!(module.methods.iter().map(|m| m.callers).collect::<Vec<_>>(), [1, 2, 2, 1]);
        assert!(
            module.text.contains("Метод0000(Пар1, Пар2);")
                && module.text.contains("Функция0001(Пар1, Пар2);")
        );
    }

    #[test]
    fn periods_are_honoured() {
        let spec = SyntheticModuleSpec {
            methods: 3,
            preproc_around_every: 1,
            query_every: 0,
            newline: "\r\n",
            ..Default::default()
        };
        let module = spec.build();
        // Three wrappers plus the default `preproc_inside_every = 7` hit on method 0.
        assert_eq!(module.text.matches("#Если Сервер Тогда\r\n").count(), 3 + 1);
        assert!(!module.text.contains("Новый Запрос"));
        assert_eq!(module.text.matches('\n').count(), module.text.matches("\r\n").count());
    }
}
