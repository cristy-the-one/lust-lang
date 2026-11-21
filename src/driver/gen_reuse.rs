// Generic-program reuse across daemon requests (docs/ARCH.md section 8;
// W-PERF wave B). Collect checks every template once against its bounds
// (`generics::template_check`) and runs instance discovery over every
// plain fn and every instance it makes (`generics::materialize`). This
// cache keeps both results per stable def across the requests of one
// daemon session, so collect checks only the bodies an edit touched and
// replays the rest in the cold order.
//
// A stored result depends on three things, and holds while all three do:
// - the trait system: trait items, impl items of lang and foreign
//   traits, and inherent impls with an associated const or a `dyn` Self,
//   each as written and resolved (`trait_key`). Collect drops every
//   entry when their fingerprint changes (`GenReuse::gate`), and the
//   driver also drops them on a const fn body edit, a nested static, or
//   a slot renumber (`compile.rs`, "generic reuse"). Derive lists are
//   not in the key (PLAN R12): every answer that reads one records a
//   `Derives` read (check/derive_delta.rs), so a derive edit rechecks
//   only the bodies that asked; codegen's reads join each slot's read
//   set, and a build regenerates the glue on a derive edit
//   (`build_derives_moved`). Impls of user traits are not in the key:
//   selection records an `ImplSet` read of the bucket it searched
//   (driver/graph.rs), so an added, removed, or edited user trait impl
//   rechecks only the bodies that read its bucket. Other inherent impls
//   are not in it either: method probes and inherent table lookups
//   record `MethodSet` reads (`Program::inherent_reads`).
// - the source: the body and head hash of the AST fn whose text the
//   body checks, and what the body takes from other checks in this
//   collect. An instance's source is its template's source plus the
//   template check's outputs it replays (picks, closure call kinds and
//   shapes): its own reads miss the probes that chose those picks. A
//   closure body fn's source is its parent's plus the closure record the
//   parent's check made. A trait default method's text is part of its
//   trait item, under the first key.
// - everything else the check read (section 8.2 `Reads`: bindings,
//   method sets, signatures with generics and bounds, layouts). An entry
//   made under another item-level environment replays its reads against
//   this program first.
use crate::ast::ExprId;
use crate::check::closure::ClosureKind;
use crate::check::collect::Program;
use crate::check::generics::{CaptureShape, FnClass, OwnedArg, OwnedClosure, OwnedTy};
use crate::check::picks::TraitPick;
use crate::def::{DefId, LocalDefId};
use crate::driver::graph::{Read, ReadKey, ReadKeys, Reads};
use crate::intern::{FxHasher, FxMap};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// One passing template check of a body with no closure expressions:
/// its trait picks and closure call kinds, by expression offset from
/// the template body's first expression (`ExprId`s move when the merged
/// Ast is rebuilt; offsets inside one body do not).
struct TemplateEntry {
    src: u64,
    /// The item-level environment the entry last held under.
    env: u64,
    /// When it was stored (`next_stamp`).
    stamp: u64,
    reads: Reads,
    n_exprs: u32,
    picks: Vec<(u32, TraitPick)>,
    calls: Vec<(u32, ClosureKind)>,
}

/// One clean check of a template with closures: its reads, and what a
/// replay needs (`out`). The derive delta compares the reads to know
/// whether the kept program's check of the template still holds
/// (driver/kept_derive.rs); collect replays `out` (`closure_template`).
struct ClosureTemplateEntry {
    src: u64,
    env: u64,
    /// When it was stored (`next_stamp`).
    stamp: u64,
    reads: Reads,
    n_exprs: u32,
    out: Option<ClosureTemplateOut>,
}

/// The output of one clean check of a template with closures: its picks
/// and closure call kinds and its closures' capture shapes, by expression
/// offset from the body's first expression (as `TemplateEntry`), and its
/// closures' records, from which a replay makes their body fns again.
struct ClosureTemplateOut {
    picks: Vec<(u32, TraitPick)>,
    calls: Vec<(u32, ClosureKind)>,
    shapes: Vec<(u32, ClosureKind, Vec<CaptureShape>)>,
    closures: Vec<OwnedClosure>,
}

/// One clean discovery check: the Adt types the body names, its
/// instance requests (template def, owned args), its closures, and its
/// vtable requests (concrete type, dyn type), in check order.
struct DiscoveryEntry {
    src: u64,
    env: u64,
    /// When it was stored (`next_stamp`).
    stamp: u64,
    reads: Reads,
    /// Each one shared with the other entries that hold the same type (`GenReuse::shared`).
    adt_tys: Vec<Arc<OwnedTy>>,
    /// The args shared as `adt_tys` are (`GenReuse::shared_args`).
    requests: Vec<(DefId, Arc<[OwnedArg]>)>,
    closures: Vec<OwnedClosure>,
    vtables: Vec<(OwnedTy, OwnedTy)>,
}

/// A copy of one stored discovery output (`GenReuse::out_copy`).
#[derive(Clone)]
pub(crate) struct OutCopy {
    adt_tys: Vec<Arc<OwnedTy>>,
    requests: Vec<(DefId, Arc<[OwnedArg]>)>,
    closures: Vec<OwnedClosure>,
    vtables: Vec<(OwnedTy, OwnedTy)>,
}

