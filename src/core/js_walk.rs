//! Port of `compiler-core/src/babelUtils.ts` — identifier walking with scope
//! tracking, over swc's AST instead of Babel's.
//!
//! Rather than a generic walker plus Babel's `isReferenced`, each syntactic
//! position is handled explicitly, which is what `isReferenced` encodes anyway.

use std::collections::HashMap;

use swc_core::ecma::ast::*;

#[derive(Debug, Clone)]
pub struct AssignInfo {
    pub op: String,
    pub right_start: u32,
    pub right_end: u32,
}

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub prefix: bool,
    pub op: String,
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone)]
pub struct IdRef {
    pub name: String,
    /// byte offsets into the parsed source
    pub start: u32,
    pub end: u32,
    pub is_referenced: bool,
    pub is_local: bool,
    /// `{ foo }` shorthand property — the key must be re-emitted
    pub shorthand_prop: bool,
    /// parent is CallExpression / NewExpression / MemberExpression
    pub parent_call_new_member: bool,
    pub no_parent: bool,
    pub assign_left: Option<AssignInfo>,
    pub update_arg: Option<UpdateInfo>,
    pub in_destructure_assignment: bool,
    pub in_new_expression: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ParentKind {
    None,
    Call,
    New,
    Member,
    Other,
}

/// Models `Object.create(context.identifiers)`: a prototype chain where only
/// own keys are reported back as the expression's `identifiers`.
#[derive(Debug, Clone, Default)]
pub struct KnownIds {
    pub base: HashMap<String, i32>,
    pub own: HashMap<String, i32>,
}

impl KnownIds {
    pub fn new(base: HashMap<String, i32>) -> Self {
        KnownIds {
            base,
            own: HashMap::new(),
        }
    }
    pub fn get(&self, k: &str) -> i32 {
        self.own
            .get(k)
            .or_else(|| self.base.get(k))
            .copied()
            .unwrap_or(0)
    }
    pub fn mark(&mut self, k: &str) {
        let v = self.get(k);
        self.own.insert(k.to_string(), v + 1);
    }
    pub fn unmark(&mut self, k: &str) {
        if let Some(v) = self.own.get_mut(k) {
            *v -= 1;
            if *v == 0 {
                self.own.remove(k);
            }
        }
    }
    /// `Object.keys(knownIds)`
    pub fn own_keys(&self) -> Vec<String> {
        self.own.keys().cloned().collect()
    }
    /// flattened view, for passing down to a nested `processExpression`
    pub fn flat(&self) -> HashMap<String, i32> {
        let mut m = self.base.clone();
        for (k, v) in &self.own {
            m.insert(k.clone(), *v);
        }
        m
    }
}

pub struct Walker<'a> {
    pub known_ids: &'a mut KnownIds,
    include_all: bool,
    out: Vec<IdRef>,
    scope_stack: Vec<Vec<String>>,
    in_destructure_assignment: bool,
    in_new_expression: bool,
    /// Vue keeps the *root* node's scope ids in `knownIds` (`node !== rootExp`)
    suppress_root_pop: bool,
}

impl<'a> Walker<'a> {
    pub fn new(known_ids: &'a mut KnownIds, include_all: bool) -> Self {
        Walker {
            known_ids,
            include_all,
            out: Vec::new(),
            scope_stack: Vec::new(),
            in_destructure_assignment: false,
            in_new_expression: false,
            suppress_root_pop: false,
        }
    }

    pub fn finish(self) -> Vec<IdRef> {
        self.out
    }

    fn push_scope(&mut self) {
        self.scope_stack.push(Vec::new());
    }

    fn pop_scope(&mut self) {
        if self.suppress_root_pop && self.scope_stack.len() == 1 {
            self.suppress_root_pop = false;
            self.scope_stack.pop();
            return;
        }
        if let Some(ids) = self.scope_stack.pop() {
            for id in ids {
                self.known_ids.unmark(&id);
            }
        }
    }

