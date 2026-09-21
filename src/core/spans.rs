//! Shifts swc spans so offsets are relative to the parsed source, matching
//! Babel's 0-based `node.start` / `node.end`.

use swc_core::common::{BytePos, Span};
use swc_core::ecma::ast::{Expr, Program};
use swc_core::ecma::visit::{VisitMut, VisitMutWith};

struct Rebase {
    base: BytePos,
}

impl VisitMut for Rebase {
    fn visit_mut_span(&mut self, span: &mut Span) {
        if span.lo.0 >= self.base.0 {
            span.lo = BytePos(span.lo.0 - self.base.0);
        }
        if span.hi.0 >= self.base.0 {
            span.hi = BytePos(span.hi.0 - self.base.0);
        }
    }
}

pub fn rebase_expr(expr: &mut Expr, base: BytePos) {
    expr.visit_mut_with(&mut Rebase { base });
}

/// Babel drops `ParenthesizedExpression` nodes by default; swc keeps them, so
/// strip them to keep node types (and `node.start`) comparable.
struct StripParens;

impl VisitMut for StripParens {
    fn visit_mut_expr(&mut self, e: &mut Expr) {
        e.visit_mut_children_with(self);
        if let Expr::Paren(p) = e {
            let inner = (*p.expr).clone();
            *e = inner;
        }
    }
}

pub fn strip_parens_expr(expr: &mut Expr) {
    expr.visit_mut_with(&mut StripParens);
}

pub fn strip_parens_program(program: &mut Program) {
    program.visit_mut_with(&mut StripParens);
}

pub fn rebase_program(program: &mut Program, base: BytePos) {
    program.visit_mut_with(&mut Rebase { base });
}