/// A discovery type as a tree for `LUST_DERIVE_LOG`: every variant with
/// its defs (by hash), args, regions and leaves.
fn owned_tree(t: &OwnedTy) -> String {
    use crate::check::generics::OwnedArg as A;
    let args = |xs: &[A]| {
        xs.iter()
            .map(|a| match a {
                A::Ty(t) => owned_tree(t),
                A::Lifetime(r) => format!("{r:?}"),
                A::Const(c) => format!("{c:?}"),
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    match t {
        OwnedTy::Leaf(k) => format!("{k:?}"),
        OwnedTy::Ref(r, m, x) => format!("Ref({r:?}, {m:?}, {})", owned_tree(x)),
        OwnedTy::RawPtr(m, x) => format!("Ptr({m:?}, {})", owned_tree(x)),
        OwnedTy::Slice(x) => format!("Slice({})", owned_tree(x)),
        OwnedTy::Array(x, c) => format!("Array({}, {c:?})", owned_tree(x)),
        OwnedTy::Tuple(xs) => format!("Tuple({})", xs.iter().map(owned_tree).collect::<Vec<_>>().join(", ")),
        OwnedTy::Adt(d, xs) => format!("Adt({:x}, [{}])", d.hash.0, args(xs)),
        OwnedTy::FnDef(d, xs) => format!("FnDef({:x}, [{}])", d.hash.0, args(xs)),
        OwnedTy::Closure(d, xs) => format!("Closure({:x}, [{}])", d.hash.0, args(xs)),
        OwnedTy::Opaque(d, xs) => format!("Opaque({:x}, [{}])", d.hash.0, args(xs)),
        OwnedTy::Projection(a, xs) => format!("Proj({a:?}, [{}])", args(xs)),
        OwnedTy::FnPtr(..) => "FnPtr(..)".into(),
        OwnedTy::Dyn(p, auto, r) => format!("Dyn({}, {auto:?}, {r:?})", p.as_ref().map_or("none".into(), |(d, xs, _)| format!("{:x} [{}]", d.hash.0, args(xs)))),
    }
}

impl OutCopy {
    /// True when fn `f`'s new discovery output `out` equals this copy
    /// where it can make anything: the closures, vtables, and the types
    /// that can name an Adt instance, in order. Collect then makes the
    /// same closures, instances, vtables, and glue from it (the other
    /// types only enter the global store; the caller interns them). The
    /// instance requests must match too where discovery walks them: in a
    /// build (`walk_instances`), and in a check of a program with generic
    /// associated consts (`patch::walks_requests`), whose new instances
    /// evaluate those consts.
    pub(crate) fn same(&self, p: &Program, out: &Discovered, walk_instances: bool) -> bool {
        let walk_instances = walk_instances || super::patch::walks_requests(p);
        let (adt_tys, requests, closures, vtables) = out;
        let makers = |ts: &[Arc<OwnedTy>]| ts.iter().filter(|t| crate::check::generics::may_make_instances(t)).cloned().collect::<Vec<_>>();
        self.closures == *closures
            && makers(&self.adt_tys) == makers(adt_tys)
            && self.vtables == *vtables
            && (!walk_instances
                || self.requests.len() == requests.len()
                    && self.requests.iter().zip(requests).all(|((d, a), (t, b))| *d == p.def_id(*t) && a == b))
    }

    /// The types that can name an Adt instance (`may_make_instances`) of
    /// this copy and of `out`, in order, when the rest that `same` compares
    /// in a check is equal (the closures and the vtables); None otherwise.
    /// The kept program admits an edit when the two lists are the same once
    /// the types whose instances were made before the body's turn are left
    /// out (driver/patch.rs `made_before`).
    pub(crate) fn makers(&self, out: &Discovered) -> Option<(Vec<Arc<OwnedTy>>, Vec<Arc<OwnedTy>>)> {
        let (adt_tys, _, closures, vtables) = out;
        if self.closures != *closures || self.vtables != *vtables {
            return None;
        }
        let makers = |ts: &[Arc<OwnedTy>]| ts.iter().filter(|t| crate::check::generics::may_make_instances(t)).cloned().collect::<Vec<_>>();
        Some((makers(&self.adt_tys), makers(adt_tys)))
    }

    /// Which part of `out` differs from this copy where `same` compares
    /// (a refusal log line).
    pub(crate) fn differs(&self, p: &Program, out: &Discovered) -> String {
        let (adt_tys, _, closures, vtables) = out;
        let makers = |ts: &[Arc<OwnedTy>]| ts.iter().filter(|t| crate::check::generics::may_make_instances(t)).count();
        let mut parts = Vec::new();
        if self.closures != *closures {
            let at = self.closures.iter().zip(closures).position(|(a, b)| a != b);
            parts.push(format!("closures {} -> {} (first at {at:?})", self.closures.len(), closures.len()));
        }
        let (a, b): (Vec<_>, Vec<_>) = (
            self.adt_tys.iter().filter(|t| crate::check::generics::may_make_instances(t)).collect(),
            adt_tys.iter().filter(|t| crate::check::generics::may_make_instances(t)).collect(),
        );
        if a != b {
            let same_set = a.iter().all(|t| b.contains(t)) && b.iter().all(|t| a.contains(t));
            let gone: Vec<String> = a.iter().filter(|t| !b.contains(t)).take(3).map(|t| owned_str(p, t)).collect();
            let new: Vec<String> = b.iter().filter(|t| !a.contains(t)).take(3).map(|t| owned_str(p, t)).collect();
            parts.push(format!("makers {} -> {} (same set {same_set}; gone {gone:?}, new {new:?})", makers(&self.adt_tys), makers(adt_tys)));
            // The first pair that differs, as trees (`owned_str` leaves out
            // a closure's args and a region's form).
            if let (Some(x), Some(y)) = (a.iter().find(|t| !b.contains(t)), b.iter().find(|t| !a.contains(t))) {
                parts.push(format!("first gone {}, first new {}", owned_tree(x), owned_tree(y)));
            }
        }
        if self.vtables != *vtables {
            parts.push(format!("vtables {} -> {}", self.vtables.len(), vtables.len()));
        }
        parts.join(", ")
    }
}

/// A short text of an owned type for a refusal log line (Adt names,
/// other leaves by kind).
fn owned_str(p: &Program, t: &OwnedTy) -> String {
    let args = |a: &[crate::check::generics::OwnedArg]| {
        let xs: Vec<String> = a
            .iter()
            .map(|g| match g {
                crate::check::generics::OwnedArg::Ty(t) => owned_str(p, t),
                crate::check::generics::OwnedArg::Lifetime(r) => format!("{r:?}"),
                crate::check::generics::OwnedArg::Const(_) => "const".to_string(),
            })
            .collect();
        xs.join(", ")
    };
    match t {
        OwnedTy::Adt(d, a) => {
            let name = p.adt_by_def(*d).map_or_else(|| format!("#{:x}", d.hash.0), |x| p.display_name(p.adt(x).name).to_string());
            format!("{name}<{}>", args(a))
        }
        OwnedTy::Ref(r, m, e) => format!("&{r:?} {m:?} {}", owned_str(p, e)),
        OwnedTy::RawPtr(_, e) => format!("*{}", owned_str(p, e)),
        OwnedTy::Slice(e) => format!("[{}]", owned_str(p, e)),
        OwnedTy::Array(e, _) => format!("[{}; _]", owned_str(p, e)),
        OwnedTy::Tuple(ts) => format!("({})", ts.iter().map(|t| owned_str(p, t)).collect::<Vec<_>>().join(", ")),
        OwnedTy::Leaf(k) => format!("{k:?}"),
        OwnedTy::Opaque(d, a) => format!("opaque#{:x}<{}>", d.hash.0, args(a)),
        OwnedTy::Closure(d, _) => format!("closure#{:x}", d.hash.0),
        _ => "other".to_string(),
    }
}

/// What one stored discovery check replays: Adt types, requests with
/// templates as this program's local ids, closures to register, and
/// vtable requests. A replay hands out the types and request args the
/// entries share (`GenReuse::shared`), not copies of them.
pub(crate) type Discovered = (Vec<Arc<OwnedTy>>, Vec<(LocalDefId, Arc<[OwnedArg]>)>, Vec<OwnedClosure>, Vec<(OwnedTy, OwnedTy)>);

type FxSet<K> = std::collections::HashSet<K, std::hash::BuildHasherDefault<FxHasher>>;

/// One discovery entry judged before its turn (`GenReuse::prejudge`): the
/// entry's stamp then, whether its keys other than `Answer` held, its
/// `Answer` keys with their stored fingerprints in stored order (asked at
/// the turn), and a copy of its output when the rest held.
pub(crate) struct Prejudged {
    stamp: u64,
    held: bool,
    answers: Vec<(ReadKey, Read)>,
    out: Option<(Vec<Arc<OwnedTy>>, Vec<(DefId, Arc<[OwnedArg]>)>, Vec<OwnedClosure>, Vec<(OwnedTy, OwnedTy)>)>,
}

/// `LUST_CHECK_PREJUDGE=1`: at each prejudged turn, every key of the entry
/// that `prejudge` answered must have that answer still, fingerprinted
/// again now (`prejudge_oracle`).
fn prejudge_check() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("LUST_CHECK_PREJUDGE").is_some_and(|v| v != "0"))
}

/// The oracle of `prejudge_check`: panics at the first key whose answer
/// moved between the round start and this turn.
fn prejudge_oracle(p: &Program, memo: &FxMap<ReadKey, Read>, keys: &ReadKeys, e: &DiscoveryEntry, env: u64, pre: &Prejudged) {
    if e.env == env {
        return;
    }
    for (k, _) in e.reads.iter(keys) {
        let (now, stable) = p.read_fingerprint_stable(k);
        if matches!(k, ReadKey::Answer(..)) || !stable {
            continue;
        }
        if memo.get(k) != Some(&now) {
            panic!("prejudge oracle: key {k:?} answered {:?} at the round start, {now:?} at the turn (held {})", memo.get(k), pre.held);
        }
    }
}

impl GenReuse {
    /// True when no template checks are cached yet (e.g. a daemon's first check).
    pub(crate) fn templates_empty(&self) -> bool {
        self.templates.is_empty() && self.closure_templates.is_empty()
    }

    /// For `main::census` (LUST_FRONT_CENSUS=1, a probe of a front image): drops one part
    /// (0 templates, 1 closure templates, 2 discovery, 3 unstored, 4 held, 5 memo, 6 template
    /// and closure outputs, 7 readers index; 8 the discovery entries' read sets, 9 their
    /// outputs, 10 the read keys).
    pub(crate) fn drop_part(&mut self, part: usize) {
        use std::mem::take;
        match part {
            0 => drop(take(&mut self.templates)),
            1 => drop(take(&mut self.closure_templates)),
            2 => drop(take(&mut self.discovery)),
            3 => drop(take(&mut self.unstored)),
            4 => drop(self.held.take()),
            5 => drop(take(&mut self.memo)),
            6 => {
                drop(take(&mut self.tpl_out));
                drop(take(&mut self.closure_out));
                drop(self.prev_outs.take());
            }
            7 => drop(take(&mut self.readers_index)),
            8 => self.discovery.values_mut().for_each(|e| drop(take(&mut e.reads))),
            9 => {
                self.discovery.values_mut().for_each(|e| {
                    drop(take(&mut e.adt_tys));
                    drop(take(&mut e.requests));
                    drop(take(&mut e.closures));
                    drop(take(&mut e.vtables));
                });
                drop(take(&mut self.shared));
                drop(take(&mut self.shared_args));
            }
            10 => drop(take(&mut self.keys)),
            _ => {}
        }
    }
}

/// The session's generic reuse cache. `collect_reuse` moves it into
/// `GenTables::reuse` for one collect and back out after.
#[derive(Default)]
pub struct GenReuse {
    /// False: every lookup misses and nothing is stored (one-shot
    /// sessions, and `collect` callers outside the driver).
    pub on: bool,
    /// The type store of the last program a full collect replaced, for the
    /// next collect to start from (B1-PLAN.md, `ty::TypePool`).
    pub(crate) type_pool: Option<crate::ty::TypePool>,
    /// True when the process exits after this compile (no daemon): no
    /// later request replays a read set, so collect's body checks
    /// (template checks, mono discovery) record no reads and compute no
    /// fingerprints (`Program::record_reads`). The driver sets it.
    pub one_shot: bool,
    /// Source fingerprint of each AST fn, in `LocalDefId` order (body
    /// and head hash), set by the driver before collect.
    pub ast_src: Vec<u64>,
    /// This request's item-level environment (item-scan hash and AST fn
    /// count), set by the driver before collect.
    pub env: u64,
    /// The read keys the frozen read sets of these entries name
    /// (`Reads::freeze`); `clear` drops them with the entries.
    keys: ReadKeys,
    templates: FxMap<DefId, TemplateEntry>,
    /// The last clean check of each template with closures, if its last
    /// check was clean (`store_closure_template`).
    closure_templates: FxMap<DefId, ClosureTemplateEntry>,
    discovery: FxMap<DefId, DiscoveryEntry>,
    /// The distinct types of the discovery entries' `adt_tys`, each kept once (`share`): on
    /// tsgo, ~525k such types hold ~50k distinct ones.
    shared: FxSet<Arc<OwnedTy>>,
    /// The distinct arg lists of the entries' instance requests, as `shared`.
    shared_args: FxSet<Arc<[OwnedArg]>>,
    /// When `shared` and `shared_args` hold this many values together, `store_discovered`
    /// drops the ones no entry holds.
    prune_at: usize,
    /// Defs whose last discovery check stored nothing (a body error), so
    /// their entry, if any, is older than what that collect used. Each
    /// keeps that check's output, stamped (`next_stamp`): never replayed,
    /// only compared (`out_copy`). A `lust check` body whose check stays
    /// pending (`Checked::closure_map_pending`, no error) also keeps that
    /// check's source hash and reads (`pending_untouched`).
    unstored: FxMap<DefId, (u64, OutCopy, Option<(u64, Reads)>)>,
    /// For a kept program put back after a refused delta (`hold`): its
    /// `Program::reuse_stamp`, and the discovery output it has for each def
    /// whose entry or note a later collect replaced (the full collect that
    /// answered that request, often with errors).
    held: Option<(u64, FxMap<DefId, OutCopy>)>,
    /// Node answers of this collect's program, for read replays (and the
    /// derive delta's keyed walk, graph.rs `keyed_dirty_slots`).
    pub(crate) memo: FxMap<ReadKey, Read>,
    /// This collect's template check outputs that instances replay, by
    /// template, and its closure records, by closure def (fingerprints;
    /// see `src_of`).
    tpl_out: FxMap<LocalDefId, u64>,
    closure_out: FxMap<DefId, u64>,
    /// `tpl_out` and `closure_out` as the last collect began: a collect
    /// that fails gives them back (`forget_failed_gate`), since the kept
    /// program the next request takes is the one they describe.
    prev_outs: Option<(FxMap<LocalDefId, u64>, FxMap<DefId, u64>)>,
    /// This collect's work, for the stats line: checks run and reused.
    pub templates_checked: usize,
    pub templates_reused: usize,
    pub discovery_checked: usize,
    pub discovery_reused: usize,
    /// Errors this collect's template checks reported (body errors).
    pub template_errors: usize,
    /// The trait-system key of the last collect that reached its body
    /// checks (`gate`).
    trait_key: Option<u64>,
    /// The derive-list key of that collect (`TraitKey::derives`).
    derive_key: Option<u64>,
    /// The derive-list key the last build made its C under
    /// (`build_derives_moved`).
    build_derive_key: Option<u64>,
    /// True unless this collect's trait-system key equals the last one;
    /// then every entry was dropped, and the driver rechecks every body.
    pub trait_changed: bool,
    /// The key of the std and lang trait impls the trait-system key
    /// leaves to `LangImpls` reads (`TraitKey::lang`), and whether this
    /// collect moved it: the driver then regenerates the glue.
    lang_key: Option<u64>,
    pub lang_changed: bool,
    /// True once this collect reached `gate`.
    gated: bool,
    /// The defs whose stored entry read each node a header delta can move
    /// (`readers`): a superset, since a replaced entry leaves its old keys
    /// until the next rebuild (`index_reads`).
    readers_index: FxMap<ReaderKey, Vec<DefId>>,
    /// Keys pushed into `readers_index` since its last rebuild.
    readers_pushed: usize,
}

/// A node a header delta can move, as `GenReuse::readers_index` keys it:
/// a def's signature, a binding by the def it resolves to, a method set
/// of an Adt head by (Adt def, name), and any `Checked` read.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ReaderKey {
    Sig(DefId),
    Method(DefId, crate::intern::Sym),
    Def(DefId),
    Checked,
    /// An `ImplSet` read of a bucket of this trait (any head).
    Trait(DefId),
}

