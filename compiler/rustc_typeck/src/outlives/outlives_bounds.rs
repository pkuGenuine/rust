use rustc_hir as hir;
use rustc_middle::ty::{self, Ty};
use rustc_span::source_map::Span;
use rustc_trait_selection::infer::InferCtxt;
use rustc_trait_selection::traits::query::type_op::{self, TypeOp, TypeOpOutput};
use rustc_trait_selection::traits::query::NoSolution;
use rustc_trait_selection::traits::{FulfillmentContext, ObligationCause, TraitEngine};

pub use rustc_middle::traits::query::OutlivesBound;

pub trait InferCtxtExt<'tcx> {
    fn implied_outlives_bounds(
        &self,
        param_env: ty::ParamEnv<'tcx>,
        body_id: hir::HirId,
        ty: Ty<'tcx>,
        span: Span,
    ) -> Vec<OutlivesBound<'tcx>>;
}

impl<'cx, 'tcx> InferCtxtExt<'tcx> for InferCtxt<'cx, 'tcx> {
    /// Implied bounds are region relationships that we deduce
    /// automatically. The idea is that (e.g.) a caller must check that a
    /// function's argument types are well-formed immediately before
    /// calling that fn, and hence the *callee* can assume that its
    /// argument types are well-formed. This may imply certain relationships
    /// between generic parameters. For example:
    /// ```
    /// fn foo<'a,T>(x: &'a T) {}
    /// ```
    /// can only be called with a `'a` and `T` such that `&'a T` is WF.
    /// For `&'a T` to be WF, `T: 'a` must hold. So we can assume `T: 'a`.
    ///
    /// # Parameters
    ///
    /// - `param_env`, the where-clauses in scope
    /// - `body_id`, the body-id to use when normalizing assoc types.
    ///   Note that this may cause outlives obligations to be injected
    ///   into the inference context with this body-id.
    /// - `ty`, the type that we are supposed to assume is WF.
    /// - `span`, a span to use when normalizing, hopefully not important,
    ///   might be useful if a `bug!` occurs.
    #[instrument(level = "debug", skip(self, param_env, body_id, span))]
    fn implied_outlives_bounds(
        &self,
        param_env: ty::ParamEnv<'tcx>,
        body_id: hir::HirId,
        ty: Ty<'tcx>,
        span: Span,
    ) -> Vec<OutlivesBound<'tcx>> {
        let result = param_env
            .and(type_op::implied_outlives_bounds::ImpliedOutlivesBounds { ty })
            .fully_perform(self);
        let result = match result {
            Ok(r) => r,
            Err(NoSolution) => {
                self.tcx.sess.delay_span_bug(
                    span,
                    "implied_outlives_bounds failed to solve all obligations",
                );
                return vec![];
            }
        };

        let TypeOpOutput { output, constraints, .. } = result;

        if let Some(constraints) = constraints {
            // Instantiation may have produced new inference variables and constraints on those
            // variables. Process these constraints.
            let mut fulfill_cx = FulfillmentContext::new();
            let cause = ObligationCause::misc(span, body_id);
            for &constraint in &constraints.outlives {
                let obligation = self.query_outlives_constraint_to_obligation(
                    constraint,
                    cause.clone(),
                    param_env,
                );
                fulfill_cx.register_predicate_obligation(self, obligation);
            }
            if !constraints.member_constraints.is_empty() {
                span_bug!(span, "{:#?}", constraints.member_constraints);
            }
            let errors = fulfill_cx.select_all_or_error(self);
            if !errors.is_empty() {
                self.tcx.sess.delay_span_bug(
                    span,
                    "implied_outlives_bounds failed to solve obligations from instantiation",
                );
            }
        };

        output
    }
}
