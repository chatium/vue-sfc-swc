//! swc-backed replacement for the `@babel/parser` calls the JS compiler makes.

use swc_core::common::{FileName, SourceMap, Spanned, sync::Lrc};
use swc_core::ecma::ast::{Expr, Module, Program};
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
            // `parseExpression` must consume the whole input; swc's parser
            // stops at the first complete expression
            let end = expr.span().hi.0.saturating_sub(base.0) as usize;
            if !src[end.min(src.len())..].trim().is_empty() {
                return Err("Unexpected token".to_string());
            }
            let mut expr = *expr;
            super::spans::rebase_expr(&mut expr, base);
            super::spans::strip_parens_expr(&mut expr);
            Ok(expr)
        }
        Err(e) => Err(e.into_kind().msg().to_string()),
    }
}

/// Parses `src` as an ES module, reporting the byte offset of a syntax error.
pub fn parse_module_with_pos(src: &str, ts: bool) -> Result<Module, (String, usize)> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(Lrc::new(FileName::Anon), src.to_string());
    let base = fm.start_pos;
    let lexer = Lexer::new(
        syntax(ts),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );
    let at = |e: swc_core::ecma::parser::error::Error| {
        let pos = (e.span().lo.0.saturating_sub(base.0)) as usize;
        (e.into_kind().msg().to_string(), pos)
    };
    let mut parser = Parser::new_from(lexer);
    match parser.parse_module() {
        Ok(mut module) => {
            if let Some(e) = parser.take_errors().into_iter().next() {
                return Err(at(e));
            }
            super::spans::rebase_module(&mut module, base);
            Ok(module)
        }
        Err(e) => Err(at(e)),
    }
}

/// Parses `src` as an ES module.
pub fn parse_module(src: &str, ts: bool) -> Result<Module, String> {
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
    match parser.parse_module() {
        Ok(mut module) => {
            if let Some(e) = parser.take_errors().into_iter().next() {
                return Err(e.into_kind().msg().to_string());
            }
            super::spans::rebase_module(&mut module, base);
            Ok(module)
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
            super::spans::strip_parens_program(&mut program);
            Ok(program)
        }
        Err(e) => Err(e.into_kind().msg().to_string()),
    }
}