impl GenReuse {
    /// Drops every entry: the trait system they were made under changed.
    pub fn clear(&mut self) {
        self.keys = ReadKeys::default();
        self.templates.clear();
        self.closure_templates.clear();
        self.discovery.clear();
        self.shared.clear();
        self.shared_args.clear();
        self.readers_index.clear();
        self.readers_pushed = 0;
        self.unstored.clear();
        self.held = None;
    }

    /// Drops the entries a collect under `env` stored or replayed (a
    /// failed collect: its static ids are unknown). Entries made under
    /// another env stay.
    pub fn drop_env(&mut self, env: u64) {
        self.templates.retain(|_, e| e.env != env);
        self.closure_templates.retain(|_, e| e.env != env);
        let gone: Vec<DefId> = self.discovery.iter().filter(|(_, e)| e.env == env).map(|(d, _)| *d).collect();
        for def in gone {
            self.keep_held(def);
        }
        self.discovery.retain(|_, e| e.env != env);
    }

    /// Starts to keep the discovery outputs the kept program with stamp `lo`
    /// has, for the defs whose entry or note later collects replace
    /// (driver/patch.rs: a refused delta puts its program back for the next
    /// request, and the full collect that answers in between stores its own).
    pub(crate) fn hold(&mut self, lo: u64) {
        if self.held.as_ref().is_none_or(|h| h.0 != lo) {
            self.held = Some((lo, FxMap::default()));
        }
    }

    /// Before the entry or note of `def` changes: keeps the output the held
    /// program has for it, when the current one is no newer than its stamp
    /// (a note comes after any entry of the same def: `store_discovered`
    /// drops it).
    fn keep_held(&mut self, def: DefId) {
        let Some((lo, map)) = &self.held else { return };
        if map.contains_key(&def) {
            return;
        }
        let stamp = self.unstored.get(&def).map(|n| n.0).or_else(|| self.discovery.get(&def).map(|e| e.stamp));
        if stamp.is_some_and(|s| s <= *lo)
            && let Some(copy) = self.out_copy(def)
            && let Some((_, map)) = &mut self.held
        {
            map.insert(def, copy);
        }
    }

    /// The discovery output the kept program with stamp `lo` has for fn
    /// `f`, when a later collect replaced its entry or note (`hold`).
    pub(crate) fn held_copy(&self, p: &Program, f: LocalDefId, lo: u64) -> Option<OutCopy> {
        let (held_lo, map) = self.held.as_ref()?;
        if *held_lo != lo {
            return None;
        }
        map.get(&p.def_id(f)).cloned()
    }

    /// Notes that a fresh discovery check of `f` stored nothing.
    pub fn note_unstored(
        &mut self,
        p: &Program,
        f: LocalDefId,
        adt_tys: &[Arc<OwnedTy>],
        requests: &[(LocalDefId, Arc<[OwnedArg]>)],
        closures: &[OwnedClosure],
        vtables: &[(OwnedTy, OwnedTy)],
        pending: Option<&Reads>,
    ) {
        self.keep_held(p.def_id(f));
        // Copies, as `share` makes them.
        let requests = requests.iter().map(|(t, args)| (p.def_id(*t), Arc::from(&**args))).collect();
        let adt_tys = adt_tys.iter().map(|t| Arc::new((**t).clone())).collect();
        let copy = OutCopy { adt_tys, requests, closures: closures.to_vec(), vtables: vtables.to_vec() };
        let pending = match pending.filter(|_| self.on) {
            Some(r) => self.src_of(p, f).map(|src| (src, with_own_sig(p, f, r, &mut self.keys))),
            None => None,
        };
        self.unstored.insert(p.def_id(f), (next_stamp(), copy, pending));
    }

    /// Why `pending_untouched` is false for fn `f` (`LUST_DERIVE_LOG`).
    pub(crate) fn pending_why(&self, p: &Program, f: LocalDefId, touched: &mut impl FnMut(&ReadKey) -> bool) -> String {
        match self.unstored.get(&p.def_id(f)) {
            None => "no note".into(),
            Some((_, _, None)) => "no reads".into(),
            Some((_, _, Some((src, _)))) if self.src_of(p, f) != Some(*src) => "source".into(),
            Some((_, _, Some((_, reads)))) => {
                let moved = reads.iter(&self.keys).filter(|(k, _)| touched(k)).count();
                let differ = reads.iter(&self.keys).find(|(k, stored)| touched(k) && p.read_fingerprint(k) != *stored);
                format!("{moved} moved reads, differs: {}", differ.map_or("none".into(), |(k, _)| format!("{k:?}")))
            }
        }
    }

    /// True when fn `f`'s last discovery check was a pending one of
    /// `lust check` (`note_unstored` kept its reads), made from its current
    /// source, and each of its reads that `moves` names keeps its
    /// fingerprint (as `template_holds_moved`): a check again would give
    /// the noted output once more (the derive delta's replay skips it).
    pub(crate) fn pending_holds_moved(&mut self, p: &Program, f: LocalDefId, moves: &mut impl FnMut(&ReadKey) -> bool) -> bool {
        let Some((_, _, Some((src, reads)))) = self.unstored.get(&p.def_id(f)) else { return false };
        if !self.on || self.src_of(p, f) != Some(*src) {
            return false;
        }
        let memo = &mut self.memo;
        reads.iter(&self.keys).all(|(k, stored)| !moves(k) || *memo.entry(*k).or_insert_with(|| p.read_fingerprint(k)) == stored)
    }

