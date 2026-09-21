//! `compiler-dom/src/transforms/validateHtmlNesting.ts` only emits warnings,
//! which `compileTemplate` surfaces as `tips` — never as output or errors — so
//! it is deliberately not ported.

use crate::core::ast::NodeId;
use crate::core::transform::TransformContext;

pub fn validate_html_nesting(_node: NodeId, _ctx: &mut TransformContext) {}
