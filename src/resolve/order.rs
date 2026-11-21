// The glob order rule (a lust subset rule, L0509).
//
// rustc 1.99 resolves glob imports in its own event order. A module keeps
// the first glob binding of a name it gets; a later glob of the same item
// with a wider visibility raises the binding's visibility (rustc's
// `ambiguity_vis_max`), and a narrower one is noted as its minimum. But
// when the first glob's source changes its binding later (its own
// visibility grew, or it got a re-fetched binding), rustc re-fetches it
// through that glob and replaces the module's binding, which drops the
// wider visibility other globs brought. Which item an ambiguity carries on
// is order-dependent too. lust does not copy rustc's order. After the
// import fixpoint, this pass computes for each glob-bound name every
// binding it may hold in some order (`OrderSet`). It proves the final
// binding the same in every order where it can (`OrderSet::fixed`); a read
// whose answer may differ between orders is rejected
// (`ResolvedTables::order_dependent`, L0509). The program may be valid
// Rust, but lust cannot be sure of rustc's answer there, and a reject is
// always safe. An explicit import of the name fixes it.
use super::imports::{AmbEntry, AmbItem, BindState, Import, ImportKind, PathMode, PathRes, ResolvedTables, Target, VisKind, ns_index};
use crate::diag::{Diag, Span};
use crate::driver::graph::Namespace;
use crate::intern::{FxMap, Interner, Sym};
use crate::ty::defs::ModuleId;

/// What one glob-bound name may hold in rustc's event orders. Stored
/// only for names where that is not one binding at all times
/// (`ResolvedTables::order_dep`); every other glob name ends as lust's
/// table says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrderSet {
    /// Every binding the name may hold at some time: visibility, the
    /// module that defines privacy for it, and the item
    /// (`AmbItem::Amb` for an ambiguity).
    pub all: Vec<AmbEntry>,
    /// The bindings it may end with; one entry when `fixed`.
    pub ends: Vec<AmbEntry>,
    /// The name may end with no binding.
    pub absent: bool,
    /// The final binding is the same in every order (`ends[0]`).
    pub fixed: bool,
    /// The first step's guess at `absent`, before `settle` (a source not
    /// settled yet counts as present when it has a binding this module
    /// can see).
    pub may_absent: bool,
    /// `settle` has set `fixed`, `ends`, and `absent`.
    pub settled: bool,
}

const NAMESPACES: [Namespace; 3] = [Namespace::Type, Namespace::Value, Namespace::Macro];

type Key = (ModuleId, u8, Sym);

/// The glob imports of the crate session after the fixpoint, as the
/// order pass reads them.
struct Globs<'a> {
    /// Importing module -> (source module, glob import).
    from: FxMap<ModuleId, Vec<(ModuleId, &'a Import)>>,
    /// Source module -> the modules that glob it.
    importers: FxMap<ModuleId, Vec<ModuleId>>,
    /// Names a glob of an enum binds, with what each brings: their
    /// answer never depends on order.
    fixed: FxMap<Key, Vec<AmbEntry>>,
}

impl ResolvedTables {
    /// True when a read of `name` in `module` from module `from` has an
    /// answer that may depend on rustc's glob order (`order_dep`): the
    /// final binding is not fixed, `from` sees some possible end, and the
    /// ends differ in item, in whether `from` sees them, or the name may
    /// be ambiguous or absent. When `from` sees none of them, every order
    /// gives a private or unresolved name, which the plain lookup reports.
    pub fn order_dependent(&self, module: ModuleId, ns: Namespace, name: Sym, from: ModuleId, interner: &Interner) -> bool {
        if self.order_dep.is_empty() {
            return false;
        }
        let Some(set) = self.order_dep.get(&(module, ns_index(ns) as u8, name)) else { return false };
        if set.fixed && !set.absent {
            return false;
        }
        let seen = set.ends.iter().filter(|(v, m, _)| self.visible(v, *m, from, interner)).count();
        seen > 0
            && (set.absent
                || seen < set.ends.len()
                || set.ends.iter().any(|(_, _, item)| matches!(item, AmbItem::Amb { .. }) || *item != set.ends[0].2))
    }