    /// Drops every entry when the trait-system key moved since the last
    /// collect. Collect calls it once the impl index is built, before any
    /// template check or discovery runs. A derive-list change alone keeps
    /// the entries: their `Derives` reads decide.
    pub fn gate(&mut self, key: TraitKey) {
        self.trait_changed = self.trait_key != Some(key.system);
        if self.trait_changed {
            self.clear();
        }
        self.trait_key = Some(key.system);
        self.derive_key = Some(key.derives);
        self.lang_changed = self.lang_key != Some(key.lang);
        self.lang_key = Some(key.lang);
        self.gated = true;
    }

    /// For a collect that failed after `gate` saw a new trait system: no
    /// body rechecked under it, so the next collect must see the trait
    /// system as changed again (drop every entry, recheck every body).
    /// The output fingerprints the failed collect cleared come back: a
    /// kept program that later requests take holds those outputs.
    pub fn forget_failed_gate(&mut self) {
        if self.gated && self.trait_changed {
            self.trait_key = None;
        }
        if self.gated && self.lang_changed {
            self.lang_key = None;
        }
        if let Some((t, c)) = self.prev_outs.take() {
            (self.tpl_out, self.closure_out) = (t, c);
        }
    }

    /// For a build: true when derive lists moved since the last build
    /// made C, and records this build's key. Derived glue reads derive
    /// lists with no recorded read, so such a build regenerates the glue;
    /// each slot's codegen records its own reads (compile.rs). The first
    /// build of a session has no C to keep.
    pub fn build_derives_moved(&mut self) -> bool {
        let moved = self.build_derive_key.is_some() && self.build_derive_key != self.derive_key;
        self.build_derive_key = self.derive_key;
        moved
    }

    /// Resets the per-collect state before a collect runs.
    pub fn begin(&mut self) {
        self.trait_changed = true;
        self.gated = false;
        self.memo.clear();
        self.prev_outs = Some((std::mem::take(&mut self.tpl_out), std::mem::take(&mut self.closure_out)));
        self.templates_checked = 0;
        self.templates_reused = 0;
        self.discovery_checked = 0;
        self.discovery_reused = 0;
        self.template_errors = 0;
    }

    /// Resets this request's work counters when the daemon's kept program
    /// stands in for collect (driver/patch.rs): no collect ran, so no
    /// template check, and the trait system stands.
    pub fn begin_kept(&mut self) {
        self.prev_outs = None;
        self.trait_changed = false;
        self.gated = false;
        self.templates_checked = 0;
        self.templates_reused = 0;
        self.discovery_checked = 0;
        self.discovery_reused = 0;
        self.template_errors = 0;
    }

    /// The source fingerprint of one fn of `p` (see the module docs),
    /// or None for a fn with no checked body of its own (a trait method
    /// stub) or whose upstream check output is not known yet.
    pub fn src_of(&self, p: &Program, f: LocalDefId) -> Option<u64> {
        self.src_depth(p, f, 0)
    }

    fn src_depth(&self, p: &Program, f: LocalDefId, depth: u32) -> Option<u64> {
        if let Some(&h) = self.ast_src.get(f.0 as usize) {
            return Some(h);
        }
        if depth > 64 {
            return None;
        }
        let mix = |a: u64, b: u64| {
            let mut h = FxHasher::default();
            (a, b).hash(&mut h);
            h.finish()
        };
        match p.gx.fn_class.get(f.0 as usize)? {
            FnClass::Instance => {
                let t = p.gx.instances.get(&f)?.template_local?;
                Some(mix(self.src_depth(p, t, depth + 1)?, *self.tpl_out.get(&t)?))
            }
            FnClass::Closure | FnClass::TemplateClosure => {
                let def = *p.gx.closure_fns.get(&f)?;
                let parent = p.gx.closures.get(&def)?.parent;
                Some(mix(self.src_depth(p, parent, depth + 1)?, *self.closure_out.get(&def)?))
            }
            // A trait default body: its text is in the trait item, which
            // the trait-system key covers.
            FnClass::Template => Some(0),
            FnClass::Plain | FnClass::Stub => None,
        }
    }

    /// Records the outputs of template `t`'s check (fresh or reused)
    /// that its instances replay, as one fingerprint over expression
    /// offsets from the body's first expression.
    pub fn note_template_out(
        &mut self,
        p: &Program,
        t: LocalDefId,
        picks: &[(ExprId, TraitPick)],
        calls: &[(ExprId, ClosureKind)],
        shapes: &[(ExprId, ClosureKind, Vec<CaptureShape>)],
    ) {
        let lo = p.fn_def(t).exprs.0;
        let off = |e: ExprId| e.0.wrapping_sub(lo);
        let mut picks: Vec<(u32, u64, u8, u8, Option<u32>)> = picks
            .iter()
            .map(|(e, k)| {
                let form = match k.form {
                    crate::driver::graph::ReceiverForm::Value => 0u8,
                    crate::driver::graph::ReceiverForm::Ref => 1,
                    crate::driver::graph::ReceiverForm::RefMut => 2,
                };
                (off(*e), k.trait_def.hash.0, k.derefs, form, k.env_pred)
            })
            .collect();
        picks.sort_unstable();
        let mut calls: Vec<(u32, ClosureKind)> = calls.iter().map(|(e, k)| (off(*e), *k)).collect();
        calls.sort_unstable();
        let mut h = FxHasher::default();
        "tpl-out-v1".hash(&mut h);
        picks.hash(&mut h);
        calls.hash(&mut h);
        (shapes.len() as u64).hash(&mut h);
        for (e, kind, caps) in shapes {
            (off(*e), kind).hash(&mut h);
            (caps.len() as u64).hash(&mut h);
            for (root, projs, mode) in caps {
                (root, mode).hash(&mut h);
                (projs.len() as u64).hash(&mut h);
                for pr in projs {
                    match pr {
                        crate::ir::Proj::Deref => 0u8.hash(&mut h),
                        crate::ir::Proj::Field(i) => (1u8, i).hash(&mut h),
                        crate::ir::Proj::Downcast(i) => (2u8, i).hash(&mut h),
                        crate::ir::Proj::Index(l) => (3u8, l).hash(&mut h),
                    }
                }
            }
        }
        self.tpl_out.insert(t, h.finish());
    }

    /// Template `t`'s output fingerprint (`note_template_out`), and its
    /// write back when a header delta undoes a template check.
    pub(crate) fn tpl_out_of(&self, t: LocalDefId) -> Option<u64> {
        self.tpl_out.get(&t).copied()
    }

    pub(crate) fn set_tpl_out(&mut self, t: LocalDefId, out: Option<u64>) {
        match out {
            Some(h) => self.tpl_out.insert(t, h),
            None => self.tpl_out.remove(&t),
        };
    }

    /// Records the closures one discovery check registers, by def (the
    /// first record of a def wins, as in `register_closures`).
    pub fn note_closures(&mut self, closures: &[OwnedClosure]) {
        for oc in closures {
            let mut h = FxHasher::default();
            "closure-out-v1".hash(&mut h);
            oc.kind.hash(&mut h);
            hash_owned_ty(&mut h, &oc.env);
            (oc.inputs.len() as u64).hash(&mut h);
            for t in &oc.inputs {
                hash_owned_ty(&mut h, t);
            }
            hash_owned_ty(&mut h, &oc.output);
            self.closure_out.entry(oc.def).or_insert(h.finish());
        }
    }

    /// The fn whose source text one fn of `p` checks: an AST fn, or a
    /// trait default method template (whose text is in its trait item).
    /// None for a trait method stub.
    pub fn source(&self, p: &Program, f: LocalDefId) -> Option<LocalDefId> {
        let mut f = f;
        // Instances and closure body fns chain to a template or parent
        // made earlier, so the walk ends; the bound only guards it.
        for _ in 0..64 {
            if (f.0 as usize) < self.ast_src.len() {
                return Some(f);
            }
            f = match p.gx.fn_class.get(f.0 as usize)? {
                FnClass::Instance => p.gx.instances.get(&f)?.template_local?,
                FnClass::Closure | FnClass::TemplateClosure => p.gx.closures.get(p.gx.closure_fns.get(&f)?)?.parent,
                FnClass::Template => return Some(f),
                FnClass::Plain | FnClass::Stub => return None,
            };
        }
        None
    }

    /// True when an entry made under `env` with `reads` still holds in
    /// this program: same environment, or every read answers the same.
    /// `entry` names it for the generic reuse oracle (its kind and def).
    fn reads_hold(
        memo: &mut FxMap<ReadKey, Read>,
        this_env: u64,
        p: &Program,
        env: u64,
        reads: &Reads,
        keys: &ReadKeys,
        entry: (&str, LocalDefId),
    ) -> bool {
        if env == this_env {
            // LUST_CHECK_GENREUSE=1: an entry its environment keeps must hold by its reads too.
            if super::genreuse_oracle::on() {
                super::genreuse_oracle::kept_reads(entry.0, p, entry.1, reads, keys, memo);
            }
            return true;
        }
        p.reads_dirty_memo(reads, keys, memo) == 0
    }

