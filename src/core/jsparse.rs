//! swc-backed replacement for the `@babel/parser` calls the JS compiler makes.

use swc_core::common::{FileName, SourceMap, sync::Lrc};
use swc_core::ecma::ast::{Expr, Program};
use swc_core::ecma::parser::{EsSyntax, Parser, StringInput, Syntax, TsSyntax, lexer::Lexer};

fn syntax(ts: bool) -> Syntax {
    if ts {
        Syntax::Typescript(TsSyntax {
            tsx: false,
            decorators: true,
            ..Default::default()
        })
    } else {
        Syntax::Es(EsSyntax {
            jsx: false,
            ..Default::default()
        })
    }
}

/// Parses `src` as a single expression. Spans are relative to the start of
/// `src` (byte offsets starting at 0).
pub fn parse_expression(src: &str, ts: bool) -> Result<Expr, String> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(Lrc::new(FileName::Anon), src.to_string());
    let base = fm.start_pos;
    let lexer = Lexer::new(
        syntax(ts),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    match parser.parse_expr() {
        Ok(expr) => {
            if let Some(e) = parser.take_errors().into_iter().next() {
                return Err(e.into_kind().msg().to_string());
            }
            let mut expr = *expr;
            super::spans::rebase_expr(&mut expr, base);
            Ok(expr)
        }
        Err(e) => Err(e.into_kind().msg().to_string()),
    }
}

/// Parses `src` as a program (used for `v-on` multi-statement handlers).
pub fn parse_program(src: &str, ts: bool) -> Result<Program, String> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(Lrc::new(FileName::Anon), src.to_string());
    let base = fm.start_pos;
    let lexer = Lexer::new(
        syntax(ts),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    match parser.parse_program() {
        Ok(program) => {
            if let Some(e) = parser.take_errors().into_iter().next() {
                return Err(e.into_kind().msg().to_string());
            }
            let mut program = program;
            super::spans::rebase_program(&mut program, base);
            Ok(program)
        }
        Err(e) => Err(e.into_kind().msg().to_string()),
    }
}