    /// True when import `imp` reads `name` in `module` (`ns`), and rustc's
    /// widest and narrowest glob bindings of the name give the import
    /// different visibilities: rustc reports that (the deny-by-default
    /// `ambiguous_glob_imports`), and which bindings it holds depends on
    /// order. A private import never differs: a binding narrower than it
    /// is not visible to it.
    fn order_import_mixed(&self, imp: &Import, module: ModuleId, ns: Namespace, name: Sym, interner: &Interner) -> bool {
        if imp.vis == VisKind::Priv {
            return false;
        }
        let Some(set) = self.order_dep.get(&(module, ns_index(ns) as u8, name)) else { return false };
        let mut first: Option<(VisKind, ModuleId)> = None;
        for (v, m, _) in &set.all {
            // rustc `import_decl_vis`: a binding narrower than the import
            // that the import still sees keeps its own visibility.
            let narrower = self.vis_within(v, *m, &imp.vis, imp.module, interner)
                && !self.same_vis(v, *m, &imp.vis, imp.module, interner)
                && self.visible(v, *m, imp.module, interner);
            let got = if narrower { (v.clone(), *m) } else { (imp.vis.clone(), imp.module) };
            match &first {
                None => first = Some(got),
                Some((fv, fm)) if !self.same_vis(fv, *fm, &got.0, got.1, interner) => return true,
                Some(_) => {}
            }
        }
        false
    }

    /// True when two visibilities reach the same set of modules.
    fn same_vis(&self, a: &VisKind, am: ModuleId, b: &VisKind, bm: ModuleId, interner: &Interner) -> bool {
        let (ra, ma) = self.vis_region(a, am, interner);
        let (rb, mb) = self.vis_region(b, bm, interner);
        ra == rb && ma == mb && (ra != 1 || self.root_of(am) == self.root_of(bm))
    }
}

/// Fills `tables.order_dep` from the final import tables, in two steps.
/// First the bindings each name may hold (`run_values` from `seed_keys`),
/// then `settle` decides which stored names have a fixed final binding.
/// The importers of a name `settle` leaves open run again, until nothing
/// changes (a cap falls back to treating no source as present for sure).
/// Names that hold one binding at all times are dropped at the end.
pub fn compute_order_dep(tables: &mut ResolvedTables, imports: &[Import], interner: &Interner) {
    tables.order_dep.clear();
    let globs = collect_globs(tables, imports, interner);
    let seeds = seed_keys(tables, &globs, interner);
    run_values(tables, &globs, seeds, false, interner);
    // The importers of a stored name counted it present before `settle`
    // decided it: run them again with what `settle` found.
    let mut done = false;
    for _ in 0..8 {
        settle(tables, &globs, interner);
        let mut again: Vec<Key> = Vec::new();
        for &(m, nsi, name) in tables.order_dep.keys() {
            if let Some(list) = globs.importers.get(&m) {
                again.extend(list.iter().map(|&i| (i, nsi, name)));
            }
        }
        if !run_values(tables, &globs, again, false, interner) {
            done = true;
            break;
        }
    }
    if !done {
        let mut again: Vec<Key> = Vec::new();
        for &(m, nsi, name) in tables.order_dep.keys() {
            if let Some(list) = globs.importers.get(&m) {
                again.extend(list.iter().map(|&i| (i, nsi, name)));
            }
        }
        run_values(tables, &globs, again, true, interner);
        settle(tables, &globs, interner);
    }
    // A name that holds one binding at all times reads as lust's table.
    tables.order_dep.retain(|_, s| !(s.fixed && !s.absent && s.all.len() == 1 && !matches!(s.all[0].2, AmbItem::Amb { .. })));
}