    /// The stored picks and closure call kinds of template `t`, rebased
    /// onto its current body, if its stored check still holds.
    #[allow(clippy::type_complexity)]
    pub fn template(&mut self, p: &Program, t: LocalDefId) -> Option<(Vec<(ExprId, TraitPick)>, Vec<(ExprId, ClosureKind)>)> {
        if !self.on {
            return None;
        }
        let src = self.src_of(p, t)?;
        let (lo, hi) = p.fn_def(t).exprs;
        let e = self.templates.get_mut(&p.def_id(t))?;
        if e.src != src
            || e.n_exprs != hi - lo
            || !Self::reads_hold(&mut self.memo, self.env, p, e.env, &e.reads, &self.keys, ("a template entry", t))
        {
            return None;
        }
        e.env = self.env;
        self.templates_reused += 1;
        let picks = e.picks.iter().map(|&(off, pick)| (ExprId(lo + off), pick)).collect();
        let calls = e.calls.iter().map(|&(off, k)| (ExprId(lo + off), k)).collect();
        Some((picks, calls))
    }

    /// Stores one passing template check (the caller passes only bodies
    /// with no closure expressions).
    pub fn store_template(&mut self, p: &Program, t: LocalDefId, reads: &Reads, picks: &[(ExprId, TraitPick)], calls: &[(ExprId, ClosureKind)]) {
        if !self.on {
            return;
        }
        let Some(src) = self.src_of(p, t) else { return };
        let (lo, hi) = p.fn_def(t).exprs;
        let inside = |e: ExprId| e.0 >= lo && e.0 < hi;
        if !picks.iter().all(|(e, _)| inside(*e)) || !calls.iter().all(|(e, _)| inside(*e)) {
            return;
        }
        let picks = picks.iter().map(|&(e, pick)| (e.0 - lo, pick)).collect();
        let calls = calls.iter().map(|&(e, k)| (e.0 - lo, k)).collect();
        let entry = TemplateEntry { src, env: self.env, stamp: next_stamp(), reads: with_own_sig(p, t, reads, &mut self.keys), n_exprs: hi - lo, picks, calls };
        self.index_reads(p, p.def_id(t), &entry.reads);
        self.templates.insert(p.def_id(t), entry);
    }

    /// Stores the reads of one check of template `t`, which has closures
    /// (`reads`), or drops its entry when the check had errors (None):
    /// an entry is always that of the template's last check.
    pub(crate) fn store_closure_template(&mut self, p: &Program, t: LocalDefId, reads: Option<&Reads>) {
        let def = p.def_id(t);
        let src = self.src_of(p, t).filter(|_| self.on);
        let (Some(reads), Some(src)) = (reads, src) else {
            self.closure_templates.remove(&def);
            return;
        };
        let (lo, hi) = p.fn_def(t).exprs;
        let entry = ClosureTemplateEntry { src, env: self.env, stamp: next_stamp(), reads: with_own_sig(p, t, reads, &mut self.keys), n_exprs: hi - lo, out: None };
        self.closure_templates.insert(def, entry);
    }

    /// Adds to the entry `store_closure_template` just made for template
    /// `t` what a replay of its clean check needs: picks, closure call
    /// kinds, capture shapes, and the closures' records.
    pub(crate) fn store_closure_template_out(
        &mut self,
        p: &Program,
        t: LocalDefId,
        picks: &[(ExprId, TraitPick)],
        calls: &[(ExprId, ClosureKind)],
        shapes: &crate::check::generics::ClosureShapes,
        closures: &[OwnedClosure],
    ) {
        let (lo, hi) = p.fn_def(t).exprs;
        let inside = |e: ExprId| e.0 >= lo && e.0 < hi;
        let Some(e) = self.closure_templates.get_mut(&p.def_id(t)) else { return };
        if !picks.iter().all(|(x, _)| inside(*x)) || !calls.iter().all(|(x, _)| inside(*x)) || !shapes.iter().all(|(x, _, _)| inside(*x)) {
            return;
        }
        e.out = Some(ClosureTemplateOut {
            picks: picks.iter().map(|&(x, pick)| (x.0 - lo, pick)).collect(),
            calls: calls.iter().map(|&(x, k)| (x.0 - lo, k)).collect(),
            shapes: shapes.iter().map(|(x, k, s)| (x.0 - lo, *k, s.clone())).collect(),
            closures: closures.to_vec(),
        });
    }

    /// The stored output of template `t`, which has closures, rebased onto
    /// its current body, if its stored check still holds: as `template`,
    /// plus no `Checked` read (a collect checks such a body again).
    #[allow(clippy::type_complexity)]
    pub(crate) fn closure_template(
        &mut self,
        p: &Program,
        t: LocalDefId,
    ) -> Option<(Vec<(ExprId, TraitPick)>, Vec<(ExprId, ClosureKind)>, crate::check::generics::ClosureShapes, Vec<OwnedClosure>)> {
        if !self.on {
            return None;
        }
        let src = self.src_of(p, t)?;
        let (lo, hi) = p.fn_def(t).exprs;
        let e = self.closure_templates.get_mut(&p.def_id(t))?;
        let out = e.out.as_ref()?;
        if e.src != src || e.n_exprs != hi - lo || e.reads.keys(&self.keys).any(|k| matches!(k, ReadKey::Checked(_))) {
            return None;
        }
        if !Self::reads_hold(&mut self.memo, self.env, p, e.env, &e.reads, &self.keys, ("a closure template entry", t)) {
            return None;
        }
        e.env = self.env;
        self.templates_reused += 1;
        let picks = out.picks.iter().map(|&(off, pick)| (ExprId(lo + off), pick)).collect();
        let calls = out.calls.iter().map(|&(off, k)| (ExprId(lo + off), k)).collect();
        let shapes = out.shapes.iter().map(|(off, k, s)| (ExprId(lo + off), *k, s.clone())).collect();
        Some((picks, calls, shapes, out.closures.clone()))
    }

    /// Whether the stored check of template `t`, which has closures, still
    /// holds. With `moves` (a derive-only request) only the read keys it
    /// takes are compared, else every read. None when there is no entry the
    /// caller can trust: none, one stored after `lo` (the kept program's
    /// `reuse_stamp`: the other mode's program may have stored it), or one
    /// with a `Checked` read (a collect checks such a body again each time).
    pub(crate) fn closure_template_holds(
        &mut self,
        p: &Program,
        t: LocalDefId,
        lo: u64,
        moves: Option<&mut dyn FnMut(&ReadKey) -> bool>,
    ) -> Option<bool> {
        let e = self.closure_templates.get(&p.def_id(t))?;
        let src = self.src_of(p, t)?;
        let (a, b) = p.fn_def(t).exprs;
        if !self.on || e.stamp > lo || e.reads.keys(&self.keys).any(|k| matches!(k, ReadKey::Checked(_))) {
            return None;
        }
        if e.src != src || e.n_exprs != b - a {
            return Some(false);
        }
        let memo = &mut self.memo;
        let mut same = |k: &ReadKey, stored: &Read| *memo.entry(*k).or_insert_with(|| p.read_fingerprint(k)) == *stored;
        Some(match moves {
            Some(moves) => e.reads.iter(&self.keys).all(|(k, stored)| !moves(k) || same(k, &stored)),
            None => e.reads.iter(&self.keys).all(|(k, stored)| same(k, &stored)),
        })
    }

    /// Drops this program's node answers: a signature delta changed the
    /// program they describe (driver/kept_delta.rs).
    pub fn forget_answers(&mut self) {
        self.memo.clear();
    }

    /// Whether template `t`'s stored check still holds; None when none
    /// is stored (`template` without the copy).
    pub fn template_holds(&mut self, p: &Program, t: LocalDefId) -> Option<bool> {
        let src = self.src_of(p, t)?;
        let (lo, hi) = p.fn_def(t).exprs;
        let e = self.templates.get(&p.def_id(t))?;
        Some(
            self.on
                && e.src == src
                && e.n_exprs == hi - lo
                && Self::reads_hold(&mut self.memo, self.env, p, e.env, &e.reads, &self.keys, ("a template entry", t)),
        )
    }

    /// The read keys of every stored template check, those of templates
    /// with closures included (the derive delta
    /// fingerprints the moved ones at once before its replay).
    pub(crate) fn template_read_keys(&self) -> impl Iterator<Item = &ReadKey> {
        let closures = self.closure_templates.values().flat_map(|e| e.reads.keys(&self.keys));
        self.templates.values().flat_map(|e| e.reads.keys(&self.keys)).chain(closures)
    }

    /// True when template `t` has a stored plain check (a log line).
    pub(crate) fn has_template_entry(&self, p: &Program, t: LocalDefId) -> bool {
        self.templates.contains_key(&p.def_id(t))
    }

