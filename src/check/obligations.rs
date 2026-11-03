// The trait obligation queue of one body (docs/ARCH.md sections 2.4 and 2.12).
//
// The checker (`check/mod.rs`) drives this queue at the fixed run
// points S3a planted: before a structural resolution (2.12), at
// coercion sites, and at body end to a fixpoint. This module owns
// the queue data; selection itself is `traits::solve`, run by the
// checker over its own inference context (`SolveCx` in
// `check/mod.rs`), because probes must bind and roll back the
// body's inference variables.
//
// Most obligations are trait refs from bounds at call sites. An
// `AssocEq` obligation comes from a selected impl's predicates
// and from bindings written at a use (`T: Iterator<Item = u8>`):
// it is proved by normalizing the projection, never by search.
use crate::diag::Span;
use crate::intern::FxMap;
use crate::traits::ImplId;
use crate::ty::{AssocEqData, GenericArg, TraitRef, Ty};

/// One pending obligation plus the span that needs it.
#[derive(Clone, Copy, Debug)]
pub enum Obligation {
    Trait { trait_ref: TraitRef, span: Span, depth: u16 },
    AssocEq { eq: AssocEqData, span: Span },
    /// A use that needs a dyn value's object lifetime to be `'static`
    /// (`check::object`) that the borrow checker cannot see (an impl for a
    /// `'static` dyn type picked by selection, a dyn call to a method with
    /// `where Self: 'static`); decided once at body end by the body-level
    /// rule (`Ctx::dyn_static_clean`).
    DynStatic { span: Span },
    /// A `'static` bound (`X: 'static`) on an instantiated type;
    /// decided once at body end (`Ctx::static_bound_err`).
    TypeStatic { ty: Ty, span: Span },
    /// The source of an unsizing coercion to a `'static` dyn type whose
    /// type still had inference variables there (`Ctx::coerce_unsize`):
    /// it must hold no borrow (`object::may_borrow_in`) once they are
    /// bound; decided once at body end.
    StaticSource { ty: Ty, span: Span },
}

/// The per-body work queue. Runs happen in `check/mod.rs`; see
/// the file header.
#[derive(Default)]
pub struct Obligations {
    queue: Vec<Obligation>,
    /// The impl and args each re-queued trait obligation (trait
    /// ref, span, depth) was last selected with; see `note_requeued`.
    requeued: FxMap<(TraitRef, Span, u16), (ImplId, Vec<GenericArg>)>,
    /// Obligations taken from the queue so far (each run round and
    /// the literal fallback take them all): the queue's work count
    /// (`Checked::queue_work`).
    work: usize,
}

impl Obligations {
    /// An empty queue for one body.
    pub fn new() -> Obligations {
        Obligations::default()
    }

    /// Records one obligation for a later run point.
    pub fn push(&mut self, obligation: Obligation) {
        self.queue.push(obligation);
    }

    /// Records one trait obligation at a nested depth.
    pub fn push_trait_at(&mut self, trait_ref: TraitRef, span: Span, depth: u16) {
        self.queue.push(Obligation::Trait { trait_ref, span, depth });
    }

    /// Raises the trait obligations queued since `from` to at least
    /// `depth`: the impl predicates a method pick queues are nested goals
    /// of the trait obligation the pick stands for (rustc counts them one
    /// level down for its recursion limit).
    pub fn deepen_from(&mut self, from: usize, depth: u16) {
        for ob in self.queue.iter_mut().skip(from) {
            if let Obligation::Trait { depth: d, .. } = ob {
                *d = (*d).max(depth);
            }
        }
    }

    /// True while nothing waits.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Takes the whole queue for one run. Obligations a run
    /// cannot finish are pushed back by the caller.
    pub fn take(&mut self) -> Vec<Obligation> {
        self.work += self.queue.len();
        std::mem::take(&mut self.queue)
    }

    /// The work count: obligations taken from the queue so far.
    pub fn work(&self) -> usize {
        self.work
    }

    /// The waiting obligations, in queue order. Closure signature
    /// deduction (ARCH 2.9) reads the `Fn` bounds on an expected type
    /// variable from here.
    pub fn pending(&self) -> &[Obligation] {
        &self.queue
    }

    /// How many obligations wait, for probe rollback.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Drops obligations queued since a `len` snapshot (a failed
    /// method probe).
    pub fn truncate(&mut self, len: usize) {
        self.queue.truncate(len);
    }