/// The first keys of `compute_order_dep`: the names two or more globs of
/// one module bring with different visibility or items, and every
/// ambiguous name with its glob importers. Globs of one visibility give
/// every binding at least that wide the same visibility, so only names
/// that some source binds narrower count there; all-private globs give
/// all of them the same one.
fn seed_keys(tables: &ResolvedTables, globs: &Globs, interner: &Interner) -> Vec<Key> {
    let mut work: Vec<Key> = globs.fixed.keys().copied().collect();
    let fixed_mods: FxMap<ModuleId, ()> = globs.fixed.keys().map(|k| (k.0, ())).collect();
    let mut count: FxMap<(u8, Sym), u8> = FxMap::default();
    let mut narrow: Vec<(u8, Sym)> = Vec::new();
    for (&m, list) in &globs.from {
        let has_fixed = fixed_mods.contains_key(&m);
        if list.len() + usize::from(has_fixed) < 2 {
            continue;
        }
        let one_vis = !has_fixed && list.iter().all(|(_, g)| g.vis == list[0].1.vis);
        if one_vis && list[0].1.vis == VisKind::Priv {
            continue;
        }
        count.clear();
        narrow.clear();
        // Only a binding this module can see is ever brought here.
        let has_std_aliases = !tables.std_aliases.is_empty();
        for &(src, g) in list {
            let g_ra = if one_vis { tables.vis_region(&g.vis, g.module, interner).0 } else { 0 };
            for nsi in 0..3u8 {
                for (&name, state) in &tables.maps[src.0 as usize][nsi as usize] {
                    match state {
                        BindState::One(b) => {
                            if one_vis {
                                // `Pub` is always at least as wide as `g.vis`, and `Crate`
                                // is at least as wide as any non-`Pub` `g.vis`: never narrow.
                                if b.vis == VisKind::Pub || (b.vis == VisKind::Crate && g_ra != 2) {
                                    continue;
                                }
                                if tables.visible(&b.vis, b.def_module, m, interner)
                                    && !tables.vis_within(&g.vis, g.module, &b.vis, b.def_module, interner)
                                {
                                    narrow.push((nsi, name));
                                }
                            } else if b.vis == VisKind::Pub || tables.visible(&b.vis, b.def_module, m, interner) {
                                *count.entry((nsi, name)).or_default() += 1;
                            }
                        }
                        BindState::Ambiguous => {
                            if !one_vis {
                                *count.entry((nsi, name)).or_default() += 1;
                            }
                        }
                    }
                }
            }
            if has_std_aliases {
                let range = (src, 0, Sym(0))..=(src, u8::MAX, Sym(u32::MAX));
                for (&(_, nsi, name), (vis, _)) in tables.std_aliases.range(range) {
                    if !tables.visible(vis, tables.std_alias_def_module((src, nsi, name)), m, interner) {
                        continue;
                    }
                    if one_vis {
                        narrow.push((nsi, name));
                    } else {
                        *count.entry((nsi, name)).or_default() += 1;
                    }
                }
            }
        }
        if one_vis {
            narrow.sort_unstable_by_key(|&(nsi, name)| (nsi, name.0));
            narrow.dedup();
            for &(nsi, name) in &narrow {
                let ns = NAMESPACES[nsi as usize];
                let n = list
                    .iter()
                    .filter(|(src, _)| tables.lookup(*src, ns, name).is_some() || tables.std_aliases.contains_key(&(*src, nsi, name)))
                    .count();
                if n >= 2 {
                    work.push((m, nsi, name));
                }
            }
        } else {
            work.extend(count.iter().filter(|&(_, &n)| n >= 2).map(|(&(nsi, name), _)| (m, nsi, name)));
        }
    }
    // Every ambiguous name with its glob importers (`ambiguous_keys`
    // holds every key ever made ambiguous; the order of `work` does not
    // matter, `run_values` sorts it).
    for &(src, nsi, name) in tables.ambiguous_keys.keys() {
        if matches!(tables.maps[src.0 as usize][nsi as usize].get(&name), Some(BindState::Ambiguous)) {
            work.push((src, nsi, name));
            work.extend(globs.importers.get(&src).into_iter().flatten().map(|&m| (m, nsi, name)));
        }
    }
    work
}