    /// `template_holds` when only the read keys `moves` takes can have
    /// moved since the stored check (a derive-only request, sigedit P3.2,
    /// and a header delta, driver/kept_delta.rs): the other reads are not
    /// compared.
    pub fn template_holds_moved(&mut self, p: &Program, t: LocalDefId, moves: &mut dyn FnMut(&ReadKey) -> bool) -> Option<bool> {
        let src = self.src_of(p, t)?;
        let (lo, hi) = p.fn_def(t).exprs;
        let e = self.templates.get(&p.def_id(t))?;
        let memo = &mut self.memo;
        let clean = e.reads.iter(&self.keys).all(|(k, stored)| !moves(k) || *memo.entry(*k).or_insert_with(|| p.read_fingerprint(k)) == stored);
        Some(self.on && e.src == src && e.n_exprs == hi - lo && clean)
    }

    /// True when a template check or discovery output of fn `f` was
    /// stored after stamp `lo` and no later than `hi`. The check and build
    /// programs share these entries: one stored after a program's
    /// `Program::reuse_stamp` (and before the work that asks, `hi`) may
    /// hold an output that program does not have (the kept program delta
    /// refuses then, driver/kept_delta.rs).
    pub(crate) fn newer(&self, p: &Program, f: LocalDefId, lo: u64, hi: u64) -> bool {
        let def = p.def_id(f);
        let after = |s: u64| s > lo && s <= hi;
        self.templates.get(&def).is_some_and(|e| after(e.stamp))
            || self.discovery.get(&def).is_some_and(|e| after(e.stamp))
            || self.unstored.get(&def).is_some_and(|e| after(e.0))
    }

    /// True when fn `f`'s stored template check or discovery output was
    /// made from its current source and read no node that `touched`
    /// names: then, when it held for the program before an edit that
    /// moves only the touched nodes, it still holds (the header delta's
    /// fast path, driver/kept_delta.rs). False with no entry or a moved
    /// source.
    pub(crate) fn untouched(&self, p: &Program, f: LocalDefId, touched: &mut impl FnMut(&ReadKey) -> bool) -> bool {
        let Some(src) = self.src_of(p, f).filter(|_| self.on) else { return false };
        let def = p.def_id(f);
        if self.unstored.contains_key(&def) {
            return false;
        }
        let (lo, hi) = p.fn_def(f).exprs;
        let reads = match (self.templates.get(&def), self.discovery.get(&def)) {
            (Some(e), _) if e.src == src && e.n_exprs == hi - lo => &e.reads,
            (None, Some(e)) if e.src == src => &e.reads,
            _ => return false,
        };
        !reads.iter(&self.keys).any(|(k, _)| touched(k))
    }

    /// `untouched` for a header delta, through the reverse index: `readers`
    /// is `GenReuse::readers` of the edited fns (no read scan per entry).
    pub(crate) fn untouched_by(&self, p: &Program, f: LocalDefId, readers: &FxMap<DefId, ()>) -> bool {
        let Some(src) = self.src_of(p, f).filter(|_| self.on) else { return false };
        let def = p.def_id(f);
        if self.unstored.contains_key(&def) || readers.contains_key(&def) {
            return false;
        }
        let (lo, hi) = p.fn_def(f).exprs;
        match (self.templates.get(&def), self.discovery.get(&def)) {
            (Some(e), _) => e.src == src && e.n_exprs == hi - lo,
            (None, Some(e)) => e.src == src,
            _ => false,
        }
    }

    /// The defs whose stored entry may read a node a header delta of the
    /// fns `defs` moves: their signatures, the bindings that resolve to
    /// them, the method sets `methods` (Adt def, name) that hold one, the
    /// impl buckets of `traits` (an edited trait impl method's trait), and
    /// every `Checked` read (`untouched_by` takes the set).
    pub(crate) fn readers(&self, defs: &[DefId], methods: &[(DefId, crate::intern::Sym)], traits: &[DefId]) -> FxMap<DefId, ()> {
        let keys = defs.iter().flat_map(|&d| [ReaderKey::Sig(d), ReaderKey::Def(d)]).chain(methods.iter().map(|&(a, n)| ReaderKey::Method(a, n)));
        let keys = keys.chain(traits.iter().map(|&t| ReaderKey::Trait(t)));
        keys.chain([ReaderKey::Checked]).filter_map(|k| self.readers_index.get(&k)).flatten().map(|&d| (d, ())).collect()
    }

    /// Adds entry `def`'s reads to `readers_index`. A binding indexes by
    /// the def it resolves to now: an entry no newer than a kept program
    /// held for it, so its bindings resolve as when it was stored. Rebuilt
    /// from the live entries once stale keys outnumber them, before the new
    /// keys go in (the caller stores the entry after this).
    fn index_reads(&mut self, p: &Program, def: DefId, reads: &Reads) {
        let push = |index: &mut FxMap<ReaderKey, Vec<DefId>>, keys: &ReadKeys, def: DefId, reads: &Reads| {
            let mut n = 0;
            for (k, _) in reads.iter(keys) {
                let mut add = |key| {
                    index.entry(key).or_default().push(def);
                    n += 1;
                };
                match k {
                    ReadKey::Sig(d) => add(ReaderKey::Sig(*d)),
                    // Only an Adt head has candidates (`method_candidates`).
                    ReadKey::MethodSet(m) => {
                        if let crate::driver::graph::Head::Adt(a) = m.head {
                            add(ReaderKey::Method(a, m.name));
                        }
                    }
                    ReadKey::Binding(b) => {
                        if let crate::driver::graph::BindingRes::Def(d) = p.binding_res_at(b) {
                            add(ReaderKey::Def(d));
                        }
                    }
                    ReadKey::Checked(_) => add(ReaderKey::Checked),
                    ReadKey::ImplSet(k) => add(ReaderKey::Trait(k.trait_)),
                    ReadKey::AssocVis(a, name) => add(ReaderKey::Method(*a, *name)),
                    ReadKey::Layout(_) | ReadKey::DropCk(_) | ReadKey::Structure(_) | ReadKey::Answer(..) | ReadKey::TraitInScope(..) | ReadKey::StdImport(..) | ReadKey::Derives(_) | ReadKey::DeriveBits(..) | ReadKey::LangImpls(_) => {}
                }
            }
            n
        };
        let live = self.templates.len() + self.discovery.len();
        if self.readers_pushed > 64 * live.max(64) {
            let mut index = FxMap::default();
            let entries = self.templates.iter().map(|(d, e)| (*d, &e.reads)).chain(self.discovery.iter().map(|(d, e)| (*d, &e.reads)));
            let pushed: usize = entries.map(|(d, r)| push(&mut index, &self.keys, d, r)).sum();
            (self.readers_index, self.readers_pushed) = (index, pushed);
        }
        self.readers_pushed += push(&mut self.readers_index, &self.keys, def, reads);
    }

    /// True when fn `f` has a stored discovery output made from its current
    /// source and its last check stored it (its reads are not compared:
    /// the derive delta's keyed walk proved them).
    pub(crate) fn discovery_src_same(&self, p: &Program, f: LocalDefId) -> bool {
        let def = p.def_id(f);
        self.on && !self.unstored.contains_key(&def) && self.discovery.get(&def).is_some_and(|e| self.src_of(p, f) == Some(e.src))
    }

    /// True when fn `f`'s stored discovery output is the one its last check
    /// made, from its current source, and each of its reads that `moves`
    /// names keeps its fingerprint. For an entry that held for the program
    /// before an edit that moves only those nodes (the kept program deltas,
    /// driver/kept_delta.rs), it then still holds, as `pending_holds_moved`
    /// and `untouched` reason.
    pub(crate) fn discovery_holds_moved(&mut self, p: &Program, f: LocalDefId, moves: &mut impl FnMut(&ReadKey) -> bool) -> bool {
        let def = p.def_id(f);
        if !self.on || self.unstored.contains_key(&def) {
            return false;
        }
        let (Some(src), Some(e)) = (self.src_of(p, f), self.discovery.get(&def)) else { return false };
        let memo = &mut self.memo;
        e.src == src && e.reads.iter(&self.keys).all(|(k, stored)| !moves(k) || *memo.entry(*k).or_insert_with(|| p.read_fingerprint(k)) == stored)
    }

    /// The stored discovery output of fn `f`, if it still holds. Not when
    /// a later check of `f` stored nothing (`unstored`, an error or a
    /// pending closure map): the caller checks again. A replay would leave
    /// that note, which `out_copy` returns, so every later kept-program
    /// delta would compare the body with the note's output and refuse.
    pub fn discovered(&mut self, p: &Program, f: LocalDefId) -> Option<Discovered> {
        if !self.on || self.unstored.contains_key(&p.def_id(f)) {
            return None;
        }
        let src = self.src_of(p, f)?;
        let e = self.discovery.get_mut(&p.def_id(f))?;
        if e.src != src || !Self::reads_hold(&mut self.memo, self.env, p, e.env, &e.reads, &self.keys, ("a discovery entry", f)) {
            return None;
        }
        let mut requests = Vec::with_capacity(e.requests.len());
        for (def, args) in &e.requests {
            let l = p.local_def(def.hash).filter(|&l| p.def_id(l) == *def)?;
            requests.push((l, args.clone()));
        }
        e.env = self.env;
        self.discovery_reused += 1;
        Some((e.adt_tys.clone(), requests, e.closures.clone(), e.vtables.clone()))
    }

