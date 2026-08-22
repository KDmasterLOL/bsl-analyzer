use crate::event::NodeKind;
use crate::parser::Parser;

use super::statements;

pub fn compiler_directive(p: &mut Parser) {
    let m = p.start();
    p.bump();
    m.complete(p, NodeKind::CompilerDirective);
}

pub fn annotation(p: &mut Parser) {
    let m = p.start();
    p.bump();

    if p.at(T![LParen]) {
        annotation_params(p);
    }

    m.complete(p, NodeKind::Annotation);
}

fn annotation_params(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.within_boundary(super::at_paren_list_punctuation, |p| {
        if !p.at(T![RParen]) {
            annotation_param(p);
            while p.eat(T![Comma]) {
                p.check_iteration_limit();
                annotation_param(p);
            }
        }
    });

    p.expect(T![RParen]);

    m.complete(p, NodeKind::AnnotationParams);
}

fn annotation_param(p: &mut Parser) {
    let m = p.start();

    if p.at(T![Ident]) {
        p.bump();
        if p.eat(T![Eq]) {
            annotation_param_value(p);
        }
    } else {
        annotation_param_value(p);
    }

    m.complete(p, NodeKind::AnnotationParam);
}

fn annotation_param_value(p: &mut Parser) {
    match p.current() {
        Some(T![Decimal])
        | Some(T![Float])
        | Some(T![String])
        | Some(T![Date])
        | Some(T![KwTrue])
        | Some(T![KwFalse])
        | Some(T![KwUndefined])
        | Some(T![KwNull]) => {
            p.bump();
        }
        Some(T![Minus]) | Some(T![Plus]) => {
            p.bump();
            if p.at(T![Decimal]) || p.at(T![Float]) {
                p.bump();
            }
        }
        _ => {
            p.error_unexpected();
        }
    }
}

pub fn procedure_def(p: &mut Parser) {
    let m = p.start();
    procedure_def_content(p);
    m.complete(p, NodeKind::ProcedureDef);
}

// A definition states the word that closes it while it parses its header and
// its body, so that a rule tripping over that word leaves it for the
// definition instead of consuming it and reporting it missing at end of file.

fn at_end_procedure(p: &Parser) -> bool {
    p.at(T![KwEndProcedure])
}

fn at_end_function(p: &Parser) -> bool {
    p.at(T![KwEndFunction])
}

/// A keyword is taken as the declared name, and the diagnostic says why not.
///
/// Section 4.2.4.6 reserves keywords against use as the names of declared
/// procedures and functions, so `Процедура Если()` is not the language. The rule
/// takes it regardless: refusing the token here costs the whole body, while
/// naming the violation costs nothing and belongs to a diagnostic —
/// `ReservedWordAsMethodName` reports it at blocker severity. Same for
/// [`function_def_content`].
///
/// Provenance: `docs/legal/bsl-clean-room-slice-b3.md`, finding D5.
pub fn procedure_def_content(p: &mut Parser) {
    p.eat(T![KwAsync]);

    p.expect(T![KwProcedure]);

    p.within_boundary(at_end_procedure, |p| {
        if p.at(T![Ident]) || p.current().is_some_and(|k| k.is_keyword()) {
            p.bump();
        }

        if p.at(T![LParen]) {
            param_list(p);
        }

        p.eat(T![KwExport]);

        statements::stmt_list(p, T![KwEndProcedure]);
    });

    p.expect(T![KwEndProcedure]);
}

pub fn function_def(p: &mut Parser) {
    let m = p.start();
    function_def_content(p);
    m.complete(p, NodeKind::FunctionDef);
}

/// A keyword is taken as the declared name, and the diagnostic says why not.
///
/// The same division as [`procedure_def_content`]: section 4.2.4.6 reserves
/// keywords against the names of declared functions, the rule takes one anyway
/// so that a bad name costs no body, and `ReservedWordAsMethodName` reports it.
///
/// Provenance: `docs/legal/bsl-clean-room-slice-b3.md`, finding D6.
pub fn function_def_content(p: &mut Parser) {
    p.eat(T![KwAsync]);

    p.expect(T![KwFunction]);

    p.within_boundary(at_end_function, |p| {
        if p.at(T![Ident]) || p.current().is_some_and(|k| k.is_keyword()) {
            p.bump();
        }

        if p.at(T![LParen]) {
            param_list(p);
        }

        p.eat(T![KwExport]);

        statements::stmt_list(p, T![KwEndFunction]);
    });

    p.expect(T![KwEndFunction]);
}

fn param_list(p: &mut Parser) {
    let m = p.start();
    p.bump();

    p.within_boundary(super::at_paren_list_punctuation, |p| {
        if !p.at(T![RParen]) {
            param(p);
            while p.eat(T![Comma]) {
                p.check_iteration_limit();
                param(p);
            }
        }
    });

    p.expect(T![RParen]);

    m.complete(p, NodeKind::ParamList);
}

/// The parameter name is optional, which 4.6.3 does not make it.
///
/// `[Знач] <Парам> [=<ДефЗнач>]` is the source's form, and the name is not in
/// brackets there. It is optional here for the line still being typed:
/// refusing would return an error on every keystroke between the paren and the
/// name.
///
/// Provenance: `docs/legal/bsl-clean-room-slice-b3.md`, finding D10.
fn param(p: &mut Parser) {
    let m = p.start();

    p.eat(T![KwVal]);

    if p.at(T![Ident]) {
        p.bump();
    }

    if p.eat(T![Eq]) {
        super::expressions::expression(p);
    }

    m.complete(p, NodeKind::Param);
}

pub fn var_declaration(p: &mut Parser) {
    let m = p.start();
    var_declaration_content(p);
    m.complete(p, NodeKind::VarDef);
}

/// `Экспорт` is taken after the whole list, and after a single name.
///
/// Section 4.6.1 states this three ways that do not agree: the production puts
/// `[Экспорт]` after the first name only, the prose requires it on every
/// declared variable separately, and the example writes `Перем А, Б Экспорт;`.
/// The two forms the section's own examples show are what is read here; the
/// form only its production yields, `Перем А Экспорт, Б;`, is not.
///
/// Provenance: `docs/legal/bsl-clean-room-slice-b3.md`, finding D7.
pub fn var_declaration_content(p: &mut Parser) {
    p.bump();

    if p.at(T![Ident]) {
        p.bump();
    }

    while p.eat(T![Comma]) {
        p.check_iteration_limit();
        if p.at(T![Ident]) {
            p.bump();
        }
    }

    p.eat(T![KwExport]);

    p.eat(T![Semicolon]);
}