/// The first step of `compute_order_dep` from `work`: stores each key
/// whose bindings are not one at all times (`key_values`), and runs the
/// glob importers of each stored key whose set changed. Sets only grow.
/// Returns true when a set changed.
fn run_values(tables: &mut ResolvedTables, globs: &Globs, mut work: Vec<Key>, pessimistic: bool, interner: &Interner) -> bool {
    work.sort_unstable_by_key(|&(m, nsi, name)| (m.0, nsi, name.0));
    work.dedup();
    work.reverse();
    let mut changed = false;
    while let Some(key) = work.pop() {
        let Some((all, may_absent)) = key_values(tables, globs, key, pessimistic, interner) else { continue };
        if let Some(old) = tables.order_dep.get(&key)
            && old.all.len() == all.len()
            && old.may_absent == may_absent
        {
            continue;
        }
        changed = true;
        let set = OrderSet { ends: all.clone(), all, absent: may_absent, fixed: false, may_absent, settled: false };
        tables.order_dep.insert(key, set);
        if let Some(list) = globs.importers.get(&key.0) {
            work.extend(list.iter().map(|&m| (m, key.1, key.2)));
        }
    }
    changed
}

/// Resolves every glob import again against the final tables.
fn collect_globs<'a>(tables: &ResolvedTables, imports: &'a [Import], interner: &Interner) -> Globs<'a> {
    let mut g = Globs { from: FxMap::default(), importers: FxMap::default(), fixed: FxMap::default() };
    let value = ns_index(Namespace::Value) as u8;
    for imp in imports {
        let ImportKind::Glob { path } = &imp.kind else { continue };
        if path.is_empty() {
            continue;
        }
        match tables.resolve_path(imp.module, path, Namespace::Type, PathMode::Use, imp.leading_colon, interner) {
            PathRes::Done(Target::Def(adt)) if tables.variants.contains_key(&adt) => {
                let (vis, dm) = match tables.def_info.get(&adt) {
                    Some(info) => tables.glob_vis(imp, &info.vis, info.module, interner),
                    None => (imp.vis.clone(), imp.module),
                };
                for &v in &tables.variants[&adt] {
                    g.fixed.entry((imp.module, value, v)).or_default().push((vis.clone(), dm, AmbItem::Item(Target::Variant(adt, v))));
                }
            }
            _ => match tables.resolve_module_path(imp.module, path, imp.leading_colon, interner) {
                PathRes::Done(Target::Module(src)) if src != imp.module => {
                    g.from.entry(imp.module).or_default().push((src, imp));
                    let list = g.importers.entry(src).or_default();
                    if !list.contains(&imp.module) {
                        list.push(imp.module);
                    }
                }
                PathRes::Done(Target::Extern(i)) if tables.externs[i as usize].is_std_lib => {
                    let strs: Vec<&str> = path.iter().map(|s| interner.get(*s)).collect();
                    for v in crate::ty::lang::std_enum_variants(&strs) {
                        let name = interner.intern(v);
                        let mut full = path.clone();
                        full.push(name);
                        g.fixed.entry((imp.module, value, name)).or_default().push((imp.vis.clone(), imp.module, AmbItem::Std(full)));
                    }
                }
                _ => {}
            },
        }
    }
    g
}

/// The one binding a glob of `src` copies for a name, from lust's table,
/// when the name is not stored in `order_dep`: an ambiguity brings each
/// of its clashing bindings' visibility as `AmbItem::Amb`. None when
/// `src` has no binding of the name a glob copies.
fn table_values(tables: &ResolvedTables, key: Key) -> Option<Vec<AmbEntry>> {
    let (src, nsi, name) = key;
    let ns = NAMESPACES[nsi as usize];
    match tables.lookup(src, ns, name) {
        Some(BindState::One(b)) => {
            if ns == Namespace::Macro && tables.is_textual_bind(src, name, b) {
                return None;
            }
            Some(vec![(b.vis.clone(), b.def_module, AmbItem::Item(b.target))])
        }
        Some(BindState::Ambiguous) => {
            let amb = AmbItem::Amb { certain: false };
            Some(match tables.ambig_vis.get(&key) {
                Some(list) if !list.is_empty() => list.iter().map(|(v, m, _)| (v.clone(), *m, amb.clone())).collect(),
                _ => vec![(VisKind::Pub, src, amb)],
            })
        }
        None => {
            let (vis, path) = tables.std_aliases.get(&key)?;
            Some(vec![(vis.clone(), tables.std_alias_def_module(key), AmbItem::Std(path.clone()))])
        }
    }
}