    /// True when a stored check read the body of one of `defs`: a
    /// `Checked` read, which a check records for each const fn its const
    /// evaluation ran. The driver's const fn edit asks (compile.rs).
    pub(crate) fn reads_checked(&self, defs: &[DefId]) -> bool {
        let stored = self.templates.values().map(|e| &e.reads).chain(self.discovery.values().map(|e| &e.reads));
        stored.into_iter().any(|r| defs.iter().any(|&d| r.contains(&self.keys, &ReadKey::Checked(d))))
    }

    /// Judges the stored discovery entries of one materialize round (`fns`)
    /// on the pool, before its turns: their reads, and a copy of each
    /// output. A key's answer goes into `memo` now, where `discovered` puts
    /// it at the entry's turn: the same answer, since mono adds only rows no
    /// walk of an existing def reaches (`FpMemo`). Two kinds of answer can
    /// still move while mono runs and stay for the turn, asked in stored
    /// order as `discovered` asks them (`discovered_prejudged`): `Answer`
    /// keys (auto traits of the instances made), and keys whose answer read
    /// an opaque definer's hidden type (`read_fingerprint_stable`). Empty
    /// when generic reuse is off.
    pub(crate) fn prejudge(&mut self, p: &Program, fns: &[LocalDefId]) -> Vec<Option<Prejudged>> {
        use crate::pool::par_map;
        if !self.on {
            return Vec::new();
        }
        let entry = |f: LocalDefId| {
            let def = p.def_id(f);
            if self.unstored.contains_key(&def) { None } else { self.discovery.get(&def) }
        };
        let judged = |e: &DiscoveryEntry| e.env != self.env;
        // The keys no earlier ask answered, by shard of their hash, then
        // each shard's distinct keys answered on the pool (the stable ones
        // go into `memo`).
        const SHARDS: usize = 64;
        let shard = |k: &ReadKey| {
            let mut h = FxHasher::default();
            k.hash(&mut h);
            (h.finish() >> 32) as usize % SHARDS
        };
        let chunks: Vec<&[LocalDefId]> = fns.chunks(256).collect();
        let missing: Vec<Vec<Vec<ReadKey>>> = par_map(&chunks, |c| {
            let mut out = vec![Vec::new(); SHARDS];
            for e in c.iter().filter_map(|&f| entry(f)).filter(|e| judged(e)) {
                for (k, _) in e.reads.iter(&self.keys) {
                    if !matches!(k, ReadKey::Answer(..)) && !self.memo.contains_key(k) {
                        out[shard(k)].push(*k);
                    }
                }
            }
            out
        });
        let shards: Vec<usize> = (0..SHARDS).collect();
        let found: Vec<Vec<(ReadKey, Read)>> = par_map(&shards, |&s| {
            let mut seen: FxSet<ReadKey> = FxSet::default();
            let mut out = Vec::new();
            for k in missing.iter().flat_map(|m| &m[s]) {
                if seen.insert(*k) {
                    let (read, stable) = p.read_fingerprint_stable(k);
                    if stable {
                        out.push((*k, read));
                    }
                }
            }
            out
        });
        self.memo.reserve(found.iter().map(Vec::len).sum());
        self.memo.extend(found.into_iter().flatten());
        let memo = &self.memo;
        par_map(fns, |&f| {
            let e = entry(f)?;
            let (held, answers) = if judged(e) {
                let mut held = true;
                let mut answers = Vec::new();
                // A key with no answer in `memo` now is one of the two kinds
                // that wait for the turn.
                for (k, stored) in e.reads.iter(&self.keys) {
                    match memo.get(k) {
                        Some(&now) if !matches!(k, ReadKey::Answer(..)) => held &= now == stored,
                        _ => answers.push((*k, stored)),
                    }
                }
                (held, answers)
            } else {
                (true, Vec::new())
            };
            let out = held.then(|| (e.adt_tys.clone(), e.requests.clone(), e.closures.clone(), e.vtables.clone()));
            Some(Prejudged { stamp: e.stamp, held, answers, out })
        })
    }

    /// `discovered` for a round entry `prejudge` judged (`pre`): the same
    /// verdict and the same `memo` answers, as of this turn. An entry stored
    /// since (a newer stamp) is judged here, as `discovered` does.
    pub fn discovered_prejudged(&mut self, p: &Program, f: LocalDefId, pre: Option<Prejudged>) -> Option<Discovered> {
        let def = p.def_id(f);
        if !self.on || self.unstored.contains_key(&def) {
            return None;
        }
        let Some(pre) = pre.filter(|pre| self.discovery.get(&def).is_some_and(|e| e.stamp == pre.stamp)) else {
            return self.discovered(p, f);
        };
        let src = self.src_of(p, f)?;
        let e = self.discovery.get_mut(&def)?;
        if e.src != src {
            return None;
        }
        if prejudge_check() {
            prejudge_oracle(p, &self.memo, &self.keys, e, self.env, &pre);
        }
        // As `reads_dirty_memo`: every key is asked (no short cut), so each
        // `Answer` key's first answer comes at this turn, as before.
        let mut held = pre.held;
        for (k, stored) in &pre.answers {
            if *self.memo.entry(*k).or_insert_with(|| p.read_fingerprint(k)) != *stored {
                held = false;
            }
        }
        if !held {
            return None;
        }
        // LUST_CHECK_GENREUSE=1: an entry its environment keeps must hold by its reads
        // too, as in `reads_hold`.
        if e.env == self.env && super::genreuse_oracle::on() {
            super::genreuse_oracle::kept_reads("a discovery entry", p, f, &e.reads, &self.keys, &mut self.memo);
        }
        let (adt_tys, reqs, closures, vtables) = pre.out?;
        let mut requests = Vec::with_capacity(reqs.len());
        for (def, args) in reqs {
            let l = p.local_def(def.hash).filter(|&l| p.def_id(l) == def)?;
            requests.push((l, args));
        }
        e.env = self.env;
        self.discovery_reused += 1;
        Some((adt_tys, requests, closures, vtables))
    }

    /// A copy of the last discovery output of the fn with def `def`, the
    /// stored one or that of a check with a body error: what collect used
    /// for that body. The daemon's kept programs take one per edited fn
    /// at the patch (driver/patch.rs).
    pub(crate) fn out_copy(&self, def: DefId) -> Option<OutCopy> {
        if let Some((_, c, _)) = self.unstored.get(&def) {
            return Some(c.clone());
        }
        let e = self.discovery.get(&def)?;
        Some(OutCopy { adt_tys: e.adt_tys.clone(), requests: e.requests.clone(), closures: e.closures.clone(), vtables: e.vtables.clone() })
    }

    /// Stores the output of one clean discovery check of fn `f`.
    #[allow(clippy::too_many_arguments)]
    pub fn store_discovered(
        &mut self,
        p: &Program,
        f: LocalDefId,
        reads: &Reads,
        adt_tys: &[Arc<OwnedTy>],
        requests: &[(LocalDefId, Arc<[OwnedArg]>)],
        closures: &[OwnedClosure],
        vtables: &[(OwnedTy, OwnedTy)],
    ) {
        if !self.on {
            return;
        }
        let Some(src) = self.src_of(p, f) else { return };
        self.keep_held(p.def_id(f));
        let requests = requests.iter().map(|(t, args)| (p.def_id(*t), share(&mut self.shared_args, args, |a| Arc::from(a)))).collect();
        let reads = with_own_sig(p, f, reads, &mut self.keys);
        let entry = DiscoveryEntry {
            src,
            env: self.env,
            stamp: next_stamp(),
            reads,
            adt_tys: adt_tys.iter().map(|t| share(&mut self.shared, t, |t| Arc::new(t.clone()))).collect(),
            requests,
            closures: closures.to_vec(),
            vtables: vtables.to_vec(),
        };
        self.unstored.remove(&p.def_id(f));
        self.index_reads(p, p.def_id(f), &entry.reads);
        self.discovery.insert(p.def_id(f), entry);
        if self.shared.len() + self.shared_args.len() >= self.prune_at {
            self.shared.retain(|t| Arc::strong_count(t) > 1);
            self.shared_args.retain(|a| Arc::strong_count(a) > 1);
            self.prune_at = (2 * (self.shared.len() + self.shared_args.len())).max(1 << 12);
        }
    }
}

/// `t` as the value that every discovery entry holding it shares (`GenReuse::shared`,
/// `GenReuse::shared_args`); `own` makes the first copy. A copy, not the caller's `Arc`: kept,
/// a discovery check's own values made the tsgo front image 27 MB larger (live bytes).
fn share<T: Hash + Eq + ?Sized>(shared: &mut FxSet<Arc<T>>, t: &T, own: impl FnOnce(&T) -> Arc<T>) -> Arc<T> {
    if let Some(s) = shared.get(t) {
        return s.clone();
    }
    let s = own(t);
    shared.insert(s.clone());
    s
}

