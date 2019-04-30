#[macro_use]
extern crate log;
use resast::prelude::*;
use ress::{Comment, CommentKind};
use std::io::{Error as IoError, Write};

mod rewrite;
pub mod write_str;

/// The writer that will take in
/// RESSA AST and write to the provided
/// `impl Write` provided
pub struct Writer<T: Write> {
    current_indent: usize,
    at_top_level: bool,
    in_for_init: bool,
    new_line: String,
    indent: String,
    quote: Option<char>,
    out: T,
}
/// For building a writer when not
/// using the default
pub struct Builder<T: Write> {
    new_line: String,
    quote: Option<char>,
    indent: String,
    p: ::std::marker::PhantomData<T>,
}

impl<T: Write> Builder<T> {
    pub fn new() -> Self {
        Self {
            new_line: "\n".to_string(),
            quote: None,
            indent: " ".repeat(4),
            p: ::std::marker::PhantomData,
        }
    }
    /// Set the string to separate new lines
    ///
    /// defaults to `"\n"`
    pub fn new_line(&mut self, new_line: &str) -> &mut Self {
        self.set_new_line(new_line);
        self
    }
    /// Set the string to separate new lines
    ///
    /// defaults to `"\n"`
    pub fn set_new_line(&mut self, new_line: &str) {
        self.new_line = new_line.to_string();
    }
    /// Sets the character to use to wrap strings in
    /// the default is to just use the quotes provided
    /// by the original text. Providing a quote character
    /// here will result in the Writer re-writing the string.
    pub fn quote(&mut self, quote: char) -> &mut Self {
        self.set_quote(quote);
        self
    }
    /// Sets the character to use to wrap strings in
    /// the default is to just use the quotes provided
    /// by the original text. Providing a quote character
    /// here will result in the Writer re-writing the string.
    pub fn set_quote(&mut self, quote: char) {
        self.quote = Some(quote);
    }
    /// Sets the string to use as indentation. By default this
    /// will be set to 4 spaces.
    pub fn indent(&mut self, indent: &str) -> &mut Self {
        self.set_indent(indent);
        self
    }
    /// Sets the string to use as indentation. By default this
    /// will be set to 4 spaces.
    pub fn set_indent(&mut self, indent: &str) {
        self.indent = indent.to_string();
    }
    /// Finalize the writer with the destination.
    pub fn build(&self, destination: T) -> Writer<T> {
        Writer::create(
            destination,
            self.new_line.clone(),
            self.quote.clone(),
            self.indent.clone(),
        )
    }
}

type Res = Result<(), IoError>;