/// True when a name in `key.0` is bound by an explicit binding (a local
/// item, a single import, or an explicit std alias): its globs do not
/// matter there.
fn explicit_bound(tables: &ResolvedTables, key: Key) -> bool {
    matches!(tables.lookup(key.0, NAMESPACES[key.1 as usize], key.2), Some(BindState::One(b)) if b.rank >= 2)
        || (tables.std_aliases.contains_key(&key) && !tables.std_glob_aliases.contains_key(&key))
}

/// Adds `e` to `list` unless an entry with the same item and the same
/// reach is there.
fn push_value(tables: &ResolvedTables, list: &mut Vec<AmbEntry>, e: AmbEntry, interner: &Interner) {
    if !list.iter().any(|(v, m, item)| *item == e.2 && tables.same_vis(v, *m, &e.0, e.1, interner)) {
        list.push(e);
    }
}

/// Every binding one name in one module may hold at some time, from its
/// globs, and whether it may end with none. A stored source brings its
/// stored bindings; its presence counts as `settle` decided it, or, while
/// it is not settled (and not `pessimistic`), as its first-step guess.
/// None when the name holds one binding at all times (nothing to store),
/// an explicit binding holds the name, or no glob brings it.
fn key_values(tables: &ResolvedTables, globs: &Globs, key: Key, pessimistic: bool, interner: &Interner) -> Option<(Vec<AmbEntry>, bool)> {
    let (m, nsi, name) = key;
    if explicit_bound(tables, key) {
        return None;
    }
    if one_binding(tables, globs, key, interner) {
        return None;
    }
    let mut all: Vec<AmbEntry> = globs.fixed.get(&key).cloned().unwrap_or_default();
    let mut absent = all.is_empty();
    for &(src, imp) in globs.from.get(&m).map(Vec::as_slice).unwrap_or(&[]) {
        let skey = (src, nsi, name);
        let (vals, mut maybe_absent) = match tables.order_dep.get(&skey) {
            Some(s) if s.settled && s.fixed && !s.absent => (s.all.clone(), !tables.visible(&s.ends[0].0, s.ends[0].1, m, interner)),
            Some(s) if s.settled || pessimistic => (s.all.clone(), s.absent || s.all.iter().any(|(v, dm, _)| !tables.visible(v, *dm, m, interner))),
            Some(s) => (s.all.clone(), s.may_absent),
            None => match table_values(tables, skey) {
                Some(v) => (v, false),
                None => continue,
            },
        };
        let mut brings = false;
        for (v, dm, item) in vals {
            if !tables.visible(&v, dm, m, interner) {
                maybe_absent |= tables.order_dep.get(&skey).is_none();
                continue;
            }
            brings = true;
            let (v, dm) = tables.glob_vis(imp, &v, dm, interner);
            push_value(tables, &mut all, (v, dm, item), interner);
        }
        // A glob whose source never holds a binding this module can see
        // never brings the name.
        if brings {
            absent &= maybe_absent;
        }
    }
    if all.is_empty() || (all.len() == 1 && !absent && !matches!(all[0].2, AmbItem::Amb { .. })) {
        return None;
    }
    Some((all, absent))
}