/// The stamp of the next stored entry. One counter for the whole
/// process, so stamps only rise, also across a fresh `GenReuse`.
fn next_stamp() -> u64 {
    STAMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

/// The stamp of the last stored entry: a program whose template checks
/// and discovery outputs agree with every entry stored so far takes it
/// as its `Program::reuse_stamp`.
pub(crate) fn stamp_now() -> u64 {
    STAMP.load(std::sync::atomic::Ordering::Relaxed)
}

static STAMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Moves the stamp counter up to `v` (a front image's value, src/image.rs),
/// so stamps stored in a restored session stay older than new ones.
pub(crate) fn stamp_restore(v: u64) {
    STAMP.fetch_max(v, std::sync::atomic::Ordering::Relaxed);
}

/// The two keys `GenReuse::gate` compares across collects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraitKey {
    /// The trait-system key (see `trait_key`).
    pub system: u64,
    /// The derive list of each Adt that has one.
    pub derives: u64,
    /// The written std and lang trait impls that name a user Adt, which
    /// `system` leaves out (`trait_key`).
    pub lang: u64,
}

/// The trait-system key (see the module docs): trait items (default
/// bodies included) and impl items (headers, method heads, associated
/// items), as written and resolved (every trait record, and each kept
/// impl's record: an alias or const a header or a bound names resolves
/// in them); and, apart, the derive-list key. Type alias and const items
/// stay out: their readers record `Sig` reads that fold the resolved
/// target or the value. An
/// impl of a trait whose impls `ImplSet` reads cover
/// (`Program::impls_read_covered`; `impl_traits` gives each trait impl's
/// trait by item index) is left out, and so is an inherent impl with no
/// associated const or type and no `dyn` Self: method probes and every
/// direct inherent table lookup record `MethodSet` reads
/// (`Program::inherent_reads`), whose fingerprints cover each method's
/// signature. For the same reason an inherent impl with associated items
/// and no `dyn` Self joins without its methods (the header delta,
/// driver/kept_delta.rs, keeps the key through their head edits).
///
/// A positive written impl of a std or lang trait whose trait ref names a
/// user Adt (`impl_traits` flags it, `Program::lang_keyed_impl`) is left
/// out too: every body whose types name that Adt records a `LangImpls`
/// read of its impls (driver/graph.rs), the index notes the same read
/// when it hands the impl out, and its `Drop` and auto trait facts fold
/// into the Adt's deep layout. Those impls make the `lang` key, which
/// only refreshes glue (compile.rs).
pub fn trait_key(p: &Program, impl_traits: &FxMap<usize, (DefId, bool, DefId)>) -> TraitKey {
    use crate::ast::{ItemKind, TyExpr};
    let user_trait = |d: &DefId| p.impls_read_covered(*d);
    let plain_inherent = |imp: &crate::ast::ImplDef| {
        imp.trait_ref.is_none() && imp.consts.is_empty() && imp.types.is_empty() && !matches!(p.ast.ty(imp.self_ty), TyExpr::Dyn { .. })
    };
    let mut h = FxHasher::default();
    "trait-system-v3".hash(&mut h);
    // The crate's `#![recursion_limit]`: every body check reads it with no
    // recorded read (check/where_overflow.rs `recursion_limit`), so a change
    // checks every body again.
    crate::check::where_overflow::recursion_limit(p).hash(&mut h);
    let mut dh = FxHasher::default();
    "derives-v1".hash(&mut dh);
    let mut lh = FxHasher::default();
    "lang-impls-v1".hash(&mut lh);
    for (i, item) in p.ast.items.iter().enumerate() {
        let derives = match &item.kind {
            ItemKind::Impl(imp) if imp.trait_ref.is_some() && impl_traits.get(&i).is_some_and(|(d, _, _)| user_trait(d)) => continue,
            ItemKind::Impl(imp) if !imp.is_negative && impl_traits.get(&i).is_some_and(|&(_, keyed, _)| keyed) => {
                crate::driver::ast_hash::item_hash(&p.ast, item).hash(&mut lh);
                continue;
            }
            ItemKind::Impl(imp) if plain_inherent(imp) => continue,
            ItemKind::Impl(imp) if imp.trait_ref.is_none() && !matches!(p.ast.ty(imp.self_ty), TyExpr::Dyn { .. }) => {
                let bare = crate::ast::Item { kind: ItemKind::Impl(crate::ast::ImplDef { fns: Vec::new(), ..imp.clone() }), ..item.clone() };
                (1u8, crate::driver::ast_hash::item_hash(&p.ast, &bare)).hash(&mut h);
                continue;
            }
            // Type aliases and consts stay out: every reader of one records
            // a `Sig` read that folds its resolved target or value, and the
            // trait records and the impls kept here fold in resolved
            // (below), so one that moves a header or a bound moves the key.
            ItemKind::Type(_) | ItemKind::Const(_) => continue,
            ItemKind::Trait(_) | ItemKind::Impl(_) => {
                (0u8, crate::driver::ast_hash::item_hash(&p.ast, item)).hash(&mut h);
                if let Some(&(_, _, def)) = impl_traits.get(&i) {
                    crate::driver::graph::resolved_impl_hash(p, def).hash(&mut h);
                }
                continue;
            }
            ItemKind::Struct(s) => (s.name, &s.derives),
            ItemKind::Enum(e) => (e.name, &e.derives),
            _ => continue,
        };
        // An Adt with no derives adds no impl: a new plain struct
        // keeps the key.
        if derives.1.is_empty() {
            continue;
        }
        p.interner.get(derives.0).hash(&mut dh);
        (derives.1.len() as u64).hash(&mut dh);
        for (d, _) in derives.1 {
            p.interner.get(*d).hash(&mut dh);
        }
    }
    crate::driver::graph::resolved_traits_hash(p).hash(&mut h);
    TraitKey { system: h.finish(), derives: dh.finish(), lang: lh.finish() }
}

/// A check's read set plus a `Sig` read of the checked fn itself: a body
/// reads its own signature (parameter and return types, deep) through
/// the compat `FnInfo` path, which records nothing, and its field
/// accesses record nothing either. The driver adds the same read to
/// each slot's stored set. Frozen: every caller stores it.
fn with_own_sig(p: &Program, f: LocalDefId, reads: &Reads, keys: &mut ReadKeys) -> Reads {
    let mut reads = reads.clone();
    let _ = p.sig_of(&mut reads, p.def_id(f));
    reads.freeze(keys);
    reads
}

/// Hashes one owned type snapshot by its stable content: defs, kinds,
/// regions, and consts; a fn pointer's session-local signature ids are
/// skipped (its inputs and output are hashed from the snapshot).
fn hash_owned_ty(h: &mut FxHasher, t: &OwnedTy) {
    match t {
        OwnedTy::Leaf(k) => (0u8, k).hash(h),
        OwnedTy::Ref(r, m, e) => {
            (1u8, r, m).hash(h);
            hash_owned_ty(h, e);
        }
        OwnedTy::RawPtr(m, e) => {
            (2u8, m).hash(h);
            hash_owned_ty(h, e);
        }
        OwnedTy::Slice(e) => {
            3u8.hash(h);
            hash_owned_ty(h, e);
        }
        OwnedTy::Array(e, n) => {
            (4u8, n).hash(h);
            hash_owned_ty(h, e);
        }
        OwnedTy::Tuple(es) => {
            (5u8, es.len() as u64).hash(h);
            for e in es {
                hash_owned_ty(h, e);
            }
        }
        OwnedTy::Adt(d, a) | OwnedTy::FnDef(d, a) | OwnedTy::Closure(d, a) | OwnedTy::Opaque(d, a) => {
            let tag = match t {
                OwnedTy::Adt(..) => 6u8,
                OwnedTy::FnDef(..) => 7,
                OwnedTy::Closure(..) => 8,
                _ => 9,
            };
            (tag, d).hash(h);
            hash_owned_args(h, a);
        }
        OwnedTy::Projection(d, a) => {
            (10u8, d.0).hash(h);
            hash_owned_args(h, a);
        }
        OwnedTy::FnPtr(b, inputs, output) => {
            (11u8, b.count, b.value.is_unsafe, b.value.extern_c, inputs.len() as u64).hash(h);
            for e in inputs {
                hash_owned_ty(h, e);
            }
            hash_owned_ty(h, output);
        }
        OwnedTy::Dyn(principal, auto, r) => {
            (12u8, auto, r).hash(h);
            match principal {
                Some((d, a, bs)) => {
                    (1u8, d).hash(h);
                    hash_owned_args(h, a);
                    (bs.len() as u64).hash(h);
                    for (bd, g) in bs {
                        bd.hash(h);
                        hash_owned_args(h, std::slice::from_ref(g));
                    }
                }
                None => 0u8.hash(h),
            }
        }
    }
}

fn hash_owned_args(h: &mut FxHasher, args: &[OwnedArg]) {
    (args.len() as u64).hash(h);
    for a in args {
        match a {
            OwnedArg::Ty(t) => {
                0u8.hash(h);
                hash_owned_ty(h, t);
            }
            OwnedArg::Lifetime(r) => (1u8, r).hash(h),
            OwnedArg::Const(c) => (2u8, c).hash(h),
        }
    }
}