impl<T: Write> Writer<T> {
    /// Create a default writer with the provided
    /// destination
    ///
    /// This will use `\n` for new lines, 4 spaces for indenting
    /// and the source text's quote character for quoting
    pub fn new(out: T) -> Self {
        trace!("new");
        Self::create(out, "\n".to_string(), None, " ".repeat(4))
    }
    /// Fully customizable constructor
    /// See `builder` for a more ergonomic solution
    pub fn create(out: T, new_line: String, quote: Option<char>, indent: String) -> Self {
        Self {
            current_indent: 0,
            at_top_level: true,
            in_for_init: false,
            out,
            new_line,
            quote,
            indent,
        }
    }
    /// Create a `Builder` for constructing your writer
    pub fn builder() -> Builder<T> {
        Builder {
            new_line: String::from("\n"),
            quote: None,
            indent: " ".repeat(4),
            p: ::std::marker::PhantomData,
        }
    }
    /// This will loop over the contents of a `Program` and
    /// attempt write them all to the provided `impl Write`
    pub fn write_program(&mut self, program: &Program) -> Res {
        let parts = match program {
            Program::Script(ref parts) => parts,
            Program::Mod(ref parts) => parts,
        };
        for ref part in parts {
            self.write_part(part)?;
        }
        Ok(())
    }
    /// This will attempt to write a single `ProgramPart`
    pub fn write_part(&mut self, part: &ProgramPart) -> Res {
        trace!("write_part: {:#?}", part);
        self.at_top_level = true;
        self._write_part(part)?;
        self.write_new_line()?;
        Ok(())
    }
    /// Internal program part writer to help with top level
    /// detection, new lines and whitespace writing
    fn _write_part(&mut self, part: &ProgramPart) -> Res {
        trace!("_write_part");
        self.write_leading_whitespace()?;
        match part {
            ProgramPart::Decl(decl) => self.write_decl(decl)?,
            ProgramPart::Dir(dir) => self.write_directive(dir)?,
            ProgramPart::Stmt(stmt) => self.write_stmt(stmt)?,
        }
        Ok(())
    }
    /// Attempt to write a `Declaration` to the `impl Write`
    pub fn write_decl(&mut self, decl: &Decl) -> Res {
        trace!("write_decl");
        match decl {
            Decl::Variable(ref kind, ref decls) => self.write_variable_decls(kind, decls)?,
            Decl::Class(ref class) => {
                self.at_top_level = false;
                self.write_class(class)?;
                self.write_new_line()?;
            }
            Decl::Function(ref func) => {
                self.at_top_level = false;
                self.write_function(func)?;
                self.write_new_line()?;
            }
            Decl::Export(ref exp) => self.write_export_decl(exp)?,
            Decl::Import(ref imp) => self.write_import_decl(imp)?,
        };
        Ok(())
    }
    /// Attempt to write a `Declaration::Variable`'s contents to the `impl Write`
    /// ```js
    /// let a, b, c, d, e = "thing";
    /// const f = "stuff";
    /// var g, h, i, j = "places";
    /// ```
    pub fn write_variable_decls(&mut self, kind: &VariableKind, decls: &[VariableDecl]) -> Res {
        trace!("write_variable_decls");
        self.write_variable_kind(kind)?;
        let mut after_first = false;
        for decl in decls {
            if after_first {
                self.write(", ")?;
            } else {
                after_first = true;
            }
            self.write_variable_decl(decl)?;
        }
        self.write_empty_stmt()?;
        self.write_new_line()
    }
    /// Attempt to write a `Class` to the `impl Write`, used for both
    /// writing the contents of `Declaration::Class` and `Expr::Class`
    ///
    /// ```js
    /// // class expression
    /// let x = class {
    ///     constructor() {
    ///     }
    ///     method1() {
    ///     }
    ///     method2() {
    ///     }
    /// }
    /// // class declaration
    /// class Y {
    ///     constructor() {
    ///     }
    ///     method1() {
    ///     }
    ///     method2() {
    ///     }
    /// }
    /// ```
    pub fn write_class(&mut self, class: &Class) -> Res {
        trace!("write_class");
        self.write("class ")?;
        if let Some(ref id) = class.id {
            self.write_ident(id)?;
            self.write(" ")?;
        }
        if let Some(ref ex) = class.super_class {
            self.write("extends ")?;
            self.write_expr(ex)?;
            self.write(" ")?;
        }
        self.write_open_brace()?;
        self.write_new_line()?;
        for ref part in &class.body {
            self.write_new_line()?;
            self.write_leading_whitespace()?;
            self.write_property(part)?;
            self.write_new_line()?;
        }
        self.write_close_brace()?;
        Ok(())
    }
    /// Attempt to write the contents of `Declaration::Export` to the `impl Write`
    /// ```js
    /// export function Thing() {
    /// }
    /// export * from 'module';
    /// export {Stuff} from 'other_module';
    /// ```
    pub fn write_export_decl(&mut self, exp: &ModExport) -> Res {
        trace!("write_export_decl");
        self.write("export ")?;
        match exp {
            ModExport::All(ref a) => self.write_all_export(a)?,
            ModExport::Default(ref d) => self.write_default_export(d)?,
            ModExport::Named(ref n) => self.write_named_export(n)?,
        }
        Ok(())
    }
    /// Attempt to write the contents `ModuleExport::All` to the `impl Write`
    /// ```js
    /// export * from 'module'
    /// ```
    pub fn write_all_export(&mut self, exp: &Literal) -> Res {
        trace!("write_all_export");
        self.write("* from ")?;
        self.write_literal(exp)?;
        Ok(())
    }
    /// Attempt to write the contents `ModuleExport::Default` to the `impl Write`
    /// ```js
    /// export default function Thing() {
    /// }
    /// ```
    pub fn write_default_export(&mut self, exp: &DefaultExportDecl) -> Res {
        trace!("write_default_export");
        self.write("default ")?;
        match exp {
            DefaultExportDecl::Decl(ref d) => self.write_decl(d)?,
            DefaultExportDecl::Expr(ref e) => self.write_expr(e)?,
        }
        Ok(())
    }
    /// Attempts to write the contents of `ModuleExport::Named` to the `impl Write`
    /// export function Thing {
    /// }
    /// export {Stuff} from 'module';
    pub fn write_named_export(&mut self, exp: &NamedExportDecl) -> Res {
        trace!("write_named_export");
        match exp {
            NamedExportDecl::Decl(ref d) => self.write_decl(d)?,
            NamedExportDecl::Specifier(ref s, ref from) => self.write_export_specifiers(s, from)?,
        }
        Ok(())
    }
    /// Attempts to write the contents of `NamedExportDecl::Specifier` to the `impl Write`
    /// ```js
    /// export {Stuff as Things} from 'module'
    /// export {Places} from 'other_module'
    /// ```
    pub fn write_export_specifiers(
        &mut self,
        specifiers: &[ExportSpecifier],
        from: &Option<Literal>,
    ) -> Res {
        trace!("write_export_specifiers");
        self.write("{")?;
        let mut after_first = false;
        for s in specifiers {
            if after_first {
                self.write(", ")?;
            }
            self.write_ident(&s.local)?;
            if let Some(ref name) = &s.exported {
                self.write(" as ")?;
                self.write(name)?;
            }
            after_first = true;
        }
        self.write("}")?;
        if let Some(ref from) = from {
            self.write(" from ")?;
            self.write_literal(from)?;
        }
        Ok(())
    }
    /// Attempts to write the contents of `Declaration::Import` to the `impl Write`
    /// ```js
    /// import * as Moment from 'moment';
    /// import {Thing, Place} from 'module';
    /// import Stuff from 'other_module';
    /// ```
    pub fn write_import_decl(&mut self, imp: &ModImport) -> Res {
        trace!("write_import_decl");
        self.write("import ")?;
        if imp.specifiers.len() == 0 {
            self.write("{}")?;
        }
        let mut opened_brace = false;
        let mut specifiers = imp.specifiers.iter();
        if let Some(ref first) = specifiers.next() {
            if let ImportSpecifier::Default(ref ident) = first {
                self.write_ident(ident)?;
            } else if let ImportSpecifier::Namespace(ref imp) = first {
                self.write_namespace_import(imp)?;
            } else {
                self.write("{ ")?;
                opened_brace = true;
                self.write_import_specificer(first)?;
            }
        }

        if !opened_brace {
            if let Some(ref next) = specifiers.next() {
                if let ImportSpecifier::Namespace(ref name) = next {
                    self.write(", ")?;
                    self.write_namespace_import(name)?;
                } else {
                    self.write(", { ")?;
                    self.write_import_specificer(next)?;
                    opened_brace = true;
                }
            }
        }

        while let Some(ref s) = specifiers.next() {
            self.write(", ")?;
            self.write_import_specificer(s)?;
        }
        if opened_brace {
            self.write(" }")?;
        }
        self.write(" from ")?;
        self.write_literal(&imp.source)?;
        self.write_empty_stmt()?;
        Ok(())
    }
    /// Attempts to write a single `ImportSpecifier` to the `impl Write`
    /// ```js
    /// import * as Moment from 'moment';
    /// import {Thing, Place} from 'module';
    /// import Stuff from 'other_module';
    /// ```
    pub fn write_import_specificer(&mut self, spec: &ImportSpecifier) -> Res {
        trace!("write_import_specificer");
        match spec {
            ImportSpecifier::Default(ref i) => self.write_ident(i)?,
            ImportSpecifier::Namespace(ref n) => self.write_namespace_import(n)?,
            ImportSpecifier::Normal(ref n, ref l) => self.write_normal_import(n, l)?,
        }
        Ok(())
    }
    /// Attempts to write the contents of`ImportSpecifier::Namespace` to the `impl Write`
    /// ```js
    /// import * as Moment from 'moment';
    /// ```
    pub fn write_namespace_import(&mut self, name: &Identifier) -> Res {
        trace!("write_namespace_import");
        self.write("* as ")?;
        self.write_ident(name)?;
        Ok(())
    }
    /// Attempts to write the contents of`ImportSpecifier::Normal` to the `impl Write`
    /// ```js
    /// import {Thing as Stuff} from 'module';
    /// ```
    pub fn write_normal_import(&mut self, name: &Identifier, local: &Option<Identifier>) -> Res {
        trace!("write_normal_import");
        self.write_ident(name)?;
        if let Some(ref ident) = local {
            self.write(" as ")?;
            self.write(ident)?;
        }
        Ok(())
    }
    /// Attempts to write a directive to the `impl Write`
    /// ```js
    /// 'use strict';
    /// ```
    pub fn write_directive(&mut self, dir: &Dir) -> Res {
        trace!("write_directive");
        self.write_literal(&dir.expr)?;
        self.write_empty_stmt()?;
        self.write_new_line()?;
        Ok(())
    }
    /// Attempts to write a variable declaration
    /// ```js
    /// let x = function() {
    /// }
    /// var a, b, c, d = 'things';
    /// ```
    pub fn write_variable_decl(&mut self, decl: &VariableDecl) -> Res {
        trace!("write_variable_decl");
        self.write_pattern(&decl.id)?;
        if let Some(ref init) = decl.init {
            self.write(" = ")?;
            self.write_expr(init)?;
        }
        Ok(())
    }
    /// Attempts to write the variable keyword (`var`/`let`/`const`)
    pub fn write_variable_kind(&mut self, kind: &VariableKind) -> Res {
        trace!("write_variable_kind");
        let s = match kind {
            VariableKind::Const => "const ",
            VariableKind::Let => "let ",
            VariableKind::Var => "var ",
        };
        self.write(s)
    }
    /// Attempts to write the contents of a `Stmt`
    pub fn write_stmt(&mut self, stmt: &Stmt) -> Res {
        trace!("write_stmt");
        let mut semi = true;
        let mut new_line = true;
        let cached_state = self.at_top_level;
        match stmt {
            Stmt::Empty => {
                new_line = false;
            }
            Stmt::Debugger => self.write_debugger_stmt()?,
            Stmt::Expr(ref stmt) => {
                let wrap = match stmt {
                    Expr::Literal(_) | Expr::Object(_) | Expr::Function(_) | Expr::Binary(_) => {
                        true
                    }
                    _ => false,
                };
                if wrap {
                    self.write_wrapped_expr(stmt)?
                } else {
                    self.write_expr(stmt)?
                }
            }
            Stmt::Block(ref stmt) => {
                self.at_top_level = false;
                self.write_block_stmt(stmt)?;
                semi = false;
                new_line = false;
                self.at_top_level = cached_state;
            }
            Stmt::With(ref stmt) => {
                self.write_with_stmt(stmt)?;
                semi = false;
            }
            Stmt::Return(ref stmt) => self.write_return_stmt(stmt)?,
            Stmt::Labeled(ref stmt) => {
                self.write_labeled_stmt(stmt)?;
                semi = false;
            }
            Stmt::Break(ref stmt) => self.write_break_stmt(stmt)?,
            Stmt::Continue(ref stmt) => self.write_continue_stmt(stmt)?,
            Stmt::If(ref stmt) => {
                self.write_if_stmt(stmt)?;
                semi = false;
            }
            Stmt::Switch(ref stmt) => {
                self.at_top_level = false;
                self.write_switch_stmt(stmt)?;
                semi = false;
            }
            Stmt::Throw(ref stmt) => self.write_throw_stmt(stmt)?,
            Stmt::Try(ref stmt) => {
                self.write_try_stmt(stmt)?;
                semi = false;
            }
            Stmt::While(ref stmt) => {
                new_line = self.write_while_stmt(stmt)?;
                semi = false;
            }
            Stmt::DoWhile(ref stmt) => self.write_do_while_stmt(stmt)?,
            Stmt::For(ref stmt) => {
                self.at_top_level = false;
                new_line = self.write_for_stmt(stmt)?;
                semi = false;
            }
            Stmt::ForIn(ref stmt) => {
                self.at_top_level = false;
                new_line = self.write_for_in_stmt(stmt)?;
                semi = false;
            }
            Stmt::ForOf(ref stmt) => {
                self.at_top_level = false;
                new_line = self.write_for_of_stmt(stmt)?;
                semi = false;
            }
            Stmt::Var(ref stmt) => self.write_var_stmt(stmt)?,
        };
        if semi {
            self.write_empty_stmt()?;
        }
        if new_line {
            self.write_new_line()?;
        }
        self.at_top_level = cached_state;
        Ok(())
    }
    /// Attempts to write a debugger stmt
    /// ```js
    /// debugger;
    /// ```
    pub fn write_debugger_stmt(&mut self) -> Res {
        trace!("write_debugger_stmt");
        self.write("debugger")
    }
    /// Attempts to write a block statement
    /// ```js
    /// {
    ///     var x = 0;
    /// }
    /// ```
    pub fn write_block_stmt(&mut self, block: &[ProgramPart]) -> Res {
        trace!("write_block_stmt");
        self.write_open_brace()?;
        if block.len() == 0 {
            self.write_new_line()?;
            self.write_leading_whitespace()?;
            self.write_new_line()?;
        }
        for ref part in block {
            self.write_new_line()?;
            self._write_part(part)?;
        }
        self.write_close_brace()?;
        Ok(())
    }
    /// Attempts to write a `WithStmt`
    /// ```js
    /// With(Math) {
    ///     var y = random() * 100;
    /// }
    /// ```
    pub fn write_with_stmt(&mut self, expr: &WithStmt) -> Res {
        trace!("write_with_stmt");
        self.write("with (")?;
        self.write_expr(&expr.object)?;
        self.write(") ")?;
        self.write_stmt(&expr.body)?;
        Ok(())
    }
    /// Attempts to write a `ReturnStmt`
    /// ```js
    /// function one() {
    ///     return 'things';
    /// }
    /// function two() {
    ///     return;
    /// }
    /// ```
    pub fn write_return_stmt(&mut self, expr: &Option<Expr>) -> Res {
        trace!("write_return_stmt");
        self.write("return")?;
        if let Some(ref e) = expr {
            self.write(" ")?;
            self.write_expr(e)?;
        }
        Ok(())
    }
    /// Attempts to write a `LabeledStmt`
    /// ```js
    /// label: {
    ///     if (true) {
    ///         break label;
    ///     }
    /// }
    /// ```
    pub fn write_labeled_stmt(&mut self, expr: &LabeledStmt) -> Res {
        trace!("write_labeled_stmt");
        self.write_ident(&expr.label)?;
        self.write(": ")?;
        self.write_stmt(&expr.body)?;
        Ok(())
    }
    /// Attempts to write a break statement
    /// ```js
    /// label: {
    ///     if (true) {
    ///         break label;
    ///     }
    /// }
    /// for (;;) {
    ///     break;
    /// }
    /// ```
    pub fn write_break_stmt(&mut self, expr: &Option<Identifier>) -> Res {
        trace!("write_break_stmt");
        self.write("break")?;
        if let Some(ref i) = expr {
            self.write(" ")?;
            self.write_ident(i)?;
        }
        Ok(())
    }
    /// Attempts to write a continue statement
    /// ```js
    /// for (;;) continue;
    /// outer: for (;;) {
    ///     inner: for (;;) {
    ///         if ((Math.random() * 100) > 50) {
    ///             continue outer;
    ///         } else {
    ///             continue inner;
    ///         }
    ///     }
    /// }
    /// ```
    pub fn write_continue_stmt(&mut self, expr: &Option<Identifier>) -> Res {
        trace!("write_continue_stmt");
        self.write("continue")?;
        if let Some(ref i) = expr {
            self.write(" ")?;
            self.write_ident(i)?;
        }
        Ok(())
    }
    /// Attempts to write an `IfStmt`
    /// ```js
    /// if ((Math.random() * 100) > 50) {
    ///
    /// } else if ((Math.random() * 100) < 25) {
    ///
    /// } else {
    ///
    /// }
    /// ```
    pub fn write_if_stmt(&mut self, expr: &IfStmt) -> Res {
        trace!("write_if_stmt");
        self.write("if (")?;
        self.write_expr(&expr.test)?;
        self.write(") ")?;
        if let Stmt::Empty = &*expr.consequent {
            self.write_block_stmt(&[])?;
        } else {
            self.write_stmt(&expr.consequent)?;
        }
        if let Some(ref alt) = &expr.alternate {
            self.write(" else ")?;
            if let Stmt::Empty = &**alt {
                self.write_block_stmt(&[])?;
            } else {
                self.write_stmt(alt)?;
            }
        }
        Ok(())
    }
    /// Attempts to write a `SwitchStmt`
    /// ```js
    /// switch (Math.floor(Math.random() * 5)) {
    ///     case 0:
    ///     default:
    /// }
    /// ```
    pub fn write_switch_stmt(&mut self, switch: &SwitchStmt) -> Res {
        trace!("write_switch_stmt");
        self.write("switch (")?;
        self.write_expr(&switch.discriminant)?;
        self.write(") ")?;
        if switch.cases.len() == 0 {
            self.write("{ }")?;
            return Ok(());
        }
        self.write_open_brace()?;
        if switch.cases.len() > 0 {
            self.write_new_line()?;
        }
        for ref case in &switch.cases {
            self.write_switch_case(case)?;
        }
        self.write_close_brace()?;
        Ok(())
    }
    /// Attempts to write a `SwitchCase`
    /// ```js
    /// switch (Math.floor(Math.random() * 5)) {
    ///     case 0:
    ///     break;
    ///     default:
    ///         return 100;
    /// }
    /// ```
    pub fn write_switch_case(&mut self, case: &SwitchCase) -> Res {
        trace!("write_switch_case");
        self.write_leading_whitespace()?;
        if let Some(ref t) = &case.test {
            self.write("case ")?;
            self.write_expr(t)?;
        } else {
            self.write("default")?;
        }
        self.write(":")?;
        self.write_new_line()?;
        self.current_indent += 1;
        let mut decrease_indent = true;
        for ref part in &case.consequent {
            if let ProgramPart::Stmt(Stmt::Break(_)) = part {
                self.current_indent = self.current_indent.saturating_sub(1);
                decrease_indent = false;
            }
            self._write_part(part)?;
            self.write_new_line()?;
        }
        if decrease_indent {
            self.current_indent = self.current_indent.saturating_sub(1);
        }
        Ok(())
    }
    /// Attempts to write a throw statement
    /// ```js
    /// function one() {
    ///     throw 'Things'
    /// }
    /// function two() {
    ///     throw new Error('Things');
    /// }
    /// ```
    pub fn write_throw_stmt(&mut self, expr: &Expr) -> Res {
        trace!("write_throw_stmt");
        self.write("throw ")?;
        self.write_expr(&expr)?;
        Ok(())
    }
    /// Attempts to write a try statement
    /// ```js
    /// try {
    ///
    /// } catch (e) {
    ///
    /// } finally {
    ///
    /// }
    /// ```
    pub fn write_try_stmt(&mut self, stmt: &TryStmt) -> Res {
        trace!("write_try_stmt");
        self.write("try ")?;
        self.write_block_stmt(&stmt.block)?;
        if let Some(ref c) = &stmt.handler {
            self.write(" catch")?;
            if let Some(ref param) = &c.param {
                self.write(" (")?;
                self.write_pattern(param)?;
                self.write(") ")?;
            }
            self.write_block_stmt(&c.body)?;
        }
        if let Some(ref f) = &stmt.finalizer {
            self.write(" finally ")?;
            self.write_block_stmt(&f)?;
        }
        Ok(())
    }
    /// Attempts to write a while statement
    /// ```js
    /// while (true) {
    /// }
    /// ```
    pub fn write_while_stmt(&mut self, stmt: &WhileStmt) -> Result<bool, IoError> {
        trace!("write_while_stmt");
        let mut ret = false;
        self.write("while (")?;
        self.write_expr(&stmt.test)?;
        self.write(") ")?;
        if let Stmt::Block(_) = &*stmt.body {
            ret = true;
        }
        self.write_stmt(&stmt.body)?;
        Ok(ret)
    }
    /// Attempts to write a do while statement
    /// ```js
    /// do {
    ///
    /// } while(true)
    pub fn write_do_while_stmt(&mut self, stmt: &DoWhileStmt) -> Res {
        trace!("write_do_while_stmt");
        self.write("do")?;
        if let Stmt::Empty = &*stmt.body {
            self.write("; ")?;
        } else {
            self.write(" ")?;
            self.write_stmt(&stmt.body)?;
            self.write(" ")?;
        }
        self.write("while (")?;
        self.write_expr(&stmt.test)?;
        self.write(")")?;
        Ok(())
    }
    /// Attempts to write a c-style for loop
    /// for (var i = 0; i < 100; i++) console.log(i);
    /// for (;;) {
    ///     break;
    /// }
    pub fn write_for_stmt(&mut self, stmt: &ForStmt) -> Result<bool, IoError> {
        trace!("write_for_stmt");
        self.write("for (")?;
        if let Some(ref init) = &stmt.init {
            self.write_loop_init(init)?;
        }
        self.write_empty_stmt()?;
        if let Some(ref test) = &stmt.test {
            self.write_expr(test)?;
        }
        self.write_empty_stmt()?;
        if let Some(ref update) = &stmt.update {
            self.write_expr(update)?;
        }
        self.write(") ")?;
        let ret = if let Stmt::Block(_) = &*stmt.body {
            true
        } else {
            false
        };
        self.write_stmt(&stmt.body)?;
        Ok(ret)
    }
    /// Attempts to write the first part of a c-style for loop's parenthetical
    pub fn write_loop_init(&mut self, init: &LoopInit) -> Res {
        self.in_for_init = true;
        match init {
            LoopInit::Expr(ref e) => self.write_expr(e)?,
            LoopInit::Variable(ref kind, ref v) => {
                self.write_variable_kind(kind)?;
                let mut after_first = false;
                for ref d in v {
                    if after_first {
                        self.write(", ")?;
                    }
                    self.write_variable_decl(d)?;
                    after_first = true;
                }
            }
        }
        self.in_for_init = false;
        Ok(())
    }
    /// Attempts to write a for in loop
    /// ```js
    /// for (var x in []) {
    ///
    /// }
    /// ```
    pub fn write_for_in_stmt(&mut self, stmt: &ForInStmt) -> Result<bool, IoError> {
        trace!("write_for_in_stmt");
        self.write("for (")?;
        self.write_loop_left(&stmt.left)?;
        self.write(" in ")?;
        self.write_expr(&stmt.right)?;
        self.write(") ")?;
        self.write_stmt(&stmt.body)?;
        let ret = if let Stmt::Block(_) = &*stmt.body {
            true
        } else {
            false
        };
        Ok(ret)
    }
    /// Attempts to write a for of loop
    /// ```js
    /// for (let x of []) {
    ///
    /// }
    /// ```
    pub fn write_for_of_stmt(&mut self, stmt: &ForOfStmt) -> Result<bool, IoError> {
        trace!("write_for_of_stmt");
        self.write("for (")?;
        self.write_loop_left(&stmt.left)?;
        self.write(" of ")?;
        self.write_expr(&stmt.right)?;
        self.write(") ")?;
        self.write_stmt(&stmt.body)?;
        let ret = if let Stmt::Block(_) = &*stmt.body {
            true
        } else {
            false
        };
        Ok(ret)
    }
    /// Attempts to write for first part of a for of or for in loop's parenthetical
    pub fn write_loop_left(&mut self, left: &LoopLeft) -> Res {
        match left {
            LoopLeft::Pat(ref pat) => self.write_pattern(pat)?,
            LoopLeft::Variable(ref kind, ref var) => {
                self.write_variable_kind(kind)?;
                self.write_variable_decl(var)?;
            }
            LoopLeft::Expr(ref expr) => self.write_expr(expr)?,
        }
        Ok(())
    }
    /// write a variable statment
    /// ```js
    /// var x;
    /// var y = x;
    /// var q, w, e, r = Infinity;
    /// ```
    pub fn write_var_stmt(&mut self, expr: &[VariableDecl]) -> Res {
        trace!("write_var_stmt");
        self.write("var ")?;
        let mut after_first = false;
        for ref d in expr {
            if after_first {
                self.write(", ")?;
            }
            self.write_variable_decl(d)?;
            after_first = true;
        }
        Ok(())
    }
    /// Write the contents of a pattern
    pub fn write_pattern(&mut self, pattern: &Pat) -> Res {
        trace!("write_pattern");
        match pattern {
            Pat::Identifier(ref i) => self.write(i),
            Pat::Object(ref o) => self.write_object_pattern(o),
            Pat::Array(ref a) => self.write_array_pattern(a.as_slice()),
            Pat::RestElement(ref r) => self.write_rest_element(r),
            Pat::Assignment(ref a) => self.write_assignment_pattern(a),
        }
    }
    /// Write an object pattern
    /// ```js
    /// let {x, y} = {x: 100, y: 0};
    /// ```
    pub fn write_object_pattern(&mut self, obj: &ObjectPat) -> Res {
        trace!("write_object_pattern");
        if obj.len() == 0 {
            self.write("{}")?;
            return Ok(());
        }
        self.write_open_brace()?;
        let mut after_first = false;
        for ref part in obj {
            if after_first {
                self.write(", ")?;
            } else {
                after_first = true;
            }
            match part {
                ObjectPatPart::Assignment(ref prop) => self.write_property(prop)?,
                ObjectPatPart::Rest(ref pat) => self.write_rest_pattern_part(pat)?,
            }
        }

        self.write_close_brace()?;
        Ok(())
    }
    /// Write an object or class property
    pub fn write_property(&mut self, prop: &Property) -> Res {
        trace!("write_property");
        match &prop.kind {
            PropertyKind::Init => self.write_init_property(prop),
            PropertyKind::Get => self.write_get_property(prop),
            PropertyKind::Set => self.write_set_property(prop),
            PropertyKind::Method => self.write_property_method(prop),
            PropertyKind::Ctor => self.write_ctor_property(prop),
        }
    }
    /// Write a property that is not a method or constructor
    /// ```js
    /// {
    ///     a: 100,
    /// }
    /// ```
    pub fn write_init_property(&mut self, prop: &Property) -> Res {
        trace!("write_init_property");
        match &prop.value {
            PropertyValue::Expr(Expr::Function(_)) => return self.write_property_method(prop),
            _ => (),
        }
        self.write_property_key(&prop.key, prop.computed)?;
        if !prop.short_hand {
            self.write(": ")?;
            self.write_property_value(&prop.value)?;
        } else {
            match &prop.value {
                PropertyValue::None => (),
                PropertyValue::Expr(_) | PropertyValue::Pat(_) => {
                    self.write(" = ")?;
                    self.write_property_value(&prop.value)?;
                }
            }
        }
        Ok(())
    }
    /// Write a get property
    /// ```js
    /// {
    ///     get thing() {
    ///         return 'thing'
    ///     }
    /// }
    /// ```
    pub fn write_get_property(&mut self, prop: &Property) -> Res {
        trace!("write_get_property");
        self.write("get ")?;
        self.write_property_method(prop)
    }
    /// Write a get property
    /// ```js
    /// class Stuff {
    ///     set thing(value) {
    ///         this.thing = value;
    ///     }
    /// }
    /// ```
    pub fn write_set_property(&mut self, prop: &Property) -> Res {
        trace!("write_set_property");
        self.write("set ")?;
        self.write_property_method(prop)
    }
    /// Write a property that is a method
    /// ```js
    /// {
    ///     thing() {
    ///         return 'thing'
    ///     }
    /// }
    /// ```
    pub fn write_property_method(&mut self, prop: &Property) -> Res {
        trace!("write_property_method");
        if let PropertyValue::Expr(Expr::Function(ref func)) = prop.value {
            if func.is_async {
                self.write("async ")?;
            }
            if func.generator {
                self.write("*")?;
            }
            self.write_property_key(&prop.key, prop.computed)?;
            self.write_function_args(&func.params)?;
            self.write_function_body(&func.body)?;
        } else {
            panic!("property method value must be a function expression");
        }
        Ok(())
    }
    /// Write the arguments of a function or method definition
    /// ```js
    /// function(arg1, arg2) {
    /// }
    /// ```
    pub fn write_function_args(&mut self, args: &[FunctionArg]) -> Res {
        trace!("write_function_args");
        self.write("(")?;
        let mut after_first = false;
        for ref arg in args {
            if after_first {
                self.write(", ")?;
            } else {
                after_first = true;
            }
            self.write_function_arg(arg)?;
        }
        self.write(")")?;
        Ok(())
    }
    /// Write a single function arg
    pub fn write_function_arg(&mut self, arg: &FunctionArg) -> Res {
        trace!("write_function_arg");
        match arg {
            FunctionArg::Expr(ref ex) => self.write_expr(ex)?,
            FunctionArg::Pat(ref pa) => self.write_pattern(pa)?,
        }
        Ok(())
    }
    /// Write the block statement that makes up a function's body
    pub fn write_function_body(&mut self, body: &FunctionBody) -> Res {
        trace!("write_function_body");
        if body.len() == 0 {
            self.write("{ ")?;
        } else {
            self.write_open_brace()?;
            self.write_new_line()?;
        }
        for ref part in body {
            self._write_part(part)?;
        }
        if body.len() == 0 {
            self.write("}")?;
        } else {
            self.write_close_brace()?;
        }
        Ok(())
    }
    /// Write a property that is a constructor for a class
    /// ```js
    /// class Thing {
    ///     constructor() {
    ///     }
    /// }
    /// ```
    pub fn write_ctor_property(&mut self, prop: &Property) -> Res {
        trace!("write_ctor_property");
        self.write("constructor")?;
        if let PropertyValue::Expr(Expr::Function(ref func)) = prop.value {
            self.write_function_args(&func.params)?;
            self.write_function_body(&func.body)?;
        } else {
            panic!("constructor's value must be a function expression");
        }
        Ok(())
    }
    /// Write a property key, taking into account of it should be wrapped in [] for "computed"
    /// properties
    pub fn write_property_key(&mut self, key: &PropertyKey, computed: bool) -> Res {
        trace!("write_property_key");
        if computed {
            self.write("[")?;
        }
        match key {
            PropertyKey::Expr(ref e) => self.write_expr(e)?,
            PropertyKey::Literal(ref l) => self.write_literal(l)?,
            PropertyKey::Pat(ref p) => self.write_pattern(p)?,
        }
        if computed {
            self.write("]")?;
        }
        Ok(())
    }
    /// Write the value for a property
    pub fn write_property_value(&mut self, value: &PropertyValue) -> Res {
        trace!("write_property_value");
        match value {
            PropertyValue::Expr(ref e) => self.write_expr(e)?,
            PropertyValue::Pat(ref p) => self.write_pattern(p)?,
            PropertyValue::None => (),
        }
        Ok(())
    }
    /// Writes a rest pattern
    /// ```js
    /// let x = [...y];
    /// ```
    pub fn write_rest_pattern_part(&mut self, pat: &Pat) -> Res {
        trace!("write_rest_pattern_part");
        self.write_pattern(pat)?;
        Ok(())
    }
    /// Writes an array literal from a pattern
    /// ```js
    /// let [x, y] = [1, 2];
    /// ```
    pub fn write_array_pattern(&mut self, arr: &[Option<ArrayPatPart>]) -> Res {
        trace!("write_array_pattern");
        if arr.len() == 0 {
            self.write("[]")?;
            return Ok(());
        }
        self.write("[")?;
        let last_idx = arr.len() - 1;
        for (i, ref p) in arr.iter().enumerate() {
            if let Some(ref part) = p {
                match &part {
                    ArrayPatPart::Expr(e) => self.write_expr(e)?,
                    ArrayPatPart::Pat(p) => self.write_pattern(p)?,
                }
            }
            if i < last_idx {
                self.write(", ")?;
            }
        }
        self.write("]")?;
        Ok(())
    }

