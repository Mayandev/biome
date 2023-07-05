use rome_analyze::context::RuleContext;
use rome_analyze::{declare_rule, ActionCategory, Ast, Rule, RuleDiagnostic};
use rome_console::markup;
use rome_diagnostics::Applicability;
use rome_js_syntax::{JsFormalParameter, JsInitializerClause, JsSyntaxToken, TsPropertyParameter};
use rome_rowan::{declare_node_union, AstNode, BatchMutationExt, Direction};

use crate::JsRuleAction;

declare_rule! {
    /// Enforce default function parameters and optional function parameters to be last.
    ///
    /// Default and optional parameters that precede a required parameter cannot be omitted at call site.
    ///
    /// ## Examples
    ///
    /// ### Invalid
    ///
    /// ```js,expect_diagnostic
    /// function f(a = 0, b) {}
    /// ```
    ///
    /// ```js,expect_diagnostic
    /// function f(a, b = 0, c) {}
    /// ```
    ///
    /// ```ts,expect_diagnostic
    /// function f(a: number, b?: number, c: number) {}
    /// ```
    ///
    /// ```ts,expect_diagnostic
    /// class Foo {
    ///     constructor(readonly a = 10, readonly b: number) {}
    /// }
    /// ```
    ///
    /// ### Valid
    ///
    /// ```js
    /// function f(a, b = 0) {}
    /// ```
    ///
    /// ```ts
    /// function f(a: number, b?: number, c = 0) {}
    /// ```
    ///
    /// ```ts
    /// function f(a: number, b = 0, c?: number) {}
    /// ```
    ///
    pub(crate) UseDefaultParameterLast {
        version: "11.0.0",
        name: "useDefaultParameterLast",
        recommended: true,
    }
}

declare_node_union! {
    pub(crate) AnyFormalParameter = JsFormalParameter | TsPropertyParameter
}

impl AnyFormalParameter {
    pub(crate) fn is_required(&self) -> bool {
        self.question_mark_token().is_none() && self.initializer().is_none()
    }

    pub(crate) fn initializer(&self) -> Option<JsInitializerClause> {
        match self {
            AnyFormalParameter::JsFormalParameter(x) => x.initializer(),
            AnyFormalParameter::TsPropertyParameter(x) => x
                .formal_parameter()
                .ok()?
                .as_js_formal_parameter()?
                .initializer(),
        }
    }

    pub(crate) fn question_mark_token(&self) -> Option<JsSyntaxToken> {
        match self {
            AnyFormalParameter::JsFormalParameter(x) => x.question_mark_token(),
            AnyFormalParameter::TsPropertyParameter(x) => x
                .formal_parameter()
                .ok()?
                .as_js_formal_parameter()?
                .question_mark_token(),
        }
    }
}

impl Rule for UseDefaultParameterLast {
    type Query = Ast<AnyFormalParameter>;
    type State = AnyFormalParameter;
    type Signals = Option<Self::State>;
    type Options = ();

    fn run(ctx: &RuleContext<Self>) -> Self::Signals {
        let formal_param = ctx.query();
        if formal_param.is_required() {
            return None;
        }
        let last_required_param = formal_param
            .syntax()
            .siblings(Direction::Next)
            .filter_map(AnyFormalParameter::cast)
            .filter(|x| x.is_required())
            .last();
        last_required_param
    }

    fn diagnostic(
        ctx: &RuleContext<Self>,
        last_required_param: &Self::State,
    ) -> Option<RuleDiagnostic> {
        let formal_param = ctx.query();
        let param_kind = if formal_param.question_mark_token().is_some() {
            "optional"
        } else {
            "default"
        };
        Some(RuleDiagnostic::new(
            rule_category!(),
            formal_param.range(),
            markup! {
                "This "<Emphasis>{param_kind}" parameter"</Emphasis>" should follow the last "<Emphasis>"required parameter"</Emphasis>" or should be a "<Emphasis>"required parameter"</Emphasis>"."
            },
        ).detail(
            last_required_param.range(),
            markup! {
                "The last "<Emphasis>"required parameter"</Emphasis>" is here:"
            },
        ).note(
            markup! {
                "A "<Emphasis>{param_kind}" parameter"</Emphasis>" that precedes a "<Emphasis>"required parameter"</Emphasis>" cannot be omitted at call site."
            }
        ))
    }

    fn action(ctx: &RuleContext<Self>, _: &Self::State) -> Option<JsRuleAction> {
        let opt_param = ctx.query();
        let mut mutation = ctx.root().begin();
        if opt_param.question_mark_token().is_some() {
            let question_mark = opt_param.question_mark_token()?;
            let next_token = question_mark.next_token()?;
            let new_next_token = next_token.with_leading_trivia_pieces(
                question_mark
                    .leading_trivia()
                    .pieces()
                    .chain(question_mark.trailing_trivia().pieces())
                    .chain(next_token.leading_trivia().pieces())
                    .collect::<Vec<_>>(),
            );
            mutation.replace_token_discard_trivia(next_token, new_next_token);
            mutation.remove_token(question_mark);
        } else {
            let initializer = opt_param.initializer()?;
            let first_initializer_token = initializer.syntax().first_token()?;
            let last_initializer_token = initializer.syntax().last_token()?;
            let prev_initializer_token = first_initializer_token.prev_token()?;
            let trailing_trivia_count = prev_initializer_token.trailing_trivia().pieces().count();
            let last_trailing_non_space = prev_initializer_token
                .trailing_trivia()
                .pieces()
                .rev()
                .position(|p| !p.is_newline() && !p.is_whitespace())
                .unwrap_or(trailing_trivia_count);
            let new_prev_initializer_token = prev_initializer_token.with_trailing_trivia_pieces(
                prev_initializer_token
                    .trailing_trivia()
                    .pieces()
                    .take(trailing_trivia_count - last_trailing_non_space)
                    .chain(first_initializer_token.leading_trivia().pieces())
                    .chain(last_initializer_token.trailing_trivia().pieces())
                    .collect::<Vec<_>>(),
            );
            mutation
                .replace_token_discard_trivia(prev_initializer_token, new_prev_initializer_token);
            mutation.remove_node(initializer);
        }
        Some(JsRuleAction {
            mutation,
            message:
                markup! {"Turn the parameter into a "<Emphasis>"required parameter"</Emphasis>"."}
                    .to_owned(),
            category: ActionCategory::QuickFix,
            applicability: Applicability::MaybeIncorrect,
        })
    }
}