/// The common case of `key_values` without allocation: every glob of
/// the module brings the same item with the same visibility from a
/// source that holds one plain binding, or none this module can see.
fn one_binding(tables: &ResolvedTables, globs: &Globs, key: Key, interner: &Interner) -> bool {
    let (m, nsi, name) = key;
    if globs.fixed.contains_key(&key) {
        return false;
    }
    let ns = NAMESPACES[nsi as usize];
    let mut first: Option<(VisKind, ModuleId, Target)> = None;
    for &(src, imp) in globs.from.get(&m).map(Vec::as_slice).unwrap_or(&[]) {
        let skey = (src, nsi, name);
        if tables.order_dep.contains_key(&skey) {
            return false;
        }
        let b = match tables.lookup(src, ns, name) {
            Some(BindState::One(b)) => b,
            Some(BindState::Ambiguous) => return false,
            None if tables.std_aliases.contains_key(&skey) => return false,
            None => continue,
        };
        if (ns == Namespace::Macro && tables.is_textual_bind(src, name, b)) || !tables.visible(&b.vis, b.def_module, m, interner) {
            continue;
        }
        let (v, dm) = tables.glob_vis(imp, &b.vis, b.def_module, interner);
        match &first {
            None => first = Some((v, dm, b.target)),
            Some((fv, fdm, t)) => {
                if *t != b.target || !tables.same_vis(fv, *fdm, &v, dm, interner) {
                    return false;
                }
            }
        }
    }
    true
}

/// Decides `fixed`, `ends`, and `absent` for every stored name. A name's
/// final binding is fixed when it brings one item, every glob's source
/// has a fixed final binding, and no glob whose final binding is
/// narrower than the widest can be re-fetched later: rustc re-fetches
/// through a glob when the source's binding changes, and that change
/// starts at a module whose visibility can grow (two visibilities
/// stored), found upstream without passing through the name's own module.
/// The fixed end is the widest final binding. Starts with every name
/// fixed and present and clears the flags until nothing changes; a cleared flag stays
/// cleared, and a round cap clears every flag.
fn settle(tables: &mut ResolvedTables, globs: &Globs, interner: &Interner) {
    let mut keys: Vec<Key> = tables.order_dep.keys().copied().collect();
    keys.sort_unstable_by_key(|&(m, nsi, name)| (m.0, nsi, name.0));
    for k in &keys {
        if let Some(s) = tables.order_dep.get_mut(k) {
            s.fixed = true;
            s.absent = false;
            s.settled = true;
            s.ends.clear();
        }
    }
    let mut grows_memo: FxMap<Key, bool> = FxMap::default();
    for _round in 0..64 {
        let mut changed = false;
        for &key in &keys {
            let Some((fixed, ends, absent)) = settle_one(tables, globs, key, &mut grows_memo, interner) else { continue };
            let s = tables.order_dep.get_mut(&key).expect("stored key");
            let fixed = fixed && s.fixed;
            let ends = if fixed { ends } else { s.all.clone() };
            s.settled = true;
            if s.fixed != fixed || s.ends != ends || s.absent != absent {
                s.fixed = fixed;
                s.ends = ends;
                s.absent = absent;
                changed = true;
            }
        }
        // A name still waiting on its sources opens; the names that
        // counted on it run again.
        if !changed && !unsettled_to_open(tables) {
            return;
        }
    }
    for s in tables.order_dep.values_mut() {
        s.fixed = false;
        s.absent = true;
        s.ends = s.all.clone();
    }
}

/// The end of `settle`: a name still waiting on its sources (a cycle of
/// stored names) is not fixed. Returns true when one opened.
fn unsettled_to_open(tables: &mut ResolvedTables) -> bool {
    let mut opened = false;
    for s in tables.order_dep.values_mut() {
        if s.fixed && s.ends.is_empty() {
            s.fixed = false;
            s.absent = true;
            s.ends = s.all.clone();
            opened = true;
        }
    }
    opened
}