    pub fn write_rest_element(&mut self, pat: &Pat) -> Res {
        trace!("write_rest_element");
        self.write("...")?;
        self.write_pattern(pat)?;
        Ok(())
    }
    /// Writes a rest pattern
    /// ```js
    /// let x = [...y];
    /// ```
    pub fn write_assignment_pattern(&mut self, assignment: &AssignmentPat) -> Res {
        trace!("write_assignment_pattern");
        self.write_pattern(&assignment.left)?;
        self.write(" = ")?;
        self.write_expr(&assignment.right)?;
        Ok(())
    }

    pub fn write_wrapped_expr(&mut self, expr: &Expr) -> Res {
        self.write("(")?;
        self.write_expr(expr)?;
        self.write(")")?;
        Ok(())
    }

    pub fn write_expr(&mut self, expr: &Expr) -> Res {
        trace!("write_expr");
        let cached_state = self.at_top_level;
        match expr {
            Expr::Literal(ref expr) => self.write_literal(expr)?,
            Expr::This => self.write_this_expr()?,
            Expr::Super => self.write_super_expr()?,
            Expr::Array(ref expr) => self.write_array_expr(expr)?,
            Expr::Object(ref expr) => self.write_object_expr(expr)?,
            Expr::Function(ref expr) => {
                self.at_top_level = false;
                self.write_function(expr)?;
                self.at_top_level = cached_state;
            }
            Expr::Unary(ref expr) => self.write_unary_expr(expr)?,
            Expr::Update(ref expr) => self.write_update_expr(expr)?,
            Expr::Binary(ref expr) => self.write_binary_expr(expr)?,
            Expr::Assignment(ref expr) => {
                self.at_top_level = false;
                self.write_assignment_expr(expr)?
            }
            Expr::Logical(ref expr) => self.write_logical_expr(expr)?,
            Expr::Member(ref expr) => self.write_member_expr(expr)?,
            Expr::Conditional(ref expr) => self.write_conditional_expr(expr)?,
            Expr::Call(ref expr) => self.write_call_expr(expr)?,
            Expr::New(ref expr) => self.write_new_expr(expr)?,
            Expr::Sequence(ref expr) => self.write_sequence_expr(expr)?,
            Expr::Spread(ref expr) => self.write_spread_expr(expr)?,
            Expr::ArrowFunction(ref expr) => {
                self.at_top_level = false;
                self.write_arrow_function_expr(expr)?;
                self.at_top_level = cached_state;
            }
            Expr::Yield(ref expr) => self.write_yield_expr(expr)?,
            Expr::Class(ref expr) => {
                self.at_top_level = false;
                self.write_class(expr)?;
                self.at_top_level = cached_state;
            }
            Expr::MetaProperty(ref expr) => self.write_meta_property(expr)?,
            Expr::Await(ref expr) => self.write_await_expr(expr)?,
            Expr::Ident(ref expr) => self.write_ident(expr)?,
            Expr::TaggedTemplate(ref expr) => self.write_tagged_template(expr)?,
            _ => unreachable!(),
        }
        Ok(())
    }
    /// Write `this`
    pub fn write_this_expr(&mut self) -> Res {
        trace!("write_this_expr");
        self.write("this")?;
        Ok(())
    }
    /// Write `super`
    pub fn write_super_expr(&mut self) -> Res {
        trace!("write_super_expr");
        self.write("super")?;
        Ok(())
    }
    /// write an array literal
    /// ```js
    /// [one,,two,,3, null];
    /// ```
    pub fn write_array_expr(&mut self, arr: &ArrayExpr) -> Res {
        trace!("write_array_expr");
        if arr.len() == 0 {
            self.write("[]")?;
            return Ok(());
        }
        self.write("[")?;
        let last_idx = arr.len() - 1;
        for (i, ref e) in arr.iter().enumerate() {
            if let Some(ref e) = e {
                self.write_expr(e)?;
                if i < last_idx {
                    self.write(", ")?;
                }
            } else {
                self.write(",")?;
            }
        }
        self.write("]")?;
        Ok(())
    }
    /// Write an object literal
    /// ```js
    /// {
    ///     a: b,
    ///     c: d,
    /// }
    /// ```
    pub fn write_object_expr(&mut self, obj: &ObjectExpr) -> Res {
        trace!("write_object_expr");
        if obj.len() == 0 {
            self.write("{}")?;
            return Ok(());
        }
        self.write("{")?;
        let mut after_first = false;
        for ref prop in obj {
            if after_first {
                self.write(", ")?;
            } else {
                after_first = true;
            }
            match prop {
                ObjectProperty::Property(ref p) => self.write_property(p),
                ObjectProperty::Spread(ref e) => self.write_expr(e),
            }?;
        }
        self.write("}")?;
        Ok(())
    }
    /// Write a function. This is used to write the contents of both a `Declaration::Function`
    /// and an `Expr::Function`
    pub fn write_function(&mut self, func: &Function) -> Res {
        trace!("write_function");
        if func.is_async {
            self.write("async ")?;
        }
        self.write("function")?;
        if let Some(ref id) = func.id {
            self.write(" ")?;
            if func.generator {
                self.write("*")?;
            }
            self.write(id)?;
        } else if func.generator {
            self.write("*")?;
        }
        self.write_function_args(&func.params)?;
        self.write(" ")?;
        self.write_function_body(&func.body)
    }
    /// Write a unary expression
    /// ```js
    /// delete x
    /// typeof y
    /// +9
    /// -10
    /// void 0
    /// ~3
    /// !true
    /// ```
    pub fn write_unary_expr(&mut self, unary: &UnaryExpr) -> Res {
        trace!("write_unary_expr");
        if unary.prefix {
            self.write_unary_operator(&unary.operator)?;
        }
        match &*unary.argument {
            Expr::Assignment(_)
            | Expr::Binary(_)
            | Expr::Logical(_)
            | Expr::Conditional(_)
            | Expr::ArrowFunction(_)
            | Expr::Function(_) => self.write_wrapped_expr(&unary.argument)?,
            Expr::Unary(_) | Expr::Update(_) => {
                self.write(" ")?;
                self.write_expr(&unary.argument)?;
            }
            _ => self.write_expr(&unary.argument)?,
        }
        if !unary.prefix {
            self.write_unary_operator(&unary.operator)?;
        }
        Ok(())
    }