    /// `markScopeIdentifier` — one increment per distinct name per scope.
    fn mark_scope(&mut self, name: &str) {
        if let Some(top) = self.scope_stack.last() {
            if top.iter().any(|n| n == name) {
                return;
            }
        }
        self.known_ids.mark(name);
        if let Some(top) = self.scope_stack.last_mut() {
            top.push(name.to_string());
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.known_ids.get(name) > 0
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        name: &str,
        span: Span,
        is_referenced: bool,
        parent: ParentKind,
        shorthand_prop: bool,
        assign_left: Option<AssignInfo>,
        update_arg: Option<UpdateInfo>,
    ) {
        let is_local = self.is_local(name);
        if !(self.include_all || (is_referenced && !is_local)) {
            return;
        }
        self.out.push(IdRef {
            name: name.to_string(),
            start: span.lo.0,
            end: span.hi.0,
            is_referenced,
            is_local,
            shorthand_prop,
            parent_call_new_member: matches!(
                parent,
                ParentKind::Call | ParentKind::New | ParentKind::Member
            ),
            no_parent: parent == ParentKind::None,
            assign_left,
            update_arg,
            in_destructure_assignment: self.in_destructure_assignment,
            in_new_expression: self.in_new_expression,
        });
    }

    fn ref_ident(&mut self, ident: &Ident, parent: ParentKind) {
        self.emit(&ident.sym, ident.span, true, parent, false, None, None);
    }

    // --- expressions --------------------------------------------------------

    pub fn walk_expr(&mut self, e: &Expr) {
        if matches!(e, Expr::Arrow(_) | Expr::Fn(_) | Expr::Class(_)) {
            self.suppress_root_pop = true;
        }
        self.walk_expr_with(e, ParentKind::None)
    }

    /// `isInNewExpression(parentStack)` walks up from the identifier through
    /// member expressions only, so the flag is inherited across a member and
    /// cleared by anything else.
    fn walk_expr_with(&mut self, e: &Expr, parent: ParentKind) {
        let saved = self.in_new_expression;
        self.in_new_expression = match parent {
            ParentKind::New => true,
            ParentKind::Member => saved,
            _ => false,
        };
        self.walk_expr_at(e, parent);
        self.in_new_expression = saved;
    }

    fn walk_expr_at(&mut self, e: &Expr, parent: ParentKind) {
        match e {
            Expr::Ident(i) => self.ref_ident(i, parent),
            Expr::This(_) | Expr::Lit(_) | Expr::PrivateName(_) | Expr::Invalid(_) => {}
            Expr::Array(a) => {
                for el in a.elems.iter().flatten() {
                    self.walk_expr_with(&el.expr, ParentKind::Other);
                }
            }
            Expr::Object(o) => {
                for p in &o.props {
                    match p {
                        PropOrSpread::Spread(s) => self.walk_expr_with(&s.expr, ParentKind::Other),
                        PropOrSpread::Prop(prop) => self.walk_prop(prop),
                    }
                }
            }
            Expr::Fn(f) => {
                self.push_scope();
                if let Some(id) = &f.ident {
                    self.mark_scope(&id.sym);
                    self.emit(&id.sym, id.span, false, ParentKind::Other, false, None, None);
                }
                self.walk_function(&f.function);
                self.pop_scope();
            }
            Expr::Arrow(a) => {
                self.push_scope();
                for p in &a.params {
                    self.mark_pat(p);
                }
                for p in &a.params {
                    self.walk_pat_binding(p);
                }
                match &*a.body {
                    ArrowFunctionBody::FunctionBody(b) => self.walk_block(&b.stmts),
                    ArrowFunctionBody::Expr(e) => self.walk_expr_with(e, ParentKind::Other),
                }
                self.pop_scope();
            }
            Expr::Unary(u) => self.walk_expr_with(&u.arg, ParentKind::Other),
            Expr::Update(u) => {
                if let Expr::Ident(i) = &*u.arg {
                    let name = i.sym.to_string();
                    let info = UpdateInfo {
                        prefix: u.prefix,
                        op: u.op.as_str().to_string(),
                        start: u.span.lo.0,
                        end: u.span.hi.0,
                    };
                    self.emit(
                        &name,
                        i.span,
                        true,
                        ParentKind::Other,
                        false,
                        None,
                        Some(info),
                    );
                } else {
                    self.walk_expr_with(&u.arg, ParentKind::Other);
                }
            }
            Expr::Bin(b) => {
                self.walk_expr_with(&b.left, ParentKind::Other);
                self.walk_expr_with(&b.right, ParentKind::Other);
            }
            Expr::Assign(a) => {
                let info = AssignInfo {
                    op: a.op.as_str().to_string(),
                    right_start: a.right.span().lo.0,
                    right_end: a.right.span().hi.0,
                };
                self.walk_assign_target(&a.left, Some(info));
                self.walk_expr_with(&a.right, ParentKind::Other);
            }
            Expr::Member(m) => self.walk_member(m),
            Expr::SuperProp(s) => {
                if let SuperProp::Computed(c) = &s.prop {
                    self.walk_expr_with(&c.expr, ParentKind::Other);
                }
            }
            Expr::Cond(c) => {
                self.walk_expr_with(&c.test, ParentKind::Other);
                self.walk_expr_with(&c.cons, ParentKind::Other);
                self.walk_expr_with(&c.alt, ParentKind::Other);
            }
            Expr::Call(c) => {
                match &c.callee {
                    Callee::Expr(e) => self.walk_expr_with(e, ParentKind::Call),
                    Callee::Super(_) | Callee::Import(_) => {}
                }
                for a in &c.args {
                    self.walk_expr_with(&a.expr, ParentKind::Call);
                }
            }
            Expr::New(n) => {
                self.walk_expr_with(&n.callee, ParentKind::New);
                if let Some(args) = &n.args {
                    for a in args {
                        self.walk_expr_with(&a.expr, ParentKind::New);
                    }
                }
            }
            Expr::Seq(s) => {
                for e in &s.exprs {
                    self.walk_expr_with(e, ParentKind::Other);
                }
            }
            Expr::Tpl(t) => {
                for e in &t.exprs {
                    self.walk_expr_with(e, ParentKind::Other);
                }
            }
            Expr::TaggedTpl(t) => {
                self.walk_expr_with(&t.tag, ParentKind::Other);
                for e in &t.tpl.exprs {
                    self.walk_expr_with(e, ParentKind::Other);
                }
            }
            Expr::Class(c) => {
                self.push_scope();
                if let Some(id) = &c.ident {
                    self.mark_scope(&id.sym);
                    self.emit(&id.sym, id.span, false, ParentKind::Other, false, None, None);
                }
                self.walk_class(&c.class);
                self.pop_scope();
            }
            Expr::Yield(y) => {
                if let Some(a) = &y.arg {
                    self.walk_expr_with(a, ParentKind::Other);
                }
            }
            Expr::MetaProp(_) => {}
            Expr::Await(a) => self.walk_expr_with(&a.arg, ParentKind::Other),
            Expr::Paren(p) => self.walk_expr_with(&p.expr, parent),
            Expr::OptChain(o) => match &*o.base {
                OptChainBase::Member(m) => self.walk_member(m),
                OptChainBase::Call(c) => {
                    self.walk_expr_with(&c.callee, ParentKind::Call);
                    for a in &c.args {
                        self.walk_expr_with(&a.expr, ParentKind::Other);
                    }
                }
            },
            // TS wrappers are transparent (TS_NODE_TYPES); type annotations are skipped
            Expr::TsAs(t) => self.walk_expr_with(&t.expr, parent),
            Expr::TsSatisfies(t) => self.walk_expr_with(&t.expr, parent),
            Expr::TsNonNull(t) => self.walk_expr_with(&t.expr, parent),
            Expr::TsTypeAssertion(t) => self.walk_expr_with(&t.expr, parent),
            Expr::TsInstantiation(t) => self.walk_expr_with(&t.expr, parent),
            Expr::TsConstAssertion(t) => self.walk_expr_with(&t.expr, parent),
            Expr::JSXMember(_)
            | Expr::JSXNamespacedName(_)
            | Expr::JSXEmpty(_)
            | Expr::JSXElement(_)
            | Expr::JSXFragment(_) => {}
        }
    }

    fn walk_member(&mut self, m: &MemberExpr) {
        self.walk_expr_with(&m.obj, ParentKind::Member);
        match &m.prop {
            MemberProp::Ident(i) => {
                // non-computed member property: reported, but not a reference
                self.emit(
                    &i.sym,
                    i.span,
                    false,
                    ParentKind::Member,
                    false,
                    None,
                    None,
                );
            }
            MemberProp::Computed(c) => self.walk_expr_with(&c.expr, ParentKind::Member),
            MemberProp::PrivateName(_) => {}
        }
    }

    fn walk_prop(&mut self, p: &Prop) {
        match p {
            Prop::Shorthand(i) => {
                self.emit(
                    &i.sym,
                    i.span,
                    true,
                    ParentKind::Other,
                    true,
                    None,
                    None,
                );
            }
            Prop::KeyValue(kv) => {
                self.walk_prop_name(&kv.key);
                self.walk_expr_with(&kv.value, ParentKind::Other);
            }
            Prop::Assign(a) => {
                // `{ foo = bar }` only valid in patterns
                self.emit(&a.key.sym, a.key.span, true, ParentKind::Other, true, None, None);
                self.walk_expr_with(&a.value, ParentKind::Other);
            }
            Prop::Getter(g) => {
                self.walk_prop_name(&g.key);
                self.push_scope();
                self.walk_function(&g.function);
                self.pop_scope();
            }
            Prop::Setter(s) => {
                self.walk_prop_name(&s.key);
                self.push_scope();
                self.walk_function(&s.function);
                self.pop_scope();
            }
            Prop::Method(m) => {
                self.walk_prop_name(&m.key);
                self.push_scope();
                self.walk_function(&m.function);
                self.pop_scope();
            }
        }
    }

    fn walk_prop_name(&mut self, key: &PropName) {
        if let PropName::Computed(c) = key {
            self.walk_expr_with(&c.expr, ParentKind::Other);
        }
        // static keys are skipped by `isStaticPropertyKey` in the callback
    }

    fn walk_assign_target(&mut self, t: &AssignTarget, info: Option<AssignInfo>) {
        match t {
            AssignTarget::Simple(s) => match s {
                SimpleAssignTarget::Ident(b) => {
                    self.emit(
                        &b.id.sym,
                        b.id.span,
                        true,
                        ParentKind::Other,
                        false,
                        info,
                        None,
                    );
                }
                SimpleAssignTarget::Member(m) => self.walk_member(m),
                SimpleAssignTarget::Paren(p) => self.walk_expr_with(&p.expr, ParentKind::Other),
                SimpleAssignTarget::OptChain(_) | SimpleAssignTarget::SuperProp(_) => {}
                SimpleAssignTarget::TsAs(t) => self.walk_expr_with(&t.expr, ParentKind::Other),
                SimpleAssignTarget::TsSatisfies(t) => {
                    self.walk_expr_with(&t.expr, ParentKind::Other)
                }
                SimpleAssignTarget::TsNonNull(t) => self.walk_expr_with(&t.expr, ParentKind::Other),
                SimpleAssignTarget::TsTypeAssertion(t) => {
                    self.walk_expr_with(&t.expr, ParentKind::Other)
                }
                SimpleAssignTarget::TsInstantiation(t) => {
                    self.walk_expr_with(&t.expr, ParentKind::Other)
                }
                SimpleAssignTarget::Invalid(_) => {}
            },
            AssignTarget::Pat(p) => {
                let saved = self.in_destructure_assignment;
                self.in_destructure_assignment = true;
                match p {
                    AssignTargetPat::Array(a) => {
                        for el in a.elems.iter().flatten() {
                            self.walk_pat_as_target(el);
                        }
                    }
                    AssignTargetPat::Object(o) => {
                        for prop in &o.props {
                            match prop {
                                ObjectPatProp::KeyValue(kv) => {
                                    self.walk_prop_name(&kv.key);
                                    self.walk_pat_as_target(&kv.value);
                                }
                                ObjectPatProp::Assign(a) => {
                                    self.emit(
                                        &a.key.id.sym,
                                        a.key.id.span,
                                        true,
                                        ParentKind::Other,
                                        true,
                                        None,
                                        None,
                                    );
                                    if let Some(v) = &a.value {
                                        self.walk_expr_with(v, ParentKind::Other);
                                    }
                                }
                                ObjectPatProp::Rest(r) => self.walk_pat_as_target(&r.arg),
                            }
                        }
                    }
                    AssignTargetPat::Invalid(_) => {}
                }
                self.in_destructure_assignment = saved;
            }
        }
    }

    /// A pattern used as an assignment target: its identifiers are references.
    fn walk_pat_as_target(&mut self, p: &Pat) {
        match p {
            Pat::Ident(b) => {
                self.emit(&b.id.sym, b.span(), true, ParentKind::Other, false, None, None);
            }
            Pat::Array(a) => {
                for el in a.elems.iter().flatten() {
                    self.walk_pat_as_target(el);
                }
            }
            Pat::Object(o) => {
                for prop in &o.props {
                    match prop {
                        ObjectPatProp::KeyValue(kv) => {
                            self.walk_prop_name(&kv.key);
                            self.walk_pat_as_target(&kv.value);
                        }
                        ObjectPatProp::Assign(a) => {
                            self.emit(
                                &a.key.id.sym,
                                a.key.id.span,
                                true,
                                ParentKind::Other,
                                true,
                                None,
                                None,
                            );
                            if let Some(v) = &a.value {
                                self.walk_expr_with(v, ParentKind::Other);
                            }
                        }
                        ObjectPatProp::Rest(r) => self.walk_pat_as_target(&r.arg),
                    }
                }
            }
            Pat::Rest(r) => self.walk_pat_as_target(&r.arg),
            Pat::Assign(a) => {
                self.walk_pat_as_target(&a.left);
                self.walk_expr_with(&a.right, ParentKind::Other);
            }
            Pat::Expr(e) => self.walk_expr_with(e, ParentKind::Other),
            Pat::Invalid(_) => {}
        }
    }

    // --- declarations / scopes ---------------------------------------------

    fn mark_pat(&mut self, p: &Pat) {
        for name in extract_pat_idents(p) {
            self.mark_scope(&name);
        }
    }

    /// Emits the pattern's binding identifiers (Babel reports every
    /// `Identifier`, and a typed param's range covers its type annotation) and
    /// walks any default-value expressions.
    fn walk_pat_binding(&mut self, p: &Pat) {
        match p {
            Pat::Ident(b) => {
                self.emit(&b.id.sym, b.span(), false, ParentKind::Other, false, None, None);
            }
            Pat::Array(a) => {
                for el in a.elems.iter().flatten() {
                    self.walk_pat_binding(el);
                }
            }
            Pat::Object(o) => {
                for prop in &o.props {
                    match prop {
                        ObjectPatProp::KeyValue(kv) => {
                            self.walk_prop_name(&kv.key);
                            self.walk_pat_binding(&kv.value);
                        }
                        ObjectPatProp::Assign(a) => {
                            self.emit(
                                &a.key.id.sym,
                                a.key.span(),
                                false,
                                ParentKind::Other,
                                false,
                                None,
                                None,
                            );
                            if let Some(v) = &a.value {
                                self.walk_expr_with(v, ParentKind::Other);
                            }
                        }
                        ObjectPatProp::Rest(r) => self.walk_pat_binding(&r.arg),
                    }
                }
            }
            Pat::Rest(r) => self.walk_pat_binding(&r.arg),
            Pat::Assign(a) => {
                self.walk_pat_binding(&a.left);
                self.walk_expr_with(&a.right, ParentKind::Other);
            }
            Pat::Expr(e) => self.walk_expr_with(e, ParentKind::Other),
            Pat::Invalid(_) => {}
        }
    }

    fn walk_function(&mut self, f: &Function) {
        for p in &f.params {
            self.mark_pat(&p.pat);
        }
        for p in &f.params {
            self.walk_pat_binding(&p.pat);
        }
        if let Some(b) = &f.body {
            self.walk_block(&b.stmts);
        }
    }

    fn walk_class(&mut self, c: &Class) {
        if let Some(sc) = &c.super_class {
            self.walk_expr_with(sc, ParentKind::Other);
        }
        for m in &c.body {
            match m {
                ClassMember::Method(m) => {
                    self.walk_prop_name(&m.key);
                    self.push_scope();
                    self.walk_function(&m.function);
                    self.pop_scope();
                }
                ClassMember::PrivateMethod(m) => {
                    self.push_scope();
                    self.walk_function(&m.function);
                    self.pop_scope();
                }
                ClassMember::ClassProp(p) => {
                    self.walk_prop_name(&p.key);
                    if let Some(v) = &p.value {
                        self.walk_expr_with(v, ParentKind::Other);
                    }
                }
                ClassMember::PrivateProp(p) => {
                    if let Some(v) = &p.value {
                        self.walk_expr_with(v, ParentKind::Other);
                    }
                }
                ClassMember::StaticBlock(b) => {
                    self.push_scope();
                    self.walk_block(&b.body.stmts);
                    self.pop_scope();
                }
                ClassMember::Constructor(c) => {
                    self.push_scope();
                    for p in &c.params {
                        if let ParamOrTsParamProp::Param(p) = p {
                            self.mark_pat(&p.pat);
                        }
                    }
                    if let Some(b) = &c.body {
                        self.walk_block(&b.stmts);
                    }
                    self.pop_scope();
                }
                ClassMember::Empty(_) | ClassMember::TsIndexSignature(_) => {}
                ClassMember::AutoAccessor(_) => {}
            }
        }
    }

    fn walk_block(&mut self, stmts: &[Stmt]) {
        self.push_scope();
        self.mark_block_declarations(stmts);
        for s in stmts {
            self.walk_stmt(s);
        }
        self.pop_scope();
    }

    /// `walkBlockDeclarations` — non-`var` declarations plus function/class decls.
    fn mark_block_declarations(&mut self, body: &[Stmt]) {
        for stmt in body {
            match stmt {
                Stmt::Decl(Decl::Var(v)) => {
                    if v.declare {
                        continue;
                    }
                    for d in &v.decls {
                        for name in extract_pat_idents(&d.name) {
                            self.mark_scope(&name);
                        }
                    }
                }
                Stmt::Decl(Decl::Fn(f)) => {
                    if f.declare {
                        continue;
                    }
                    self.mark_scope(&f.ident.sym);
                }
                Stmt::Decl(Decl::Class(c)) => {
                    if c.declare {
                        continue;
                    }
                    self.mark_scope(&c.ident.sym);
                }
                Stmt::For(f) => self.walk_for_decl(f.init.as_ref().and_then(as_var_decl), true),
                Stmt::ForIn(f) => self.walk_for_decl(for_head_var(&f.left), true),
                Stmt::ForOf(f) => self.walk_for_decl(for_head_var(&f.left), true),
                Stmt::Switch(s) => self.mark_switch(s, true),
                _ => {}
            }
        }
    }

    fn walk_for_decl(&mut self, decl: Option<&VarDecl>, is_var: bool) {
        if let Some(v) = decl {
            let kind_is_var = v.kind == VarDeclKind::Var;
            if kind_is_var == is_var {
                for d in &v.decls {
                    for name in extract_pat_idents(&d.name) {
                        self.mark_scope(&name);
                    }
                }
            }
        }
    }

    fn mark_switch(&mut self, s: &SwitchStmt, is_var: bool) {
        for case in &s.cases {
            for stmt in &case.cons {
                if let Stmt::Decl(Decl::Var(v)) = stmt {
                    let kind_is_var = v.kind == VarDeclKind::Var;
                    if kind_is_var == is_var {
                        for d in &v.decls {
                            for name in extract_pat_idents(&d.name) {
                                self.mark_scope(&name);
                            }
                        }
                    }
                }
            }
            self.mark_block_declarations(&case.cons);
        }
    }

    pub fn walk_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Expr(e) => self.walk_expr_with(&e.expr, ParentKind::None),
            Stmt::Block(b) => self.walk_block(&b.stmts),
            Stmt::Empty(_) | Stmt::Debugger(_) => {}
            Stmt::With(w) => {
                self.walk_expr_with(&w.obj, ParentKind::Other);
                self.walk_stmt(&w.body);
            }
            Stmt::Return(r) => {
                if let Some(a) = &r.arg {
                    self.walk_expr_with(a, ParentKind::Other);
                }
            }
            Stmt::Labeled(l) => self.walk_stmt(&l.body),
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::If(i) => {
                self.walk_expr_with(&i.test, ParentKind::Other);
                self.walk_stmt(&i.cons);
                if let Some(a) = &i.alt {
                    self.walk_stmt(a);
                }
            }
            Stmt::Switch(s) => {
                self.walk_expr_with(&s.discriminant, ParentKind::Other);
                self.push_scope();
                self.mark_switch(s, false);
                for case in &s.cases {
                    if let Some(t) = &case.test {
                        self.walk_expr_with(t, ParentKind::Other);
                    }
                    for stmt in &case.cons {
                        self.walk_stmt(stmt);
                    }
                }
                self.pop_scope();
            }
            Stmt::Throw(t) => self.walk_expr_with(&t.arg, ParentKind::Other),
            Stmt::Try(t) => {
                self.walk_block(&t.block.stmts);
                if let Some(h) = &t.handler {
                    self.push_scope();
                    if let Some(p) = &h.param {
                        self.mark_pat(p);
                        self.walk_pat_binding(p);
                    }
                    self.walk_block(&h.body.stmts);
                    self.pop_scope();
                }
                if let Some(f) = &t.finalizer {
                    self.walk_block(&f.stmts);
                }
            }
            Stmt::While(w) => {
                self.walk_expr_with(&w.test, ParentKind::Other);
                self.walk_stmt(&w.body);
            }
            Stmt::DoWhile(d) => {
                self.walk_stmt(&d.body);
                self.walk_expr_with(&d.test, ParentKind::Other);
            }
            Stmt::For(f) => {
                self.push_scope();
                self.walk_for_decl(f.init.as_ref().and_then(as_var_decl), false);
                if let Some(init) = &f.init {
                    match init {
                        VarDeclOrExpr::VarDecl(v) => self.walk_var_decl(v),
                        VarDeclOrExpr::Expr(e) => self.walk_expr_with(e, ParentKind::Other),
                    }
                }
                if let Some(t) = &f.test {
                    self.walk_expr_with(t, ParentKind::Other);
                }
                if let Some(u) = &f.update {
                    self.walk_expr_with(u, ParentKind::Other);
                }
                self.walk_stmt(&f.body);
                self.pop_scope();
            }
            Stmt::ForIn(f) => {
                self.push_scope();
                self.walk_for_decl(for_head_var(&f.left), false);
                self.walk_for_head(&f.left);
                self.walk_expr_with(&f.right, ParentKind::Other);
                self.walk_stmt(&f.body);
                self.pop_scope();
            }
            Stmt::ForOf(f) => {
                self.push_scope();
                self.walk_for_decl(for_head_var(&f.left), false);
                self.walk_for_head(&f.left);
                self.walk_expr_with(&f.right, ParentKind::Other);
                self.walk_stmt(&f.body);
                self.pop_scope();
            }
            Stmt::Decl(d) => self.walk_decl(d),
        }
    }

    fn walk_for_head(&mut self, head: &ForHead) {
        match head {
            ForHead::VarDecl(v) => self.walk_var_decl(v),
            ForHead::Pat(p) => self.walk_pat_as_target(p),
            ForHead::UsingDecl(_) => {}
        }
    }

    fn walk_var_decl(&mut self, v: &VarDecl) {
        for d in &v.decls {
            self.walk_pat_binding(&d.name);
            if let Some(init) = &d.init {
                self.walk_expr_with(init, ParentKind::Other);
            }
        }
    }

    fn walk_decl(&mut self, d: &Decl) {
        match d {
            Decl::Class(c) => {
                self.push_scope();
                self.walk_class(&c.class);
                self.pop_scope();
            }
            Decl::Fn(f) => {
                self.push_scope();
                self.walk_function(&f.function);
                self.pop_scope();
            }
            Decl::Var(v) => self.walk_var_decl(v),
            _ => {}
        }
    }

    pub fn walk_program(&mut self, p: &Program) {
        match p {
            Program::Module(m) => {
                for item in &m.body {
                    if let ModuleItem::Stmt(s) = item {
                        self.walk_stmt(s);
                    }
                }
            }
            Program::Script(s) => {
                if let Some(Stmt::Expr(e)) = s.body.first() {
                    if matches!(&*e.expr, Expr::Arrow(_) | Expr::Fn(_) | Expr::Class(_)) {
                        self.suppress_root_pop = true;
                    }
                }
                for stmt in &s.body {
                    self.walk_stmt(stmt);
                }
            }
        }
    }
}