/// One step of `settle` for one stored name: (fixed, ends, absent), or
/// None while every final binding it could get comes from fixed sources
/// not settled yet.
fn settle_one(
    tables: &ResolvedTables,
    globs: &Globs,
    key: Key,
    grows_memo: &mut FxMap<Key, bool>,
    interner: &Interner,
) -> Option<(bool, Vec<AmbEntry>, bool)> {
    let (m, nsi, name) = key;
    let set = &tables.order_dep[&key];
    let one_item = set.all.iter().all(|(_, _, item)| !matches!(item, AmbItem::Amb { .. }) && *item == set.all[0].2);
    // (glob source, its final binding seen through the glob) per glob.
    let mut finals: Vec<(Option<ModuleId>, AmbEntry)> = globs.fixed.get(&key).map(|l| l.iter().map(|e| (None, e.clone())).collect()).unwrap_or_default();
    let mut all_fixed = true;
    let mut pending = false;
    let mut absent = finals.is_empty();
    for &(src, imp) in globs.from.get(&m).map(Vec::as_slice).unwrap_or(&[]) {
        let skey = (src, nsi, name);
        let (src_fixed, ends, src_absent, src_all) = match tables.order_dep.get(&skey) {
            Some(s) => (s.fixed && !s.absent, s.ends.clone(), s.absent, s.all.clone()),
            None => match table_values(tables, skey) {
                Some(v) if v.len() == 1 => (true, v.clone(), false, v),
                Some(v) => (false, v.clone(), false, v),
                None => continue,
            },
        };
        if src_fixed {
            // A fixed source not settled yet adds nothing this round.
            let Some((v, dm, item)) = ends.into_iter().next() else {
                pending = true;
                continue;
            };
            if tables.visible(&v, dm, m, interner) {
                let (v, dm) = tables.glob_vis(imp, &v, dm, interner);
                finals.push((Some(src), (v, dm, item)));
                absent = false;
            }
        } else if src_all.iter().any(|(v, dm, _)| tables.visible(v, *dm, m, interner)) {
            all_fixed = false;
            let sees_all = src_all.iter().all(|(v, dm, _)| tables.visible(v, *dm, m, interner));
            absent &= src_absent || !sees_all;
        }
    }
    if !one_item || !all_fixed {
        return Some((false, Vec::new(), absent));
    }
    if finals.is_empty() {
        return if pending { None } else { Some((false, Vec::new(), absent)) };
    }
    // The widest final binding; all of them are ancestors' visibilities.
    let Some((_, widest)) = finals.iter().find(|(_, (v, dm, _))| finals.iter().all(|(_, (ov, odm, _))| tables.vis_within(ov, *odm, v, *dm, interner)))
    else {
        return Some((false, Vec::new(), absent));
    };
    let widest = widest.clone();
    for (src, (v, dm, _)) in &finals {
        let Some(src) = *src else { continue };
        if !tables.same_vis(v, *dm, &widest.0, widest.1, interner) && upstream_grows(tables, globs, (src, nsi, name), m, grows_memo, interner) {
            return Some((false, Vec::new(), false));
        }
    }
    Some((true, vec![widest], false))
}

/// True when the binding of `start` can change after a module first
/// copies it: `start` or a module upstream of it (through globs that can
/// bring the name, not through `skip`) has two stored visibilities, so
/// its visibility can grow and rustc re-sends it down the chain.
fn upstream_grows(tables: &ResolvedTables, globs: &Globs, start: Key, skip: ModuleId, memo: &mut FxMap<Key, bool>, interner: &Interner) -> bool {
    // Without `skip` first: when nothing upstream grows, no path does.
    let any = match memo.get(&start) {
        Some(&b) => b,
        None => {
            let b = grows_from(tables, globs, start, None, interner);
            memo.insert(start, b);
            b
        }
    };
    any && grows_from(tables, globs, start, Some(skip), interner)
}