    pub fn write_unary_operator(&mut self, op: &UnaryOperator) -> Res {
        trace!("write_unary_operator");
        match op {
            UnaryOperator::Delete => self.write("delete "),
            UnaryOperator::Minus => self.write("-"),
            UnaryOperator::Not => self.write("!"),
            UnaryOperator::Plus => self.write("+"),
            UnaryOperator::Tilde => self.write("~"),
            UnaryOperator::TypeOf => self.write("typeof "),
            UnaryOperator::Void => self.write("void "),
        }?;
        Ok(())
    }
    /// Write an update expression
    /// ```js
    /// a++
    /// --b
    /// ```
    pub fn write_update_expr(&mut self, update: &UpdateExpr) -> Res {
        trace!("write_update_expr");
        if update.prefix {
            self.write_update_operator(&update.operator)?;
        }
        self.write_expr(&update.argument)?;
        if !update.prefix {
            self.write_update_operator(&update.operator)?;
        }
        Ok(())
    }

    pub fn write_update_operator(&mut self, op: &UpdateOperator) -> Res {
        let s = match op {
            UpdateOperator::Decrement => "--",
            UpdateOperator::Increment => "++",
        };
        self.write(s)?;
        Ok(())
    }
    /// Writes a binary expression
    /// ```js
    /// a == b
    /// c !== d
    /// x instanceof y
    /// x * 100
    /// ```
    pub fn write_binary_expr(&mut self, binary: &BinaryExpr) -> Res {
        trace!("write_binary_expr {:#?}", binary);
        let wrap = self.in_for_init && binary.operator == BinaryOperator::In;
        if wrap {
            self.write("(")?;
        }
        self.write_binary_side(&*binary.left)?;
        self.write(" ")?;
        self.write_binary_operator(&binary.operator)?;
        self.write(" ")?;
        self.write_binary_side(&*binary.right)?;
        if wrap {
            self.write(")")?;
        }
        Ok(())
    }

