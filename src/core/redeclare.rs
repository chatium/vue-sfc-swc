//! The slice of Babel's `ScopeHandler.checkRedeclarationInScope` that swc's
//! parser leaves out: `Identifier 'x' has already been declared.`
//!
//! Only conflicts Babel always rejects are reported, since a false positive
//! would fail a valid file: two lexical bindings (`let`/`const`/`class`, a
//! module- or block-level `function`, an import) in one scope, a lexical one
//! against a `var` or a parameter there. Types, `declare`, overloads, enums and
//! namespaces are left alone, as are the cases Babel also rejects but this
//! does not track (a `var` hoisting past an inner `let`, a catch parameter).

use swc_core::atoms::Atom;
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{Visit, VisitWith};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Lexical,
    Var,
    Param,
}

struct Scope {
    names: Vec<(Atom, Kind)>,
    /// a function body's top level, where a `function` binds like a `var`
    function_top: bool,
}

#[derive(Default)]
struct Checker {
    scopes: Vec<Scope>,
    error: Option<(String, usize)>,
}

/// The first redeclaration in source order: Babel's message and the byte
/// offset of the second binding.
pub fn first_redeclaration(module: &Module) -> Option<(String, usize)> {
    let mut c = Checker::default();
    c.scoped(false, |c| module.visit_children_with(c));
    c.error
}

impl Checker {
    fn scoped(&mut self, function_top: bool, f: impl FnOnce(&mut Self)) {
        self.scopes.push(Scope {
            names: Vec::new(),
            function_top,
        });
        f(self);
        self.scopes.pop();
    }

    fn declare(&mut self, id: &Ident, kind: Kind) {
        if self.error.is_some() {
            return;
        }
        let scope = self.scopes.last_mut().unwrap();
        // ponytail: linear scan per scope; a HashMap if a scope ever holds thousands
        let clash = scope.names.iter().any(|(n, k)| {
            *n == id.sym
                && match kind {
                    Kind::Lexical => true,
                    Kind::Var | Kind::Param => *k == Kind::Lexical,
                }
        });
        if clash {
            self.error = Some((
                format!("Identifier '{}' has already been declared.", id.sym),
                id.span.lo.0 as usize,
            ));
        }
        scope.names.push((id.sym.clone(), kind));
    }

    fn declare_pat(&mut self, pat: &Pat, kind: Kind) {
        let mut ids = Vec::new();
        collect_binding_idents(pat, &mut ids);
        for id in ids {
            self.declare(id, kind);
        }
    }

    fn function_kind(&self) -> Kind {
        if self.scopes.last().unwrap().function_top {
            Kind::Var
        } else {
            Kind::Lexical
        }
    }

    /// A function's params and body share its top-level scope. Babel's
    /// `checkParams` declares the params after the body, so a `let` there
    /// that reuses a param's name is reported at the param.
    fn function_scope(&mut self, params: &[&Pat], body: Option<&[Stmt]>) {
        self.scoped(true, |c| {
            params.iter().for_each(|p| p.visit_with(c));
            if let Some(stmts) = body {
                stmts.visit_with(c);
            }
            for p in params {
                c.declare_pat(p, Kind::Param);
            }
        });
    }
}

fn collect_binding_idents<'a>(pat: &'a Pat, out: &mut Vec<&'a Ident>) {
    match pat {
        Pat::Ident(b) => out.push(&b.id),
        Pat::Array(a) => a.elems.iter().flatten().for_each(|p| collect_binding_idents(p, out)),
        Pat::Rest(r) => collect_binding_idents(&r.arg, out),
        Pat::Object(o) => {
            for p in &o.props {
                match p {
                    ObjectPatProp::KeyValue(kv) => collect_binding_idents(&kv.value, out),
                    ObjectPatProp::Assign(a) => out.push(&a.key.id),
                    ObjectPatProp::Rest(r) => collect_binding_idents(&r.arg, out),
                }
            }
        }
        Pat::Assign(a) => collect_binding_idents(&a.left, out),
        Pat::Invalid(_) | Pat::Expr(_) => {}
    }
}