/// The search of `upstream_grows`.
fn grows_from(tables: &ResolvedTables, globs: &Globs, start: Key, skip: Option<ModuleId>, interner: &Interner) -> bool {
    let (_, nsi, name) = start;
    let mut seen: Vec<ModuleId> = vec![start.0];
    let mut stack: Vec<ModuleId> = vec![start.0];
    while let Some(t) = stack.pop() {
        if let Some(s) = tables.order_dep.get(&(t, nsi, name)) {
            let vis_two = s.all.iter().skip(1).any(|(v, m, _)| !tables.same_vis(v, *m, &s.all[0].0, s.all[0].1, interner));
            if vis_two {
                return true;
            }
        }
        if explicit_bound(tables, (t, nsi, name)) {
            continue;
        }
        for &(u, _) in globs.from.get(&t).map(Vec::as_slice).unwrap_or(&[]) {
            if Some(u) == skip || seen.contains(&u) {
                continue;
            }
            let ukey = (u, nsi, name);
            let vals = match tables.order_dep.get(&ukey) {
                Some(s) => s.all.clone(),
                None => table_values(tables, ukey).unwrap_or_default(),
            };
            if vals.iter().any(|(v, m, _)| tables.visible(v, *m, t, interner)) {
                seen.push(u);
                stack.push(u);
            }
        }
    }
    false
}

/// L0509 for each module-level import whose path reads a glob name with
/// an order-dependent answer (`order_dependent`), or one whose widest and
/// narrowest bindings give the import different visibilities
/// (`order_import_mixed`). Every segment counts. One diagnostic per
/// import; imports in `done` are already reported.
pub fn report_order_imports(tables: &ResolvedTables, imports: &[Import], interner: &Interner, diags: &mut Vec<Diag>, done: &[Span]) {
    let mut seen: Vec<Span> = done.to_vec();
    if tables.order_dep.is_empty() {
        return;
    }
    // Only a path that names a stored name can read one.
    let names: FxMap<Sym, ()> = tables.order_dep.keys().map(|k| (k.2, ())).collect();
    let crate_like = |s: Sym| matches!(interner.get(s), "crate" | "self" | "super") || tables.crate_magic.contains_key(&s);
    for imp in imports {
        let (mod_path, last) = match &imp.kind {
            ImportKind::Single { path, .. } => match path.split_last() {
                Some((&last, rest)) => (rest, Some(last)),
                None => continue,
            },
            ImportKind::Glob { path } | ImportKind::CheckPath { path } => (path.as_slice(), None),
            ImportKind::BadLeading { .. } => continue,
        };
        if mod_path.is_empty() || seen.contains(&imp.span) || !mod_path.iter().chain(&last).any(|s| names.contains_key(s)) {
            continue;
        }
        let hit = |module: ModuleId, ns: Namespace, name: Sym| {
            tables.order_dependent(module, ns, name, imp.module, interner) || tables.order_import_mixed(imp, module, ns, name, interner)
        };
        let mut bad: Option<String> = None;
        // Each module segment, read in the module the prefix before it names.
        for k in 0..mod_path.len() {
            let parent = if k == 0 {
                if imp.leading_colon || crate_like(mod_path[0]) {
                    continue;
                }
                imp.module
            } else {
                match tables.resolve_module_path(imp.module, &mod_path[..k], imp.leading_colon, interner) {
                    PathRes::Done(Target::Module(p)) => p,
                    _ => break,
                }
            };
            if names.contains_key(&mod_path[k]) && hit(parent, Namespace::Type, mod_path[k]) {
                bad = Some(mod_path[..=k].iter().map(|s| interner.get(*s)).collect::<Vec<_>>().join("::"));
                break;
            }
        }
        if bad.is_none()
            && let Some(last) = last
            && names.contains_key(&last)
            && let PathRes::Done(Target::Module(src)) = tables.resolve_module_path(imp.module, mod_path, imp.leading_colon, interner)
            && NAMESPACES.iter().any(|&ns| hit(src, ns, last))
        {
            bad = Some(interner.get(last).to_string());
        }
        if let Some(what) = bad {
            seen.push(imp.span);
            diags.push(order_diag(&what, imp.span));
        }
    }
}

/// The L0509 diagnostic for path `what`: it reads a name that glob
/// imports bring with an answer that depends on rustc's import order.
pub fn order_diag(what: &str, span: Span) -> Diag {
    Diag::new(
        "L0509",
        span,
        format!(
            "`{what}` reads a glob-imported name whose binding depends on rustc's import order; lust rejects this on purpose (a lust subset rule)"
        ),
        "Import the item explicitly from the module that defines it (`use crate::module::Name;`): an explicit import wins over globs.",
    )
}