    pub fn write_binary_side(&mut self, side: &Expr) -> Res {
        match &*side {
            Expr::Assignment(_)
            | Expr::Conditional(_)
            | Expr::Logical(_)
            | Expr::Function(_)
            | Expr::ArrowFunction(_)
            | Expr::Binary(_) => self.write_wrapped_expr(side),
            _ => self.write_expr(side),
        }
    }

    pub fn write_binary_operator(&mut self, op: &BinaryOperator) -> Res {
        let s = match op {
            BinaryOperator::And => "&",
            BinaryOperator::Equal => "==",
            BinaryOperator::GreaterThan => ">",
            BinaryOperator::GreaterThanEqual => ">=",
            BinaryOperator::In => "in",
            BinaryOperator::InstanceOf => "instanceof",
            BinaryOperator::LeftShift => "<<",
            BinaryOperator::LessThan => "<",
            BinaryOperator::LessThanEqual => "<=",
            BinaryOperator::Minus => "-",
            BinaryOperator::Mod => "%",
            BinaryOperator::NotEqual => "!=",
            BinaryOperator::Or => "|",
            BinaryOperator::Over => "/",
            BinaryOperator::Plus => "+",
            BinaryOperator::PowerOf => "**",
            BinaryOperator::RightShift => ">>",
            BinaryOperator::StrictEqual => "===",
            BinaryOperator::StrictNotEqual => "!==",
            BinaryOperator::Times => "*",
            BinaryOperator::UnsignedRightShift => ">>>",
            BinaryOperator::XOr => "^",
        };
        self.write(s)?;
        Ok(())
    }
    /// Write an assignment expression
    /// ```js
    /// a = b
    /// b += 8
    /// q **= 100
    /// ```
    pub fn write_assignment_expr(&mut self, assignment: &AssignmentExpr) -> Res {
        trace!("write_assignment_expr");
        let wrap_self = match &assignment.left {
            AssignmentLeft::Expr(ref e) => match &**e {
                Expr::Object(_) | Expr::Array(_) => true,
                _ => false,
            },
            AssignmentLeft::Pat(ref p) => match p {
                Pat::Array(_) => true,
                Pat::Object(_) => true,
                _ => false,
            },
        };
        if wrap_self {
            self.write("(")?;
        }
        match &assignment.left {
            AssignmentLeft::Expr(ref e) => self.write_expr(e)?,
            AssignmentLeft::Pat(ref p) => self.write_pattern(p)?,
        }
        self.write(" ")?;
        self.write_assignment_operator(&assignment.operator)?;
        self.write(" ")?;
        self.write_expr(&assignment.right)?;
        if wrap_self {
            self.write(")")?;
        }
        Ok(())
    }

