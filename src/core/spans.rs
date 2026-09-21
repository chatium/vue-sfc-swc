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

pub fn rebase_program(program: &mut Program, base: BytePos) {
    program.visit_mut_with(&mut Rebase { base });
}