impl Visit for Checker {
    fn visit_import_decl(&mut self, n: &ImportDecl) {
        if n.type_only {
            return;
        }
        for s in &n.specifiers {
            let (local, type_only) = match s {
                ImportSpecifier::Named(s) => (&s.local, s.is_type_only),
                ImportSpecifier::Default(s) => (&s.local, false),
                ImportSpecifier::Namespace(s) => (&s.local, false),
            };
            if !type_only {
                self.declare(local, Kind::Lexical);
            }
        }
    }

    fn visit_var_decl(&mut self, n: &VarDecl) {
        if n.declare {
            return;
        }
        let kind = if n.kind == VarDeclKind::Var {
            Kind::Var
        } else {
            Kind::Lexical
        };
        for d in &n.decls {
            self.declare_pat(&d.name, kind);
            d.name.visit_with(self);
            d.init.visit_with(self);
        }
    }

    fn visit_fn_decl(&mut self, n: &FnDecl) {
        // an overload signature or `declare function` binds nothing
        if n.declare || n.function.body.is_none() {
            return;
        }
        let kind = self.function_kind();
        self.declare(&n.ident, kind);
        n.function.visit_with(self);
    }

    fn visit_class_decl(&mut self, n: &ClassDecl) {
        if n.declare {
            return;
        }
        self.declare(&n.ident, Kind::Lexical);
        n.class.visit_with(self);
    }

    fn visit_function(&mut self, n: &Function) {
        n.decorators.visit_with(self);
        let params: Vec<&Pat> = n.params.iter().map(|p| &p.pat).collect();
        self.function_scope(&params, n.body.as_ref().map(|b| &b.stmts[..]));
    }

    fn visit_arrow_expr(&mut self, n: &ArrowExpr) {
        let params: Vec<&Pat> = n.params.iter().collect();
        match &*n.body {
            ArrowFunctionBody::FunctionBody(b) => self.function_scope(&params, Some(&b.stmts)),
            ArrowFunctionBody::Expr(e) => self.scoped(true, |c| {
                params.iter().for_each(|p| p.visit_with(c));
                e.visit_with(c);
            }),
        }
    }

    fn visit_constructor(&mut self, n: &Constructor) {
        let params: Vec<&Pat> = n
            .params
            .iter()
            .filter_map(|p| match p {
                ParamOrTsParamProp::Param(p) => Some(&p.pat),
                ParamOrTsParamProp::TsParamProp(_) => None,
            })
            .collect();
        self.function_scope(&params, n.body.as_ref().map(|b| &b.stmts[..]));
    }

    fn visit_static_block(&mut self, n: &StaticBlock) {
        self.function_scope(&[], Some(&n.body.stmts));
    }

    fn visit_block_stmt(&mut self, n: &BlockStmt) {
        self.scoped(false, |c| n.stmts.visit_with(c));
    }

    fn visit_for_stmt(&mut self, n: &ForStmt) {
        self.scoped(false, |c| n.visit_children_with(c));
    }

    fn visit_for_in_stmt(&mut self, n: &ForInStmt) {
        self.scoped(false, |c| n.visit_children_with(c));
    }

    fn visit_for_of_stmt(&mut self, n: &ForOfStmt) {
        self.scoped(false, |c| n.visit_children_with(c));
    }

    fn visit_switch_stmt(&mut self, n: &SwitchStmt) {
        n.discriminant.visit_with(self);
        self.scoped(false, |c| n.cases.visit_with(c));
    }

    fn visit_catch_clause(&mut self, n: &CatchClause) {
        self.scoped(false, |c| {
            n.param.visit_with(c);
            n.body.visit_with(c);
        });
    }

    // a function or class expression's own name is bound inside it alone
    fn visit_fn_expr(&mut self, n: &FnExpr) {
        n.function.visit_with(self);
    }

    fn visit_class_expr(&mut self, n: &ClassExpr) {
        n.class.visit_with(self);
    }

    // namespaces, `declare module` and type-level code bind nothing here
    fn visit_ts_module_decl(&mut self, _: &TsModuleDecl) {}
    fn visit_ts_enum_decl(&mut self, _: &TsEnumDecl) {}
    fn visit_ts_interface_decl(&mut self, _: &TsInterfaceDecl) {}
    fn visit_ts_type_alias_decl(&mut self, _: &TsTypeAliasDecl) {}
    fn visit_ts_type(&mut self, _: &TsType) {}
}