    pub fn write_assignment_operator(&mut self, op: &AssignmentOperator) -> Res {
        let s = match op {
            AssignmentOperator::AndEqual => "&=",
            AssignmentOperator::DivEqual => "/=",
            AssignmentOperator::Equal => "=",
            AssignmentOperator::LeftShiftEqual => "<<=",
            AssignmentOperator::MinusEqual => "-=",
            AssignmentOperator::ModEqual => "%=",
            AssignmentOperator::OrEqual => "|=",
            AssignmentOperator::PlusEqual => "+=",
            AssignmentOperator::PowerOfEqual => "**=",
            AssignmentOperator::RightShiftEqual => ">>=",
            AssignmentOperator::TimesEqual => "*=",
            AssignmentOperator::UnsignedRightShiftEqual => ">>>=",
            AssignmentOperator::XOrEqual => "^=",
        };
        self.write(s)?;
        Ok(())
    }
    /// Writes a logical expression
    /// ```js
    /// a && b
    /// y || q
    /// ```
    pub fn write_logical_expr(&mut self, logical: &LogicalExpr) -> Res {
        trace!("write_logical_expr {:#?}", logical);
        let wrap_left = match &*logical.left {
            Expr::Logical(ref l) => l.operator == LogicalOperator::Or,
            Expr::Assignment(_) | Expr::Conditional(_) => true,
            _ => false,
        };
        if wrap_left {
            self.write_wrapped_expr(&logical.left)?;
        } else {
            self.write_expr(&logical.left)?;
        }
        self.write(" ")?;
        self.write_logical_operator(&logical.operator)?;
        let wrap_right = match &*logical.right {
            Expr::Logical(ref _l) => true,
            Expr::Assignment(_) | Expr::Conditional(_) => true,
            _ => false,
        };
        self.write(" ")?;
        if wrap_right {
            self.write_wrapped_expr(&logical.right)?;
        } else {
            self.write_expr(&logical.right)?;
        }
        Ok(())
    }