    /// Records the impl selection of a trait obligation that goes
    /// back on the queue because an impl parameter is still an
    /// inference variable. False when its last selection had the
    /// same impl and args: the nested predicates of that selection
    /// are queued, so this one repeats them and settles nothing.
    /// A selection with other args (a variable got bound) is new.
    pub fn note_requeued(&mut self, trait_ref: TraitRef, span: Span, depth: u16, id: ImplId, args: &[GenericArg]) -> bool {
        match self.requeued.get_mut(&(trait_ref, span, depth)) {
            Some(last) if last.0 == id && last.1 == args => false,
            Some(last) => {
                *last = (id, args.to_vec());
                true
            }
            None => {
                self.requeued.insert((trait_ref, span, depth), (id, args.to_vec()));
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::check::{self, collect};
    use crate::def::{CrateIds, LOCAL_CRATE};
    use crate::diag::FileId;
    use crate::intern::{Interner, Known};
    use crate::{lexer, parser};

    /// A generic impl over an integer literal (`Wrap(5)`): the obligation
    /// `Wrap<{integer}>: Show` selects the impl but stays queued until the
    /// literal's type is known. `run` is the body of main; the impls for
    /// `u8` and `u16` come first, then `extra`.
    fn sample(extra: &str, run: &str) -> String {
        format!(
            "trait Show {{ fn show(&self) -> u32; }}\n\
             impl Show for u8 {{ fn show(&self) -> u32 {{ *self as u32 }} }}\n\
             impl Show for u16 {{ fn show(&self) -> u32 {{ *self as u32 }} }}\n{extra}\n\
             struct Wrap<T>(T);\n\
             impl<T: Show> Show for Wrap<T> {{ fn show(&self) -> u32 {{ self.0.show() + 1 }} }}\n\
             struct Num(u32);\n\
             impl Num {{ fn get(&self) -> u32 {{ self.0 }} }}\n\
             fn need<S: Show>(s: S) -> u32 {{ s.show() }}\n\
             fn main() {{ {run} }}\n"
        )
    }

    /// Checks main of one sample file: (queue work count, error codes).
    fn check_main(src: &str) -> (usize, Vec<&'static str>) {
        let (work, errs) = check_main_diags(src);
        (work, errs.iter().map(|d| d.code).collect())
    }

    fn check_main_diags(src: &str) -> (usize, Vec<crate::diag::Diag>) {
        let interner = std::sync::Arc::new(Interner::default());
        let known = std::sync::Arc::new(Known::new(&interner));
        let lexed = lexer::lex(src, FileId(0), 0, &interner).unwrap();
        let ast = parser::parse(&lexed, &interner, &known).unwrap();
        let ids = CrateIds { local: LOCAL_CRATE, stable: crate::def::synthetic_crate_hash("test.rs", false) };
        let program = collect::collect(std::sync::Arc::new(ast), interner.clone(), known.clone(), ids).unwrap();
        let (ck, errs) = check::check_fn(&program, program.main.unwrap());
        (ck.queue_work, errs)
    }

    #[test]
    fn missing_from_impl_names_source_type() {
        let src = r#"
            struct Json;
            impl From<String> for Json {
                fn from(value: String) -> Json { Json }
            }
            impl Json {
                fn set<T: Into<Json>>(&mut self, name: &str, value: T) {}
            }
            fn main() {
                let mut json = Json;
                let tag = String::from("frog");
                json.set("name", &tag);
            }
        "#;
        for value in ["&tag", "&format!(\"{}\", tag)"] {
            let (_, errs) = check_main_diags(&src.replace("&tag", value));
            assert_eq!(errs.len(), 1, "{value}: {errs:?}");
            assert_eq!(errs[0].code, "L0612");
            assert_eq!(errs[0].message, "no impl of `From<&String>` for `Json`");
        }
        for value in ["tag", "format!(\"{}\", tag)"] {
            let (_, errs) = check_main_diags(&src.replace("&tag", value));
            assert!(errs.is_empty(), "owned String should work: {value}: {errs:?}");
        }
    }

    #[test]
    fn missing_impl_without_arguments_keeps_its_name() {
        let (_, errs) = check_main_diags("trait Show {} struct S; fn need<T: Show>(x: T) {} fn main() { need(S); }");
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].message, "no impl of `Show` for `S`");
    }

    /// Selecting the same impl again for a queued obligation is not
    /// progress and queues no copies. Counting it as progress ran each
    /// run point to the 128-round cap and copied the nested `{integer}:
    /// Show` every round (3.5 s for this main, 5 s for the 74 lines of
    /// tests/pass/fmt_user_impl.rs).
    #[test]
    fn requeued_impl_selection_is_not_repeated() {
        let run = "let a = need(Wrap(5)); let n = Num(a); let b = n.get() + n.get(); let c = n.get() * 2; \
                   let d = need(Wrap(Wrap(7))); let e = Num(d).get(); println!(\"{} {} {} {}\", a, b, c, e);";
        let ok = "impl Show for i32 { fn show(&self) -> u32 { *self as u32 } }";
        let (work, errs) = check_main(&sample(ok, run));
        assert!(errs.is_empty(), "the sample checks clean, got {errs:?}");
        // 87 today; the repeat gave 1.5 million.
        assert!(work <= 1000, "queue work {work}: a queued impl selection repeats each round");
    }

    /// The nested predicates still hold the obligation to account: with
    /// no `i32` impl, the literal falls back to `i32` and `i32: Show`
    /// fails once per distinct selection, not once per round (257 copies
    /// of the error before).
    #[test]
    fn requeued_impl_selection_keeps_its_nested_errors() {
        let (_, errs) = check_main(&sample("", "need(Wrap(5));"));
        assert!(!errs.is_empty() && errs.len() <= 2 && errs.iter().all(|&c| c == "L0612"), "want one or two L0612 errors, got {errs:?}");
    }
}