fn as_var_decl(init: &VarDeclOrExpr) -> Option<&VarDecl> {
    match init {
        VarDeclOrExpr::VarDecl(v) => Some(v),
        _ => None,
    }
}

fn for_head_var(head: &ForHead) -> Option<&VarDecl> {
    match head {
        ForHead::VarDecl(v) => Some(v),
        _ => None,
    }
}

/// `extractIdentifiers`
pub fn extract_pat_idents(p: &Pat) -> Vec<String> {
    let mut out = Vec::new();
    collect_pat_idents(p, &mut out);
    out
}

fn collect_pat_idents(p: &Pat, out: &mut Vec<String>) {
    match p {
        Pat::Ident(b) => out.push(b.id.sym.to_string()),
        Pat::Array(a) => {
            for el in a.elems.iter().flatten() {
                collect_pat_idents(el, out);
            }
        }
        Pat::Object(o) => {
            for prop in &o.props {
                match prop {
                    ObjectPatProp::KeyValue(kv) => collect_pat_idents(&kv.value, out),
                    ObjectPatProp::Assign(a) => out.push(a.key.id.sym.to_string()),
                    ObjectPatProp::Rest(r) => collect_pat_idents(&r.arg, out),
                }
            }
        }
        Pat::Rest(r) => collect_pat_idents(&r.arg, out),
        Pat::Assign(a) => collect_pat_idents(&a.left, out),
        Pat::Expr(e) => {
            // MemberExpression target: push the root object
            let mut cur = &**e;
            while let Expr::Member(m) = cur {
                cur = &m.obj;
            }
            if let Expr::Ident(i) = cur {
                out.push(i.sym.to_string());
            }
        }
        Pat::Invalid(_) => {}
    }
}

use swc_core::common::{Span, Spanned};