    pub fn write_logical_operator(&mut self, op: &LogicalOperator) -> Res {
        trace!("write_logical_operator");
        let s = match op {
            LogicalOperator::And => "&&",
            LogicalOperator::Or => "||",
        };
        self.write(s)?;
        Ok(())
    }
    /// Writes a member expression
    /// ```js
    /// console.log
    /// console['log']
    /// ```
    pub fn write_member_expr(&mut self, member: &MemberExpr) -> Res {
        trace!("write_member_expr");
        match &*member.object {
            Expr::Assignment(_)
            | Expr::Literal(Literal::Number(_))
            | Expr::Conditional(_)
            | Expr::Logical(_)
            | Expr::Function(_)
            | Expr::ArrowFunction(_)
            | Expr::Object(_)
            | Expr::Binary(_)
            | Expr::Unary(_)
            | Expr::Update(_) => self.write_wrapped_expr(&member.object)?,
            _ => self.write_expr(&member.object)?,
        }
        if member.computed {
            self.write("[")?;
        } else {
            self.write(".")?;
        }
        self.write_expr(&member.property)?;
        if member.computed {
            self.write("]")?;
        }
        Ok(())
    }
    /// Writes a conditional expression
    /// ```js
    /// let x = isTrue ? 'yes' : 'no';
    /// ```
    pub fn write_conditional_expr(&mut self, conditional: &ConditionalExpr) -> Res {
        trace!("write_conditional_expr");
        self.write_expr(&conditional.test)?;
        self.write(" ? ")?;
        if let Expr::Logical(_) = &*conditional.consequent {
            self.write_wrapped_expr(&conditional.consequent)?;
        } else {
            self.write_expr(&conditional.consequent)?;
        }
        self.write(" : ")?;
        self.write_expr(&conditional.alternate)?;
        Ok(())
    }
    /// Writes a call expression
    /// ```js
    /// console.log()
    /// (function() {
    /// })()
    /// ```
    pub fn write_call_expr(&mut self, call: &CallExpr) -> Res {
        trace!("write_call_expr");
        match &*call.callee {
            Expr::Function(_) | Expr::ArrowFunction(_) => self.write_wrapped_expr(&call.callee)?,
            _ => self.write_expr(&call.callee)?,
        }
        self.write_sequence_expr(&call.arguments)?;
        Ok(())
    }
    /// Writes a new expression
    /// ```js
    /// new Uint8Array(100);
    /// ```
    pub fn write_new_expr(&mut self, new: &NewExpr) -> Res {
        trace!("write_new_expr");
        self.write("new ")?;
        match &*new.callee {
            Expr::Assignment(_) | Expr::Call(_) => self.write_wrapped_expr(&new.callee)?,
            _ => self.write_expr(&new.callee)?,
        }
        self.write_sequence_expr(&new.arguments)?;
        Ok(())
    }
    /// Writes a sequence of sub-expressions
    /// ```js
    /// a = b, c = d, q * 100
    /// ```
    pub fn write_sequence_expr(&mut self, sequence: &[Expr]) -> Res {
        trace!("write_sequence_expr");
        let mut after_first = false;
        self.write("(")?;
        for ref e in sequence {
            if after_first {
                self.write(", ")?;
            }
            self.write_expr(e)?;
            after_first = true;
        }
        self.write(")")?;
        Ok(())
    }
    /// Writes a spread expression
    /// ```js
    /// function(...args) {
    /// }
    /// ```
    pub fn write_spread_expr(&mut self, spread: &Expr) -> Res {
        trace!("write_spread_expr");
        self.write("...")?;
        self.write_expr(spread)?;
        Ok(())
    }
    /// Writes and arrow function
    /// ```js
    /// x => console.log(x);
    /// (x, y) => {
    ///     return x * y;
    /// }
    /// ```
    pub fn write_arrow_function_expr(&mut self, func: &ArrowFunctionExpr) -> Res {
        trace!("write_arrow_function_expr");
        if func.is_async {
            self.write("async ")?;
        }
        if func.params.len() == 1 {
            match &func.params[0] {
                FunctionArg::Expr(ref arg) => match arg {
                    Expr::Ident(_) => self.write_function_arg(&func.params[0])?,
                    _ => self.write_function_args(&func.params)?,
                },
                FunctionArg::Pat(ref arg) => match arg {
                    Pat::Identifier(_) => self.write_function_arg(&func.params[0])?,
                    _ => self.write_function_args(&func.params)?,
                },
            }
        } else {
            self.write_function_args(&func.params)?;
        }
        self.write(" => ")?;
        match &func.body {
            ArrowFunctionBody::FunctionBody(ref b) => self.write_function_body(b)?,
            ArrowFunctionBody::Expr(ref e) => match &**e {
                Expr::Object(_) | Expr::Binary(_) => self.write_wrapped_expr(e)?,
                _ => self.write_expr(e)?,
            },
        }
        Ok(())
    }
    /// Writes a yield expression
    /// ```js
    /// function *gen() {
    ///     while (true) {
    ///         yield 100;
    ///     }
    /// }
    /// ```
    pub fn write_yield_expr(&mut self, expr: &YieldExpr) -> Res {
        trace!("write_yield_expr");
        self.write("yield")?;
        if expr.argument.is_some() {
            self.write(" ")?;
        }
        if expr.delegate {
            self.write("*")?;
        }
        if let Some(ref arg) = &expr.argument {
            self.write_expr(arg)?;
        }
        Ok(())
    }
    /// Writes a meta property
    /// ```js
    /// function Thing() {
    ///     if (new.target) {
    ///         this.stuff = 'things'
    ///     } else {
    ///         return new Thing;
    ///     }
    /// }
    /// ```
    pub fn write_meta_property(&mut self, meta: &MetaProperty) -> Res {
        trace!("write_meta_property");
        self.write_ident(&meta.meta)?;
        self.write(".")?;
        self.write_ident(&meta.property)?;
        Ok(())
    }
    /// Write an expression preceded by the await keyword
    pub fn write_await_expr(&mut self, expr: &Expr) -> Res {
        trace!("write_await_expr");
        self.write("await ")?;
        self.write_expr(expr)?;
        Ok(())
    }
    /// Write a plain identifier
    pub fn write_ident(&mut self, ident: &str) -> Res {
        trace!("write_ident");
        self.write(ident)
    }
    /// Write a template preceded by an identifier
    /// ```js
    /// tag`things ${0} stuff`;
    /// ```
    pub fn write_tagged_template(&mut self, template: &TaggedTemplateExpr) -> Res {
        trace!("write_tagged_template");
        self.write_expr(&template.tag)?;
        self.write_template(&template.quasi)?;
        Ok(())
    }
    /// Write a literal
    /// ```js
    /// null
    /// 'string'
    /// "string"
    /// 0.1e100
    /// 0xff
    /// 0o77
    /// 0b11
    /// false,
    /// true,
    /// /.+/g
    /// `things`
    pub fn write_literal(&mut self, lit: &Literal) -> Res {
        trace!("write_literal");
        match lit {
            Literal::Boolean(b) => self.write_bool(*b),
            Literal::Null => self.write("null"),
            Literal::Number(n) => self.write(&n),
            Literal::String(s) => self.write_string(s),
            Literal::RegEx(r) => self.write_regex(r),
            Literal::Template(t) => self.write_template(t),
        }
    }
    /// Write true or false
    pub fn write_bool(&mut self, boolean: bool) -> Res {
        trace!("write_bool");
        if boolean {
            self.write("true")
        } else {
            self.write("false")
        }
    }
    /// write a string, re-writes the string if quote configuration is set
    pub fn write_string(&mut self, s: &str) -> Res {
        trace!("write_string");
        if let Some(c) = self.quote {
            self.re_write_string(s, c)?;
        } else {
            self.write(s)?;
        }
        Ok(())
    }

    fn re_write_string(&mut self, s: &str, c: char) -> Res {
        let s = rewrite::re_write(s, c).unwrap_or(s.to_string());
        self.write(&s)?;;
        Ok(())
    }

    pub fn write_regex(&mut self, regex: &RegEx) -> Res {
        trace!("write_regex");
        self.write("/")?;
        self.write(&regex.pattern)?;
        self.write("/")?;
        self.write(&regex.flags)?;
        Ok(())
    }

    pub fn write_template(&mut self, template: &TemplateLiteral) -> Res {
        trace!("write_template");
        let mut quasis = template.quasis.iter();
        let mut exprs = template.expressions.iter();
        while let Some(quasi) = quasis.next() {
            self.write(&quasi.raw)?;
            if let Some(exp) = exprs.next() {
                self.write_expr(exp)?;
            }
        }
        Ok(())
    }

    pub fn write_empty_stmt(&mut self) -> Res {
        trace!("write_empty_stmt");
        self.write(";")
    }

    pub fn write_open_brace(&mut self) -> Res {
        trace!("write_open_brace");
        self.write("{")?;
        self.current_indent += 1;
        Ok(())
    }

    pub fn write_close_brace(&mut self) -> Res {
        trace!("write_close_brace");
        self.current_indent -= 1;
        self.write_leading_whitespace()?;
        self.write("}")?;
        Ok(())
    }

    pub fn write_leading_whitespace(&mut self) -> Res {
        trace!("write_leading_whitespace");
        self.write(&self.indent.repeat(self.current_indent))?;
        Ok(())
    }

    pub fn write_new_line(&mut self) -> Res {
        trace!("write_new_line");
        self.write(&self.new_line.clone())?;
        Ok(())
    }

    fn write(&mut self, s: &str) -> Res {
        let _ = self.out.write(s.as_bytes())?;
        Ok(())
    }
    pub fn write_comment(&mut self, comment: Comment) -> Res {
        match comment.kind {
            CommentKind::Single => self.write(&format!("//{}", comment.content))?,
            CommentKind::Multi => self.write(&format!("/*{}\n*/", comment.content))?,
            CommentKind::Html => self.write(&format!(
                "<!--{}-->{}",
                comment.content,
                comment.tail_content.unwrap_or(String::new())
            ))?,
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn write_empty_expression() {
        let mut f = write_str::WriteString::new();
        let mut w = Writer::new(f.generate_child());
        w.write_empty_stmt().unwrap();
        let out = f.get_string_lossy();
        assert_eq!(out, ";".to_string());
    }

    #[test]
    fn write_debugger_stmt() {
        let mut f = write_str::WriteString::new();
        let mut w = Writer::new(f.generate_child());
        w.write_debugger_stmt().unwrap();
        let s = f.get_string_lossy();
        assert_eq!(s, "debugger");
    }

    #[test]
    fn write_variable_decls() {
        let mut f = write_str::WriteString::new();
        let mut w = Writer::new(f.generate_child());
        w.write_variable_decls(
            &VariableKind::Var,
            &[VariableDecl::with_value("thing", Expr::boolean(false))],
        )
        .unwrap();
        let s = f.get_string_lossy();
        assert_eq!(s, "var thing = false;\n".to_string());
        let mut f = write_str::WriteString::new();
        let mut w = Writer::new(f.generate_child());
        w.write_variable_decls(
            &VariableKind::Let,
            &[
                VariableDecl::uninitialized("stuff"),
                VariableDecl::uninitialized("places"),
                VariableDecl::with_value("thing", Expr::boolean(false)),
            ],
        )
        .unwrap();
        let s = f.get_string_lossy();
        assert_eq!(s, "let stuff, places, thing = false;\n");
    }
}
