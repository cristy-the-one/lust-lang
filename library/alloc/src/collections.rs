// `alloc::collections` overlay (docs/ARCH.md section 3.1; W-STD-C):
// `BTreeMap`, `BTreeSet`, `VecDeque`, and their modules `btree_map`,
// `btree_set`, `vec_deque`. `HashMap` and `HashSet` stay W-STD-B's;
// `std::collections::X` and `alloc::collections::X` walk into this
// module only for the names it defines (`STD_MODULES` in
// src/stdmods.rs).
//
// Lust std source with std's observable behavior, not std's data
// structures:
// - `BTreeMap` and `BTreeSet` keep their entries sorted in one `Vec`
//   (binary search; O(n) insert and remove). Iteration order, drop
//   order (ascending keys, key before value), which key an insert of
//   an existing key keeps (the old one), and every return value are
//   std's.
// - `VecDeque` keeps its elements in logical order in one `Vec`, which
//   grows by the same RawVec rule, so `capacity()` matches. A ghost
//   `head` field tracks where std's ring head would be (growth keeps
//   it, front/back shifts follow std's insert/remove/rotate/shrink
//   rules), so `as_slices` splits and every other ring-observable
//   result match as well (W-STD-C phase 2).
// - The iterator types are nominal structs whose one field holds a
//   builtin adapter (`Map<slice::Iter<'a, (K, V)>, fn(&(K, V)) ->
//   (&K, &V)>` and kin), so the whole adapter set works on them (the
//   checker types their methods as the inner type's; see
//   `collection_iter_inner`), with std's item types. They stay
//   distinct from the inner types, as std's own iterator structs
//   are. They have no `Debug` (std's prints the remaining items).
// Not here yet: the cursor and `extract_if`
// APIs, `DoubleEndedIterator`
// for the `Map` aliases beyond what the builtin adapters provide.

// The three types are defined here, not only re-exported, so a user
// `use std::collections::X` finds them while the overlay's own imports
// are still resolving; their modules re-export them.

/// An ordered map: entries sorted by key.
pub struct BTreeMap<K, V> {
    entries: Vec<(K, V)>,
}

/// An ordered set: elements sorted, no duplicates.
pub struct BTreeSet<T> {
    items: Vec<T>,
}

/// A double-ended queue. `buf` holds the elements in logical
/// order; `head` is where std's ring head would sit in a buffer
/// of `buf.capacity()` slots (see the file header).
pub struct VecDeque<T> {
    buf: Vec<T>,
    head: usize,
}

/// The order of two keys. A `.cmp` call on a generic `Q` cannot be
/// instantiated for every `Q` yet (`&str`, tuples: the generic stub
/// pick), the comparison operators can. For the key types allowed here
/// (`Ord` consistent with `PartialOrd`, see the header) the answer is
/// `Ord::cmp`'s.
fn cmp_of<Q: ?Sized + Ord>(a: &Q, b: &Q) -> core::cmp::Ordering {
    if a < b {
        core::cmp::Ordering::Less
    } else if a == b {
        core::cmp::Ordering::Equal
    } else {
        core::cmp::Ordering::Greater
    }
}

pub mod btree_map {
    use core::borrow::Borrow;
    use core::hash::{Hash, Hasher};

    pub use super::BTreeMap;

    pub struct Iter<'a, K, V> {
        inner: core::iter::Map<core::slice::Iter<'a, (K, V)>, fn(&(K, V)) -> (&K, &V)>,
    }
    pub struct IterMut<'a, K, V> {
        inner: core::iter::Map<core::slice::IterMut<'a, (K, V)>, fn(&mut (K, V)) -> (&K, &mut V)>,
    }
    pub struct Keys<'a, K, V> {
        inner: core::iter::Map<core::slice::Iter<'a, (K, V)>, fn(&(K, V)) -> &K>,
    }
    pub struct Values<'a, K, V> {
        inner: core::iter::Map<core::slice::Iter<'a, (K, V)>, fn(&(K, V)) -> &V>,
    }
    pub struct ValuesMut<'a, K, V> {
        inner: core::iter::Map<core::slice::IterMut<'a, (K, V)>, fn(&mut (K, V)) -> &mut V>,
    }
    pub struct IntoIter<K, V> {
        inner: std::vec::IntoIter<(K, V)>,
    }
    pub struct IntoKeys<K, V> {
        inner: core::iter::Map<std::vec::IntoIter<(K, V)>, fn((K, V)) -> K>,
    }
    pub struct IntoValues<K, V> {
        inner: core::iter::Map<std::vec::IntoIter<(K, V)>, fn((K, V)) -> V>,
    }

    impl<'a, K, V> Iterator for Iter<'a, K, V> {
        type Item = (&'a K, &'a V);
        fn next(&mut self) -> Option<(&'a K, &'a V)> {
            self.inner.next()
        }
    }
    impl<'a, K, V> Iterator for IterMut<'a, K, V> {
        type Item = (&'a K, &'a mut V);
        fn next(&mut self) -> Option<(&'a K, &'a mut V)> {
            self.inner.next()
        }
    }
    impl<'a, K, V> Iterator for Keys<'a, K, V> {
        type Item = &'a K;
        fn next(&mut self) -> Option<&'a K> {
            self.inner.next()
        }
    }
    impl<'a, K, V> Iterator for Values<'a, K, V> {
        type Item = &'a V;
        fn next(&mut self) -> Option<&'a V> {
            self.inner.next()
        }
    }
    impl<'a, K, V> Iterator for ValuesMut<'a, K, V> {
        type Item = &'a mut V;
        fn next(&mut self) -> Option<&'a mut V> {
            self.inner.next()
        }
    }
    impl<K, V> Iterator for IntoIter<K, V> {
        type Item = (K, V);
        fn next(&mut self) -> Option<(K, V)> {
            self.inner.next()
        }
    }
    impl<K, V> Iterator for IntoKeys<K, V> {
        type Item = K;
        fn next(&mut self) -> Option<K> {
            self.inner.next()
        }
    }
    impl<K, V> Iterator for IntoValues<K, V> {
        type Item = V;
        fn next(&mut self) -> Option<V> {
            self.inner.next()
        }
    }
    pub struct Range<'a, K, V> {
        inner: core::iter::Map<core::slice::Iter<'a, (K, V)>, fn(&(K, V)) -> (&K, &V)>,
    }
    pub struct RangeMut<'a, K, V> {
        inner: core::iter::Map<core::slice::IterMut<'a, (K, V)>, fn(&mut (K, V)) -> (&K, &mut V)>,
    }

    impl<'a, K, V> Iterator for Range<'a, K, V> {
        type Item = (&'a K, &'a V);
        fn next(&mut self) -> Option<(&'a K, &'a V)> {
            self.inner.next()
        }
    }
    impl<'a, K, V> Iterator for RangeMut<'a, K, V> {
        type Item = (&'a K, &'a mut V);
        fn next(&mut self) -> Option<(&'a K, &'a mut V)> {
            self.inner.next()
        }
    }

    fn pair_ref<K, V>(p: &(K, V)) -> (&K, &V) {
        (&p.0, &p.1)
    }

    fn pair_mut<K, V>(p: &mut (K, V)) -> (&K, &mut V) {
        (&p.0, &mut p.1)
    }

    fn key_ref<K, V>(p: &(K, V)) -> &K {
        &p.0
    }

    fn value_ref<K, V>(p: &(K, V)) -> &V {
        &p.1
    }

    fn value_mut<K, V>(p: &mut (K, V)) -> &mut V {
        &mut p.1
    }

    fn into_key<K, V>(p: (K, V)) -> K {
        p.0
    }

    fn into_value<K, V>(p: (K, V)) -> V {
        p.1
    }

    impl<K, V> BTreeMap<K, V> {
        pub const fn new() -> BTreeMap<K, V> {
            BTreeMap { entries: Vec::new() }
        }

        pub fn len(&self) -> usize {
            self.entries.len()
        }

        pub fn is_empty(&self) -> bool {
            self.entries.is_empty()
        }

        pub fn clear(&mut self) {
            self.entries.clear();
        }

        pub fn iter(&self) -> Iter<'_, K, V> {
            Iter { inner: self.entries.iter().map(pair_ref as fn(&(K, V)) -> (&K, &V)) }
        }

        pub fn iter_mut(&mut self) -> IterMut<'_, K, V> {
            IterMut { inner: self.entries.iter_mut().map(pair_mut as fn(&mut (K, V)) -> (&K, &mut V)) }
        }

        pub fn keys(&self) -> Keys<'_, K, V> {
            Keys { inner: self.entries.iter().map(key_ref as fn(&(K, V)) -> &K) }
        }

        pub fn values(&self) -> Values<'_, K, V> {
            Values { inner: self.entries.iter().map(value_ref as fn(&(K, V)) -> &V) }
        }

        pub fn values_mut(&mut self) -> ValuesMut<'_, K, V> {
            ValuesMut { inner: self.entries.iter_mut().map(value_mut as fn(&mut (K, V)) -> &mut V) }
        }

        pub fn into_keys(self) -> IntoKeys<K, V> {
            IntoKeys { inner: self.entries.into_iter().map(into_key as fn((K, V)) -> K) }
        }

        pub fn into_values(self) -> IntoValues<K, V> {
            IntoValues { inner: self.entries.into_iter().map(into_value as fn((K, V)) -> V) }
        }

        pub fn first_key_value(&self) -> Option<(&K, &V)> {
            match self.entries.first() {
                Some(p) => Some((&p.0, &p.1)),
                None => None,
            }
        }

        pub fn last_key_value(&self) -> Option<(&K, &V)> {
            match self.entries.last() {
                Some(p) => Some((&p.0, &p.1)),
                None => None,
            }
        }

        pub fn pop_first(&mut self) -> Option<(K, V)> {
            if self.entries.is_empty() { None } else { Some(self.entries.remove(0)) }
        }

        pub fn pop_last(&mut self) -> Option<(K, V)> {
            self.entries.pop()
        }

        pub fn retain<F: FnMut(&K, &mut V) -> bool>(&mut self, mut f: F) {
            let mut i = 0;
            while i < self.entries.len() {
                let keep = {
                    let p = &mut self.entries[i];
                    f(&p.0, &mut p.1)
                };
                if keep {
                    i += 1;
                } else {
                    self.entries.remove(i);
                }
            }
        }
    }

    impl<K: Ord, V> BTreeMap<K, V> {
        /// The index of `key`, or where it would go, by `K`'s own order.
        fn search_key(&self, key: &K) -> Result<usize, usize> {
            let mut lo = 0;
            let mut hi = self.entries.len();
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                match super::cmp_of(&self.entries[mid].0, key) {
                    core::cmp::Ordering::Less => lo = mid + 1,
                    core::cmp::Ordering::Greater => hi = mid,
                    core::cmp::Ordering::Equal => return Ok(mid),
                }
            }
            Err(lo)
        }

        /// The index of `key`, or where it would go, through `Borrow`.
        fn search<Q: ?Sized + Ord>(&self, key: &Q) -> Result<usize, usize>
        where
            K: Borrow<Q>,
        {
            let mut lo = 0;
            let mut hi = self.entries.len();
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                match super::cmp_of(self.entries[mid].0.borrow(), key) {
                    core::cmp::Ordering::Less => lo = mid + 1,
                    core::cmp::Ordering::Greater => hi = mid,
                    core::cmp::Ordering::Equal => return Ok(mid),
                }
            }
            Err(lo)
        }

        pub fn insert(&mut self, key: K, value: V) -> Option<V> {
            match self.search_key(&key) {
                Ok(i) => Some(core::mem::replace(&mut self.entries[i].1, value)),
                Err(i) => {
                    self.entries.insert(i, (key, value));
                    None
                }
            }
        }

        pub fn get<Q: ?Sized + Ord>(&self, key: &Q) -> Option<&V>
        where
            K: Borrow<Q>,
        {
            match self.search(key) {
                Ok(i) => Some(&self.entries[i].1),
                Err(_) => None,
            }
        }

        pub fn get_key_value<Q: ?Sized + Ord>(&self, key: &Q) -> Option<(&K, &V)>
        where
            K: Borrow<Q>,
        {
            match self.search(key) {
                Ok(i) => Some((&self.entries[i].0, &self.entries[i].1)),
                Err(_) => None,
            }
        }

        pub fn get_mut<Q: ?Sized + Ord>(&mut self, key: &Q) -> Option<&mut V>
        where
            K: Borrow<Q>,
        {
            match self.search(key) {
                Ok(i) => Some(&mut self.entries[i].1),
                Err(_) => None,
            }
        }

        pub fn contains_key<Q: ?Sized + Ord>(&self, key: &Q) -> bool
        where
            K: Borrow<Q>,
        {
            match self.search(key) {
                Ok(_) => true,
                Err(_) => false,
            }
        }

        pub fn remove<Q: ?Sized + Ord>(&mut self, key: &Q) -> Option<V>
        where
            K: Borrow<Q>,
        {
            match self.search(key) {
                Ok(i) => Some(self.entries.remove(i).1),
                Err(_) => None,
            }
        }

        pub fn remove_entry<Q: ?Sized + Ord>(&mut self, key: &Q) -> Option<(K, V)>
        where
            K: Borrow<Q>,
        {
            match self.search(key) {
                Ok(i) => Some(self.entries.remove(i)),
                Err(_) => None,
            }
        }

        pub fn entry(&mut self, key: K) -> Entry<'_, K, V> {
            match self.search_key(&key) {
                Ok(index) => Entry::Occupied(OccupiedEntry { map: self, index }),
                Err(index) => Entry::Vacant(VacantEntry { map: self, key, index }),
            }
        }

        /// The entry of the smallest key, if any.
        pub fn first_entry(&mut self) -> Option<OccupiedEntry<'_, K, V>> {
            if self.entries.is_empty() { None } else { Some(OccupiedEntry { map: self, index: 0 }) }
        }

        /// The entry of the largest key, if any.
        pub fn last_entry(&mut self) -> Option<OccupiedEntry<'_, K, V>> {
            match self.entries.len() {
                0 => None,
                n => Some(OccupiedEntry { map: self, index: n - 1 }),
            }
        }

        /// Moves every entry of `other` in. On an equal key `other`'s whole
        /// entry stays and this map's drops, as std's merge does.
        pub fn append(&mut self, other: &mut BTreeMap<K, V>) {
            if other.entries.is_empty() {
                return;
            }
            if self.entries.is_empty() {
                core::mem::swap(&mut self.entries, &mut other.entries);
                return;
            }
            let left = core::mem::replace(&mut self.entries, Vec::new());
            let right = core::mem::replace(&mut other.entries, Vec::new());
            let mut out: Vec<(K, V)> = Vec::new();
            let mut a = left.into_iter();
            let mut b = right.into_iter();
            let mut na = a.next();
            let mut nb = b.next();
            loop {
                let ord = if let Some(x) = &na {
                    if let Some(y) = &nb { super::cmp_of(&x.0, &y.0) } else { core::cmp::Ordering::Less }
                } else if nb.is_some() {
                    core::cmp::Ordering::Greater
                } else {
                    break
                };
                match ord {
                    core::cmp::Ordering::Less => {
                        out.push(core::mem::replace(&mut na, None).unwrap()); //@ never
                        na = a.next();
                    }
                    core::cmp::Ordering::Greater => {
                        out.push(core::mem::replace(&mut nb, None).unwrap()); //@ never
                        nb = b.next();
                    }
                    core::cmp::Ordering::Equal => {
                        let dropped = core::mem::replace(&mut na, None);
                        drop(dropped);
                        out.push(core::mem::replace(&mut nb, None).unwrap()); //@ never
                        na = a.next();
                        nb = b.next();
                    }
                }
            }
            self.entries = out;
        }

        /// Splits off the entries with keys at or above `key`.
        pub fn split_off<Q: ?Sized + Ord>(&mut self, key: &Q) -> BTreeMap<K, V>
        where
            K: Borrow<Q>,
        {
            let at = match self.search(key) {
                Ok(i) => i,
                Err(i) => i,
            };
            BTreeMap { entries: self.entries.split_off(at) }
        }
    }

    impl<K: Ord, V> BTreeMap<K, V> {
        /// The entry span of a range: std's panic checks first
        /// (BTreeMap texts), then binary searches for both ends. An
        /// empty map checks nothing, as std's has no root to search.
        fn range_span<Q: ?Sized + Ord>(&self, start: core::ops::Bound<&Q>, end: core::ops::Bound<&Q>) -> (usize, usize)
        where
            K: Borrow<Q>,
        {
            use core::ops::Bound;
            if self.entries.is_empty() {
                return (0, 0);
            }
            let bounded = match (&start, &end) {
                (Bound::Excluded(s), Bound::Excluded(e)) => {
                    if super::cmp_of(*s, *e) == core::cmp::Ordering::Equal {
                        panic!("range start and end are equal and excluded in BTreeMap"); //@ alloc/src/collections/btree/search.rs:112:21
                    }
                    Some((s, e))
                }
                (Bound::Included(s), Bound::Included(e)) => Some((s, e)),
                (Bound::Included(s), Bound::Excluded(e)) => Some((s, e)),
                (Bound::Excluded(s), Bound::Included(e)) => Some((s, e)),
                _ => None,
            };
            if let Some((s, e)) = bounded
                && super::cmp_of(*s, *e) == core::cmp::Ordering::Greater
            {
                panic!("range start is greater than range end in BTreeMap"); //@ alloc/src/collections/btree/search.rs:121:21
            }
            let lo = match &start {
                Bound::Included(x) => match self.search(*x) {
                    Ok(i) => i,
                    Err(i) => i,
                },
                Bound::Excluded(x) => match self.search(*x) {
                    Ok(i) => i + 1,
                    Err(i) => i,
                },
                Bound::Unbounded => 0,
            };
            let hi = match &end {
                Bound::Included(x) => match self.search(*x) {
                    Ok(i) => i + 1,
                    Err(i) => i,
                },
                Bound::Excluded(x) => match self.search(*x) {
                    Ok(i) => i,
                    Err(i) => i,
                },
                Bound::Unbounded => self.entries.len(),
            };
            (lo, hi)
        }

        /// `range` with its bounds already split.
        fn range_between<Q: ?Sized + Ord>(&self, start: core::ops::Bound<&Q>, end: core::ops::Bound<&Q>) -> Range<'_, K, V>
        where
            K: Borrow<Q>,
        {
            let (lo, hi) = self.range_span(start, end);
            let slice: &[(K, V)] = &self.entries[lo..hi];
            Range { inner: slice.iter().map(pair_ref as fn(&(K, V)) -> (&K, &V)) }
        }

        /// `range_mut` with its bounds already split.
        fn range_between_mut<Q: ?Sized + Ord>(&mut self, start: core::ops::Bound<&Q>, end: core::ops::Bound<&Q>) -> RangeMut<'_, K, V>
        where
            K: Borrow<Q>,
        {
            let (lo, hi) = self.range_span(start, end);
            let slice: &mut [(K, V)] = &mut self.entries[lo..hi];
            RangeMut { inner: slice.iter_mut().map(pair_mut as fn(&mut (K, V)) -> (&K, &mut V)) }
        }
    }

    impl<K: Ord, V> BTreeMap<K, V> {
    }

    impl<K: Ord, V> BTreeMap<K, V> {
        pub fn range<Q: ?Sized + Ord, R: core::ops::RangeBounds<Q>>(&self, range: R) -> Range<'_, K, V>
        where
            K: Borrow<Q>,
        {
            self.range_between(range.start_bound(), range.end_bound())
        }

        pub fn range_mut<Q: ?Sized + Ord, R: core::ops::RangeBounds<Q>>(&mut self, range: R) -> RangeMut<'_, K, V>
        where
            K: Borrow<Q>,
        {
            self.range_between_mut(range.start_bound(), range.end_bound())
        }
    }

    /// A view into one entry of a map.
    pub enum Entry<'a, K, V> {
        Vacant(VacantEntry<'a, K, V>),
        Occupied(OccupiedEntry<'a, K, V>),
    }

    /// An entry whose key is not in the map.
    pub struct VacantEntry<'a, K, V> {
        map: &'a mut BTreeMap<K, V>,
        key: K,
        index: usize,
    }

    /// An entry whose key is in the map.
    pub struct OccupiedEntry<'a, K, V> {
        map: &'a mut BTreeMap<K, V>,
        index: usize,
    }

    impl<'a, K: Ord, V> Entry<'a, K, V> {
        pub fn or_insert(self, default: V) -> &'a mut V {
            match self {
                Entry::Occupied(e) => e.into_mut(),
                Entry::Vacant(e) => e.insert(default),
            }
        }

        pub fn or_insert_with<F: FnOnce() -> V>(self, default: F) -> &'a mut V {
            match self {
                Entry::Occupied(e) => e.into_mut(),
                Entry::Vacant(e) => e.insert(default()),
            }
        }

        pub fn or_insert_with_key<F: FnOnce(&K) -> V>(self, default: F) -> &'a mut V {
            match self {
                Entry::Occupied(e) => e.into_mut(),
                Entry::Vacant(e) => {
                    let value = default(&e.key);
                    e.insert(value)
                }
            }
        }

        pub fn key(&self) -> &K {
            match self {
                Entry::Occupied(e) => e.key(),
                Entry::Vacant(e) => e.key(),
            }
        }

        pub fn and_modify<F: FnOnce(&mut V)>(self, f: F) -> Entry<'a, K, V> {
            match self {
                Entry::Occupied(mut e) => {
                    f(e.get_mut());
                    Entry::Occupied(e)
                }
                Entry::Vacant(e) => Entry::Vacant(e),
            }
        }
    }

    impl<'a, K: Ord, V: Default> Entry<'a, K, V> {
        pub fn or_default(self) -> &'a mut V {
            match self {
                Entry::Occupied(e) => e.into_mut(),
                Entry::Vacant(e) => e.insert(V::default()),
            }
        }
    }

    impl<'a, K: Ord, V> VacantEntry<'a, K, V> {
        pub fn key(&self) -> &K {
            &self.key
        }

        pub fn into_key(self) -> K {
            self.key
        }

        pub fn insert(self, value: V) -> &'a mut V {
            let index = self.index;
            let map = self.map;
            map.entries.insert(index, (self.key, value));
            &mut map.entries[index].1
        }
    }

    impl<'a, K: Ord, V> OccupiedEntry<'a, K, V> {
        pub fn key(&self) -> &K {
            &self.map.entries[self.index].0
        }

        pub fn get(&self) -> &V {
            &self.map.entries[self.index].1
        }

        pub fn get_mut(&mut self) -> &mut V {
            &mut self.map.entries[self.index].1
        }

        pub fn into_mut(self) -> &'a mut V {
            let index = self.index;
            &mut self.map.entries[index].1
        }

        pub fn insert(&mut self, value: V) -> V {
            core::mem::replace(&mut self.map.entries[self.index].1, value)
        }

        pub fn remove(self) -> V {
            self.map.entries.remove(self.index).1
        }

        pub fn remove_entry(self) -> (K, V) {
            self.map.entries.remove(self.index)
        }
    }

    impl<K, V> Default for BTreeMap<K, V> {
        fn default() -> BTreeMap<K, V> {
            BTreeMap::new()
        }
    }

    impl<K: Clone, V: Clone> Clone for BTreeMap<K, V> {
        fn clone(&self) -> BTreeMap<K, V> {
            BTreeMap { entries: self.entries.clone() }
        }
    }

    impl<K: PartialEq, V: PartialEq> PartialEq for BTreeMap<K, V> {
        fn eq(&self, other: &BTreeMap<K, V>) -> bool {
            self.entries == other.entries
        }
    }

    impl<K: Eq, V: Eq> Eq for BTreeMap<K, V> {}

    impl<K: core::fmt::Debug, V: core::fmt::Debug> core::fmt::Debug for BTreeMap<K, V> {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let mut m = f.debug_map();
            for p in &self.entries {
                m.entry(&p.0, &p.1);
            }
            m.finish()
        }
    }

    /// Std's bulk build: a stable sort by key, then the last of each run
    /// of equal keys stays (the earlier ones drop in order), as
    /// `DedupSortedIter` does.
    fn sorted_dedup_last<K: Ord, V>(mut items: Vec<(K, V)>) -> Vec<(K, V)> {
        use super::cmp_of;
        // Stable insertion sort (comparisons are unobservable: keys
        // reaching a written Ord impl reject at check, see the header).
        let mut i = 1;
        while i < items.len() {
            let mut j = i;
            while j > 0 && cmp_of(&items[j - 1].0, &items[j].0) == core::cmp::Ordering::Greater {
                items.swap(j - 1, j);
                j -= 1;
            }
            i += 1;
        }
        let mut out: Vec<(K, V)> = Vec::new();
        for item in items {
            let dup = match out.last() {
                Some(last) => cmp_of(&last.0, &item.0) == core::cmp::Ordering::Equal,
                None => false,
            };
            if dup {
                out.pop();
            }
            out.push(item);
        }
        out
    }

    impl<K: Ord, V> FromIterator<(K, V)> for BTreeMap<K, V> {
        fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> BTreeMap<K, V> {
            let mut items: Vec<(K, V)> = Vec::new();
            for kv in iter {
                items.push(kv);
            }
            BTreeMap { entries: sorted_dedup_last(items) }
        }
    }

    // Core's generic try collect over this map (F3), written out
    // here (a generic `B::from_iter` call on a parameter is not
    // in the subset yet): pull the `Ok` prefix, bulk-build it
    // exactly as `from_iter` does, and on an `Err` drop the
    // source iterator before the built prefix, as std's order is.
    impl<K: Ord, V, E> FromIterator<Result<(K, V), E>> for Result<BTreeMap<K, V>, E> {
        fn from_iter<I: IntoIterator<Item = Result<(K, V), E>>>(iter: I) -> Result<BTreeMap<K, V>, E> {
            let mut items: Vec<(K, V)> = Vec::new();
            let mut it = iter.into_iter();
            loop {
                match it.next() {
                    Some(Ok(kv)) => items.push(kv),
                    Some(Err(e)) => {
                        let built = BTreeMap { entries: sorted_dedup_last(items) };
                        drop(it);
                        drop(built);
                        return Err(e);
                    }
                    None => break,
                }
            }
            Ok(BTreeMap { entries: sorted_dedup_last(items) })
        }
    }

    // The `Option` form of the try collect above.
    impl<K: Ord, V> FromIterator<Option<(K, V)>> for Option<BTreeMap<K, V>> {
        fn from_iter<I: IntoIterator<Item = Option<(K, V)>>>(iter: I) -> Option<BTreeMap<K, V>> {
            let mut items: Vec<(K, V)> = Vec::new();
            let mut it = iter.into_iter();
            loop {
                match it.next() {
                    Some(Some(kv)) => items.push(kv),
                    Some(None) => {
                        let built = BTreeMap { entries: sorted_dedup_last(items) };
                        drop(it);
                        drop(built);
                        return None;
                    }
                    None => break,
                }
            }
            Some(BTreeMap { entries: sorted_dedup_last(items) })
        }
    }

    /// Std's `Extend` is an insert loop: a repeated key keeps the
    /// key already in the map and takes the new value, dropping
    /// the replaced values as it goes.
    impl<K: Ord, V> Extend<(K, V)> for BTreeMap<K, V> {
        fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
            for (k, v) in iter {
                self.insert(k, v);
            }
        }
    }

    impl<K: Ord, V, Q: ?Sized + Ord> core::ops::Index<&Q> for BTreeMap<K, V>
    where
        K: Borrow<Q>,
    {
        type Output = V;
        /// As std's: a missing key panics with `no entry found for key`,
        /// at the caller (core's `Index::index` is `#[track_caller]`).
        #[track_caller]
        fn index(&self, key: &Q) -> &V {
            match self.get(key) {
                Some(v) => v,
                None => panic!("no entry found for key"),
            }
        }
    }

    // No `IndexMut` for the map: std has none (assigning through
    // `map[&k]` is rustc E0594; `get_mut` and `entry` are the way).

    impl<K: Ord, V, const N: usize> From<[(K, V); N]> for BTreeMap<K, V> {
        /// As std's: the bulk build, so the last of equal keys stays.
        fn from(arr: [(K, V); N]) -> BTreeMap<K, V> {
            BTreeMap { entries: sorted_dedup_last(Vec::from(arr)) }
        }
    }

    /// Std's hash: the length, then every entry's key and value.
    impl<K: Hash, V: Hash> Hash for BTreeMap<K, V> {
        fn hash<H: Hasher>(&self, state: &mut H) {
            state.write_usize(self.len());
            for (k, v) in self.iter() {
                k.hash(state);
                v.hash(state);
            }
        }
    }

    impl<K, V> IntoIterator for BTreeMap<K, V> {
        type Item = (K, V);
        type IntoIter = IntoIter<K, V>;
        fn into_iter(self) -> IntoIter<K, V> {
            IntoIter { inner: self.entries.into_iter() }
        }
    }

    impl<'a, K, V> IntoIterator for &'a BTreeMap<K, V> {
        type Item = (&'a K, &'a V);
        type IntoIter = Iter<'a, K, V>;
        fn into_iter(self) -> Iter<'a, K, V> {
            self.iter()
        }
    }

    impl<'a, K, V> IntoIterator for &'a mut BTreeMap<K, V> {
        type Item = (&'a K, &'a mut V);
        type IntoIter = IterMut<'a, K, V>;
        fn into_iter(self) -> IterMut<'a, K, V> {
            self.iter_mut()
        }
    }
}

pub mod btree_set {
    use core::borrow::Borrow;
    use core::hash::{Hash, Hasher};

    pub use super::BTreeSet;

    pub struct Iter<'a, T> {
        inner: core::slice::Iter<'a, T>,
    }
    pub struct IntoIter<T> {
        inner: std::vec::IntoIter<T>,
    }

    impl<'a, T> Iterator for Iter<'a, T> {
        type Item = &'a T;
        fn next(&mut self) -> Option<&'a T> {
            self.inner.next()
        }
    }
    pub struct Range<'a, T> {
        inner: core::slice::Iter<'a, T>,
    }
    impl<'a, T> Iterator for Range<'a, T> {
        type Item = &'a T;
        fn next(&mut self) -> Option<&'a T> {
            self.inner.next()
        }
    }
    impl<T> Iterator for IntoIter<T> {
        type Item = T;
        fn next(&mut self) -> Option<T> {
            self.inner.next()
        }
    }

    impl<T> BTreeSet<T> {
        pub const fn new() -> BTreeSet<T> {
            BTreeSet { items: Vec::new() }
        }

        pub fn len(&self) -> usize {
            self.items.len()
        }

        pub fn is_empty(&self) -> bool {
            self.items.is_empty()
        }

        pub fn clear(&mut self) {
            self.items.clear();
        }

        pub fn iter(&self) -> Iter<'_, T> {
            Iter { inner: self.items.iter() }
        }

        pub fn first(&self) -> Option<&T> {
            self.items.first()
        }

        pub fn last(&self) -> Option<&T> {
            self.items.last()
        }

        pub fn pop_first(&mut self) -> Option<T> {
            if self.items.is_empty() { None } else { Some(self.items.remove(0)) }
        }

        pub fn pop_last(&mut self) -> Option<T> {
            self.items.pop()
        }

        pub fn retain<F: FnMut(&T) -> bool>(&mut self, mut f: F) {
            let mut i = 0;
            while i < self.items.len() {
                if f(&self.items[i]) {
                    i += 1;
                } else {
                    self.items.remove(i);
                }
            }
        }
    }

    impl<T: Ord> BTreeSet<T> {
        /// The index of `value`, or where it would go, by `T`'s own order.
        fn search_key(&self, value: &T) -> Result<usize, usize> {
            let mut lo = 0;
            let mut hi = self.items.len();
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                match super::cmp_of(&self.items[mid], value) {
                    core::cmp::Ordering::Less => lo = mid + 1,
                    core::cmp::Ordering::Greater => hi = mid,
                    core::cmp::Ordering::Equal => return Ok(mid),
                }
            }
            Err(lo)
        }

        fn search<Q: ?Sized + Ord>(&self, value: &Q) -> Result<usize, usize>
        where
            T: Borrow<Q>,
        {
            let mut lo = 0;
            let mut hi = self.items.len();
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                match super::cmp_of(self.items[mid].borrow(), value) {
                    core::cmp::Ordering::Less => lo = mid + 1,
                    core::cmp::Ordering::Greater => hi = mid,
                    core::cmp::Ordering::Equal => return Ok(mid),
                }
            }
            Err(lo)
        }

        /// Adds `value`; false (and `value` dropped) when an equal one is
        /// already there, which stays.
        pub fn insert(&mut self, value: T) -> bool {
            match self.search_key(&value) {
                Ok(_) => false,
                Err(i) => {
                    self.items.insert(i, value);
                    true
                }
            }
        }

        /// Adds `value`, replacing and returning an equal one.
        pub fn replace(&mut self, value: T) -> Option<T> {
            match self.search_key(&value) {
                Ok(i) => Some(core::mem::replace(&mut self.items[i], value)),
                Err(i) => {
                    self.items.insert(i, value);
                    None
                }
            }
        }

        pub fn contains<Q: ?Sized + Ord>(&self, value: &Q) -> bool
        where
            T: Borrow<Q>,
        {
            match self.search(value) {
                Ok(_) => true,
                Err(_) => false,
            }
        }

        pub fn get<Q: ?Sized + Ord>(&self, value: &Q) -> Option<&T>
        where
            T: Borrow<Q>,
        {
            match self.search(value) {
                Ok(i) => Some(&self.items[i]),
                Err(_) => None,
            }
        }

        pub fn remove<Q: ?Sized + Ord>(&mut self, value: &Q) -> bool
        where
            T: Borrow<Q>,
        {
            match self.search(value) {
                Ok(i) => {
                    self.items.remove(i);
                    true
                }
                Err(_) => false,
            }
        }

        pub fn take<Q: ?Sized + Ord>(&mut self, value: &Q) -> Option<T>
        where
            T: Borrow<Q>,
        {
            match self.search(value) {
                Ok(i) => Some(self.items.remove(i)),
                Err(_) => None,
            }
        }

        /// Std's merge: on an equal value `other`'s stays and this set's
        /// drops.
        pub fn append(&mut self, other: &mut BTreeSet<T>) {
            if other.items.is_empty() {
                return;
            }
            if self.items.is_empty() {
                core::mem::swap(&mut self.items, &mut other.items);
                return;
            }
            let left = core::mem::replace(&mut self.items, Vec::new());
            let right = core::mem::replace(&mut other.items, Vec::new());
            let mut out: Vec<T> = Vec::new();
            let mut a = left.into_iter();
            let mut b = right.into_iter();
            let mut na = a.next();
            let mut nb = b.next();
            loop {
                let ord = if let Some(x) = &na {
                    if let Some(y) = &nb { super::cmp_of(x, y) } else { core::cmp::Ordering::Less }
                } else if nb.is_some() {
                    core::cmp::Ordering::Greater
                } else {
                    break
                };
                match ord {
                    core::cmp::Ordering::Less => {
                        out.push(core::mem::replace(&mut na, None).unwrap()); //@ never
                        na = a.next();
                    }
                    core::cmp::Ordering::Greater => {
                        out.push(core::mem::replace(&mut nb, None).unwrap()); //@ never
                        nb = b.next();
                    }
                    core::cmp::Ordering::Equal => {
                        let dropped = core::mem::replace(&mut na, None);
                        drop(dropped);
                        out.push(core::mem::replace(&mut nb, None).unwrap()); //@ never
                        na = a.next();
                        nb = b.next();
                    }
                }
            }
            self.items = out;
        }

        pub fn split_off<Q: ?Sized + Ord>(&mut self, value: &Q) -> BTreeSet<T>
        where
            T: Borrow<Q>,
        {
            let at = match self.search(value) {
                Ok(i) => i,
                Err(i) => i,
            };
            BTreeSet { items: self.items.split_off(at) }
        }

        pub fn is_subset(&self, other: &BTreeSet<T>) -> bool {
            for v in &self.items {
                if let Err(_) = other.search_key(v) {
                    return false;
                }
            }
            true
        }

        pub fn is_superset(&self, other: &BTreeSet<T>) -> bool {
            other.is_subset(self)
        }

        pub fn is_disjoint(&self, other: &BTreeSet<T>) -> bool {
            for v in &self.items {
                if let Ok(_) = other.search_key(v) {
                    return false;
                }
            }
            true
        }
    }

    impl<T: Ord> BTreeSet<T> {
        /// The element span of a range, with std's panic checks
        /// (BTreeSet texts). An empty set checks nothing (see BTreeMap's).
        fn range_span<Q: ?Sized + Ord>(&self, start: core::ops::Bound<&Q>, end: core::ops::Bound<&Q>) -> (usize, usize)
        where
            T: Borrow<Q>,
        {
            use core::ops::Bound;
            if self.items.is_empty() {
                return (0, 0);
            }
            let bounded = match (&start, &end) {
                (Bound::Excluded(s), Bound::Excluded(e)) => {
                    if super::cmp_of(*s, *e) == core::cmp::Ordering::Equal {
                        panic!("range start and end are equal and excluded in BTreeSet"); //@ alloc/src/collections/btree/search.rs:110:21
                    }
                    Some((s, e))
                }
                (Bound::Included(s), Bound::Included(e)) => Some((s, e)),
                (Bound::Included(s), Bound::Excluded(e)) => Some((s, e)),
                (Bound::Excluded(s), Bound::Included(e)) => Some((s, e)),
                _ => None,
            };
            if let Some((s, e)) = bounded
                && super::cmp_of(*s, *e) == core::cmp::Ordering::Greater
            {
                panic!("range start is greater than range end in BTreeSet"); //@ alloc/src/collections/btree/search.rs:119:21
            }
            let lo = match &start {
                Bound::Included(x) => match self.search(*x) {
                    Ok(i) => i,
                    Err(i) => i,
                },
                Bound::Excluded(x) => match self.search(*x) {
                    Ok(i) => i + 1,
                    Err(i) => i,
                },
                Bound::Unbounded => 0,
            };
            let hi = match &end {
                Bound::Included(x) => match self.search(*x) {
                    Ok(i) => i + 1,
                    Err(i) => i,
                },
                Bound::Excluded(x) => match self.search(*x) {
                    Ok(i) => i,
                    Err(i) => i,
                },
                Bound::Unbounded => self.items.len(),
            };
            (lo, hi)
        }

        /// `range` with its bounds already split (see BTreeMap's).
        fn range_between<Q: ?Sized + Ord>(&self, start: core::ops::Bound<&Q>, end: core::ops::Bound<&Q>) -> Range<'_, T>
        where
            T: Borrow<Q>,
        {
            let (lo, hi) = self.range_span(start, end);
            let slice: &[T] = &self.items[lo..hi];
            Range { inner: slice.iter() }
        }
    }

    impl<T: Ord> BTreeSet<T> {
    }

    impl<T: Ord> BTreeSet<T> {
        pub fn range<Q: ?Sized + Ord, R: core::ops::RangeBounds<Q>>(&self, range: R) -> Range<'_, T>
        where
            T: Borrow<Q>,
        {
            self.range_between(range.start_bound(), range.end_bound())
        }
    }

    impl<T> Default for BTreeSet<T> {
        fn default() -> BTreeSet<T> {
            BTreeSet::new()
        }
    }

    impl<T: Clone> Clone for BTreeSet<T> {
        fn clone(&self) -> BTreeSet<T> {
            BTreeSet { items: self.items.clone() }
        }
    }

    impl<T: PartialEq> PartialEq for BTreeSet<T> {
        fn eq(&self, other: &BTreeSet<T>) -> bool {
            self.items == other.items
        }
    }

    impl<T: Eq> Eq for BTreeSet<T> {}

    impl<T: core::fmt::Debug> core::fmt::Debug for BTreeSet<T> {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let mut s = f.debug_set();
            for v in &self.items {
                s.entry(v);
            }
            s.finish()
        }
    }

    impl<T: Ord> FromIterator<T> for BTreeSet<T> {
        fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> BTreeSet<T> {
            let mut items: Vec<T> = Vec::new();
            for v in iter {
                items.push(v);
            }
            BTreeSet { items: sorted_dedup_last_set(items) }
        }
    }

    // Core's generic try collects over this set (F3), written out
    // as the map's are (see `btree_map`): bulk-build the success
    // prefix, and on a failure drop the source iterator before
    // the built prefix.
    impl<T: Ord, E> FromIterator<Result<T, E>> for Result<BTreeSet<T>, E> {
        fn from_iter<I: IntoIterator<Item = Result<T, E>>>(iter: I) -> Result<BTreeSet<T>, E> {
            let mut items: Vec<T> = Vec::new();
            let mut it = iter.into_iter();
            loop {
                match it.next() {
                    Some(Ok(v)) => items.push(v),
                    Some(Err(e)) => {
                        let built = BTreeSet { items: sorted_dedup_last_set(items) };
                        drop(it);
                        drop(built);
                        return Err(e);
                    }
                    None => break,
                }
            }
            Ok(BTreeSet { items: sorted_dedup_last_set(items) })
        }
    }

    impl<T: Ord> FromIterator<Option<T>> for Option<BTreeSet<T>> {
        fn from_iter<I: IntoIterator<Item = Option<T>>>(iter: I) -> Option<BTreeSet<T>> {
            let mut items: Vec<T> = Vec::new();
            let mut it = iter.into_iter();
            loop {
                match it.next() {
                    Some(Some(v)) => items.push(v),
                    Some(None) => {
                        let built = BTreeSet { items: sorted_dedup_last_set(items) };
                        drop(it);
                        drop(built);
                        return None;
                    }
                    None => break,
                }
            }
            Some(BTreeSet { items: sorted_dedup_last_set(items) })
        }
    }

    /// Std's `Extend` is an insert loop: an equal value already in
    /// the set stays, the new one drops.
    impl<T: Ord> Extend<T> for BTreeSet<T> {
        fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
            for v in iter {
                self.insert(v);
            }
        }
    }

    /// Std's bulk build for the set: stable sort, the last of
    /// equal values stays.
    fn sorted_dedup_last_set<T: Ord>(mut items: Vec<T>) -> Vec<T> {
        let mut i = 1;
        while i < items.len() {
            let mut j = i;
            while j > 0 && super::cmp_of(&items[j - 1], &items[j]) == core::cmp::Ordering::Greater {
                items.swap(j - 1, j);
                j -= 1;
            }
            i += 1;
        }
        let mut out: Vec<T> = Vec::new();
        for item in items {
            let dup = match out.last() {
                Some(last) => super::cmp_of(last, &item) == core::cmp::Ordering::Equal,
                None => false,
            };
            if dup {
                out.pop();
            }
            out.push(item);
        }
        out
    }

    impl<T: Ord, const N: usize> From<[T; N]> for BTreeSet<T> {
        fn from(arr: [T; N]) -> BTreeSet<T> {
            BTreeSet { items: sorted_dedup_last_set(Vec::from(arr)) }
        }
    }

    /// Std's hash: the length, then every element.
    impl<T: Hash> Hash for BTreeSet<T> {
        fn hash<H: Hasher>(&self, state: &mut H) {
            state.write_usize(self.len());
            for elt in self.iter() {
                elt.hash(state);
            }
        }
    }

    impl<T> IntoIterator for BTreeSet<T> {
        type Item = T;
        type IntoIter = IntoIter<T>;
        fn into_iter(self) -> IntoIter<T> {
            IntoIter { inner: self.items.into_iter() }
        }
    }

    impl<'a, T> IntoIterator for &'a BTreeSet<T> {
        type Item = &'a T;
        type IntoIter = Iter<'a, T>;
        fn into_iter(self) -> Iter<'a, T> {
            Iter { inner: self.items.iter() }
        }
    }
}

pub mod vec_deque {
    use core::hash::{Hash, Hasher};

    pub use super::VecDeque;

    pub struct Iter<'a, T> {
        inner: core::slice::Iter<'a, T>,
    }
    pub struct IterMut<'a, T> {
        inner: core::slice::IterMut<'a, T>,
    }
    pub struct IntoIter<T> {
        inner: std::vec::IntoIter<T>,
    }

    impl<'a, T> Iterator for Iter<'a, T> {
        type Item = &'a T;
        fn next(&mut self) -> Option<&'a T> {
            self.inner.next()
        }
    }
    impl<'a, T> Iterator for IterMut<'a, T> {
        type Item = &'a mut T;
        fn next(&mut self) -> Option<&'a mut T> {
            self.inner.next()
        }
    }
    impl<T> Iterator for IntoIter<T> {
        type Item = T;
        fn next(&mut self) -> Option<T> {
            self.inner.next()
        }
    }

    impl<T> VecDeque<T> {
        pub const fn new() -> VecDeque<T> {
            VecDeque { buf: Vec::new(), head: 0 }
        }

        pub fn with_capacity(capacity: usize) -> VecDeque<T> {
            VecDeque { buf: Vec::with_capacity(capacity), head: 0 }
        }

        pub fn len(&self) -> usize {
            self.buf.len()
        }

        pub fn is_empty(&self) -> bool {
            self.buf.is_empty()
        }

        pub fn capacity(&self) -> usize {
            self.buf.capacity()
        }

        pub fn clear(&mut self) {
            self.buf.clear();
            self.head = 0;
        }

        /// The physical index one past the front, in a buffer of
        /// the current capacity (which a grow has just settled).
        fn wrap_next(&self, head: usize) -> usize {
            if head + 1 >= self.buf.capacity() { 0 } else { head + 1 }
        }

        /// The physical index one before the front, likewise.
        fn wrap_prev(&self, head: usize) -> usize {
            if head == 0 { self.buf.capacity() - 1 } else { head - 1 }
        }

        /// The state a capacity growth copies from: the capacity
        /// and first-slice length before the growing operation.
        fn pre_grow(&self) -> (usize, usize) {
            (self.buf.capacity(), self.first_len())
        }

        /// Where the ring head lands after the buffer grew from
        /// the `pre_grow` state: std copies the smaller of the
        /// two segments to the far end, so when the back segment
        /// (from the head) is the smaller or equal one it ends
        /// flush with the new buffer's end; otherwise the head
        /// keeps its offset.
        fn note_grow(&mut self, pre: (usize, usize), len_before: usize) {
            let (cap_before, first_before) = pre;
            let cap = self.buf.capacity();
            if cap > cap_before && first_before < len_before && first_before <= len_before - first_before {
                self.head = cap - first_before;
            }
        }

        /// How many elements the first of the two ring slices holds.
        fn first_len(&self) -> usize {
            let cap = self.buf.capacity();
            if cap == 0 {
                0
            } else if self.head + self.buf.len() <= cap {
                self.buf.len()
            } else {
                cap - self.head
            }
        }

        pub fn as_slices(&self) -> (&[T], &[T]) {
            let first = self.first_len();
            (&self.buf[..first], &self.buf[first..])
        }

        // No `as_mut_slices`: it needs `split_at_mut`, which the
        // subset has no form of yet.

        pub fn make_contiguous(&mut self) -> &mut [T] {
            let cap = self.buf.capacity();
            let len = self.buf.len();
            if cap > 0 && self.head + len > cap {
                let free = cap - len;
                let left = cap - self.head;
                let right = len - left;
                self.head = if free >= left || left <= right {
                    0
                } else if right < free {
                    right
                } else {
                    free
                };
            }
            self.buf.as_mut_slice()
        }

        /// Reverses `buf[lo..hi]` in place (for the rotates).
        fn reverse_range(&mut self, mut lo: usize, mut hi: usize) {
            while lo + 1 < hi {
                hi -= 1;
                self.buf.swap(lo, hi);
                lo += 1;
            }
        }

        /// Logically rotates the buffer left by `k` (three reversals).
        fn rotate_buf_left(&mut self, k: usize) {
            let len = self.buf.len();
            if k == 0 || k >= len {
                return;
            }
            self.reverse_range(0, k);
            self.reverse_range(k, len);
            self.reverse_range(0, len);
        }

        pub fn rotate_left(&mut self, n: usize) {
            assert!(n <= self.len()); //@ alloc/src/collections/vec_deque/mod.rs:3137:9
            let len = self.buf.len();
            if len <= 1 || n == 0 || n == len {
                return;
            }
            if n <= len / 2 {
                let mut h = self.head;
                let mut i = 0;
                while i < n {
                    h = self.wrap_next(h);
                    i += 1;
                }
                self.head = h;
            } else {
                let mut h = self.head;
                let mut i = 0;
                while i < len - n {
                    h = self.wrap_prev(h);
                    i += 1;
                }
                self.head = h;
            }
            self.rotate_buf_left(n);
        }

        pub fn rotate_right(&mut self, n: usize) {
            assert!(n <= self.len()); //@ alloc/src/collections/vec_deque/mod.rs:3180:9
            let len = self.buf.len();
            if len <= 1 || n == 0 || n == len {
                return;
            }
            if n <= len / 2 {
                let mut h = self.head;
                let mut i = 0;
                while i < n {
                    h = self.wrap_prev(h);
                    i += 1;
                }
                self.head = h;
            } else {
                let mut h = self.head;
                let mut i = 0;
                while i < len - n {
                    h = self.wrap_next(h);
                    i += 1;
                }
                self.head = h;
            }
            self.rotate_buf_left(len - n);
        }

        pub fn push_back(&mut self, value: T) {
            let pre = self.pre_grow();
            let len = self.buf.len();
            self.buf.push(value);
            self.note_grow(pre, len);
        }

        pub fn push_front(&mut self, value: T) {
            let pre = self.pre_grow();
            let len = self.buf.len();
            self.buf.insert(0, value);
            self.note_grow(pre, len);
            self.head = self.wrap_prev(self.head);
        }

        pub fn pop_back(&mut self) -> Option<T> {
            self.buf.pop()
        }

        pub fn pop_front(&mut self) -> Option<T> {
            if self.buf.is_empty() {
                None
            } else {
                self.head = self.wrap_next(self.head);
                Some(self.buf.remove(0))
            }
        }

        pub fn front(&self) -> Option<&T> {
            self.buf.first()
        }

        pub fn back(&self) -> Option<&T> {
            self.buf.last()
        }

        pub fn front_mut(&mut self) -> Option<&mut T> {
            self.buf.first_mut()
        }

        pub fn back_mut(&mut self) -> Option<&mut T> {
            self.buf.last_mut()
        }

        pub fn get(&self, index: usize) -> Option<&T> {
            self.buf.get(index)
        }

        pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
            self.buf.get_mut(index)
        }

        pub fn insert(&mut self, index: usize, value: T) {
            let len = self.buf.len();
            if index > len {
                panic!("index out of bounds"); //@ alloc/src/collections/vec_deque/mod.rs:2564:9
            }
            let pre = self.pre_grow();
            self.buf.insert(index, value);
            self.note_grow(pre, len);
            // Std shifts the shorter side: the front part moves
            // when `2 * index <= len` (the head steps back).
            if index * 2 <= len {
                self.head = self.wrap_prev(self.head);
            }
        }

        pub fn remove(&mut self, index: usize) -> Option<T> {
            let len = self.buf.len();
            if index >= len {
                return None;
            }
            let out = self.buf.remove(index);
            // Std shifts the shorter side: the front part moves
            // when `2 * index < len` (the head steps forward).
            if index * 2 < len {
                self.head = self.wrap_next(self.head);
            }
            Some(out)
        }

        pub fn swap(&mut self, i: usize, j: usize) {
            assert!(i < self.len()); //@ alloc/src/collections/vec_deque/mod.rs:1030:9
            assert!(j < self.len()); //@ alloc/src/collections/vec_deque/mod.rs:1031:9
            self.buf.swap(i, j);
        }

        pub fn truncate(&mut self, len: usize) {
            self.buf.truncate(len);
        }

        pub fn reserve(&mut self, additional: usize) {
            self.buf.len().checked_add(additional).expect("capacity overflow"); //@ alloc/src/collections/vec_deque/mod.rs:1108:56
            let pre = self.pre_grow();
            let len = self.buf.len();
            self.buf.reserve(additional);
            self.note_grow(pre, len);
        }

        pub fn reserve_exact(&mut self, additional: usize) {
            self.buf.len().checked_add(additional).expect("capacity overflow"); //@ alloc/src/collections/vec_deque/mod.rs:1078:56
            let pre = self.pre_grow();
            let len = self.buf.len();
            self.buf.reserve_exact(additional);
            self.note_grow(pre, len);
        }

        pub fn shrink_to_fit(&mut self) {
            self.shrink_head(self.buf.len());
            self.buf.shrink_to_fit();
        }

        pub fn shrink_to(&mut self, min_capacity: usize) {
            let len = self.buf.len();
            let target = if min_capacity > len { min_capacity } else { len };
            self.shrink_head(target);
            self.buf.shrink_to(target);
        }

        /// Where the ring head lands when the buffer shrinks to
        /// `target` slots: a wrapped layout keeps its distance
        /// from the buffer's end, an unwrapped one keeps its head
        /// when it still fits, else the front.
        fn shrink_head(&mut self, target: usize) {
            let cap = self.buf.capacity();
            let len = self.buf.len();
            if target >= cap {
                return;
            }
            self.head = if self.head + len > cap {
                self.head - (cap - target)
            } else if self.head < target {
                self.head
            } else {
                0
            };
        }

        pub fn iter(&self) -> Iter<'_, T> {
            Iter { inner: self.buf.iter() }
        }

        pub fn iter_mut(&mut self) -> IterMut<'_, T> {
            IterMut { inner: self.buf.iter_mut() }
        }

        pub fn contains(&self, x: &T) -> bool
        where
            T: PartialEq,
        {
            self.buf.contains(x)
        }

        /// Std's algorithm, for its drop order: kept elements swap to
        /// the front, and one `truncate` drops the rest at the end, in
        /// their order after the swaps.
        pub fn retain<F: FnMut(&T) -> bool>(&mut self, mut f: F) {
            let len = self.buf.len();
            let mut idx = 0;
            let mut cur = 0;
            while cur < len {
                if !f(&self.buf[cur]) {
                    cur += 1;
                    break;
                }
                cur += 1;
                idx += 1;
            }
            while cur < len {
                if !f(&self.buf[cur]) {
                    cur += 1;
                    continue;
                }
                self.buf.swap(idx, cur);
                cur += 1;
                idx += 1;
            }
            if cur != idx {
                self.buf.truncate(idx);
            }
        }

        pub fn append(&mut self, other: &mut VecDeque<T>) {
            let pre = self.pre_grow();
            let len = self.buf.len();
            self.buf.append(&mut other.buf);
            self.note_grow(pre, len);
            // std empties `other` with `len = 0, head = 0`.
            other.head = 0;
        }

        pub fn split_off(&mut self, at: usize) -> VecDeque<T> {
            let len = self.buf.len();
            assert!(at <= len, "`at` out of bounds"); //@ alloc/src/collections/vec_deque/mod.rs:2672:9
            VecDeque { buf: self.buf.split_off(at), head: 0 }
        }

        pub fn swap_remove_back(&mut self, index: usize) -> Option<T> {
            if index < self.buf.len() { Some(self.buf.swap_remove(index)) } else { None }
        }

        pub fn drain<R: core::ops::RangeBounds<usize>>(&mut self, range: R) -> Drain<'_, T> {
            use core::ops::Bound;
            // The ring head after the drain finishes (consumed or
            // dropped): std closes the gap from the shorter side,
            // and a full drain resets it. The bounds are read
            // here only for the ghost head; `buf.drain` panics on
            // a bad range before the head can be observed.
            let len = self.buf.len();
            let start = match range.start_bound() {
                Bound::Included(&n) => n,
                Bound::Excluded(&n) => n.wrapping_add(1),
                Bound::Unbounded => 0,
            };
            let end = match range.end_bound() {
                Bound::Included(&n) => n.wrapping_add(1),
                Bound::Excluded(&n) => n,
                Bound::Unbounded => len,
            };
            if end >= start && end <= len {
                let removed = end - start;
                if removed >= len {
                    self.head = 0;
                } else if start < len - end {
                    let mut h = self.head;
                    let mut i = 0;
                    while i < removed {
                        h = self.wrap_next(h);
                        i += 1;
                    }
                    self.head = h;
                }
            }
            self.buf.drain(range)
        }

        pub fn swap_remove_front(&mut self, index: usize) -> Option<T> {
            if index >= self.buf.len() {
                return None;
            }
            self.buf.swap(0, index);
            self.head = self.wrap_next(self.head);
            Some(self.buf.remove(0))
        }
    }

    pub type Drain<'a, T> = std::vec::Drain<'a, T>;

    impl<T> Default for VecDeque<T> {
        fn default() -> VecDeque<T> {
            VecDeque::new()
        }
    }

    impl<T: Clone> Clone for VecDeque<T> {
        fn clone(&self) -> VecDeque<T> {
            VecDeque { buf: self.buf.clone(), head: 0 }
        }
    }

    impl<T: PartialEq> PartialEq for VecDeque<T> {
        fn eq(&self, other: &VecDeque<T>) -> bool {
            self.buf == other.buf
        }
    }

    impl<T: Eq> Eq for VecDeque<T> {}

    impl<T: core::fmt::Debug> core::fmt::Debug for VecDeque<T> {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            let mut l = f.debug_list();
            for v in &self.buf {
                l.entry(v);
            }
            l.finish()
        }
    }

    impl<T> From<Vec<T>> for VecDeque<T> {
        fn from(buf: Vec<T>) -> VecDeque<T> {
            VecDeque { buf, head: 0 }
        }
    }

    // No `FromIterator` impl here: `collect` into a `VecDeque`
    // takes the compiler's `Vec` collect (check/iter.rs), whose
    // capacity is std's for a deque too; a source `from_iter`
    // body could not state that capacity (the trusted-length
    // exact reservation). A direct `VecDeque::from_iter(..)`
    // call takes the same builtin (check/iter.rs, fx-fromiter).

    /// Std's extend: reserve the size hint's lower bound once,
    /// then push at the back.
    impl<T> Extend<T> for VecDeque<T> {
        fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
            let iter = iter.into_iter();
            let (lower, _) = iter.size_hint();
            self.reserve(lower);
            for x in iter {
                self.buf.push(x);
            }
        }
    }

    // The reference-item `Extend` impls of std (`Extend<&T>`,
    // `Extend<(&K, &V)>`) are not written here: with two `Extend`
    // impls for one type, method selection cannot pick between
    // them yet (the input type is what the argument decides), so
    // every `extend` call would miss. They land with that fix.

    impl<T> core::ops::Index<usize> for VecDeque<T> {
        type Output = T;
        #[track_caller]
        fn index(&self, index: usize) -> &T {
            self.get(index).expect("out of bounds access")
        }
    }

    impl<T> core::ops::IndexMut<usize> for VecDeque<T> {
        #[track_caller]
        fn index_mut(&mut self, index: usize) -> &mut T {
            self.get_mut(index).expect("out of bounds access")
        }
    }

    impl<T, const N: usize> From<[T; N]> for VecDeque<T> {
        fn from(arr: [T; N]) -> VecDeque<T> {
            VecDeque { buf: Vec::from(arr), head: 0 }
        }
    }

    /// Std's hash: the length, then every element in order.
    impl<T: Hash> Hash for VecDeque<T> {
        fn hash<H: Hasher>(&self, state: &mut H) {
            state.write_usize(self.len());
            for elt in self.iter() {
                elt.hash(state);
            }
        }
    }

    impl<T> IntoIterator for VecDeque<T> {
        type Item = T;
        type IntoIter = IntoIter<T>;
        fn into_iter(self) -> IntoIter<T> {
            IntoIter { inner: self.buf.into_iter() }
        }
    }

    impl<'a, T> IntoIterator for &'a VecDeque<T> {
        type Item = &'a T;
        type IntoIter = Iter<'a, T>;
        fn into_iter(self) -> Iter<'a, T> {
            Iter { inner: self.buf.iter() }
        }
    }

    impl<'a, T> IntoIterator for &'a mut VecDeque<T> {
        type Item = &'a mut T;
        type IntoIter = IterMut<'a, T>;
        fn into_iter(self) -> IterMut<'a, T> {
            IterMut { inner: self.buf.iter_mut() }
        }
    }
}

/// A max-heap. `data` is std's heap array: the same sift rules
/// (`sift_up`, `sift_down_to_bottom` on pop, `sift_down_range` when
/// rebuilt from a `Vec`), so `iter`, `into_vec`, `as_slice` and `Debug`
/// see std's element order. Swaps stand in for std's `Hole` moves;
/// both leave every element where std leaves it.
pub struct BinaryHeap<T> {
    data: Vec<T>,
}

pub mod binary_heap {
    pub use super::BinaryHeap;

    impl<T: Ord> BinaryHeap<T> {
        pub fn new() -> BinaryHeap<T> {
            BinaryHeap { data: Vec::new() }
        }

        pub fn with_capacity(capacity: usize) -> BinaryHeap<T> {
            BinaryHeap { data: Vec::with_capacity(capacity) }
        }

        pub fn push(&mut self, item: T) {
            let old_len = self.data.len();
            self.data.push(item);
            self.sift_up(0, old_len);
        }

        pub fn pop(&mut self) -> Option<T> {
            match self.data.pop() {
                None => None,
                Some(mut item) => {
                    if !self.data.is_empty() {
                        core::mem::swap(&mut item, &mut self.data[0]);
                        self.sift_down_to_bottom(0);
                    }
                    Some(item)
                }
            }
        }

        pub fn peek(&self) -> Option<&T> {
            self.data.first()
        }

        pub fn into_sorted_vec(mut self) -> Vec<T> {
            let mut end = self.data.len();
            while end > 1 {
                end -= 1;
                self.data.swap(0, end);
                self.sift_down_range(0, end);
            }
            self.data
        }

        fn sift_up(&mut self, start: usize, pos: usize) -> usize {
            let mut pos = pos;
            while pos > start {
                let parent = (pos - 1) / 2;
                if self.data[pos] <= self.data[parent] {
                    break;
                }
                self.data.swap(pos, parent);
                pos = parent;
            }
            pos
        }

        fn sift_down_range(&mut self, pos: usize, end: usize) {
            let mut pos = pos;
            let mut child = 2 * pos + 1;
            while end >= 2 && child <= end - 2 {
                if self.data[child] <= self.data[child + 1] {
                    child += 1;
                }
                if self.data[pos] >= self.data[child] {
                    return;
                }
                self.data.swap(pos, child);
                pos = child;
                child = 2 * pos + 1;
            }
            if end >= 1 && child == end - 1 && self.data[pos] < self.data[child] {
                self.data.swap(pos, child);
            }
        }

        fn sift_down_to_bottom(&mut self, pos: usize) {
            let end = self.data.len();
            let start = pos;
            let mut pos = pos;
            let mut child = 2 * pos + 1;
            while end >= 2 && child <= end - 2 {
                if self.data[child] <= self.data[child + 1] {
                    child += 1;
                }
                self.data.swap(pos, child);
                pos = child;
                child = 2 * pos + 1;
            }
            if end >= 1 && child == end - 1 {
                self.data.swap(pos, child);
                pos = child;
            }
            self.sift_up(start, pos);
        }

        fn rebuild(&mut self) {
            let mut n = self.data.len() / 2;
            while n > 0 {
                n -= 1;
                let len = self.data.len();
                self.sift_down_range(n, len);
            }
        }
    }

    impl<T> BinaryHeap<T> {
        pub fn len(&self) -> usize {
            self.data.len()
        }

        pub fn is_empty(&self) -> bool {
            self.data.is_empty()
        }

        pub fn capacity(&self) -> usize {
            self.data.capacity()
        }

        pub fn clear(&mut self) {
            self.data.clear();
        }

        pub fn into_vec(self) -> Vec<T> {
            self.data
        }

        pub fn as_slice(&self) -> &[T] {
            self.data.as_slice()
        }
    }

    impl<T: Ord> From<Vec<T>> for BinaryHeap<T> {
        fn from(vec: Vec<T>) -> BinaryHeap<T> {
            let mut heap = BinaryHeap { data: vec };
            heap.rebuild();
            heap
        }
    }

    impl<T: Ord> Default for BinaryHeap<T> {
        fn default() -> BinaryHeap<T> {
            BinaryHeap::new()
        }
    }

    impl<T: Clone> Clone for BinaryHeap<T> {
        fn clone(&self) -> BinaryHeap<T> {
            BinaryHeap { data: self.data.clone() }
        }
    }

    impl<T: core::fmt::Debug> core::fmt::Debug for BinaryHeap<T> {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_list().entries(self.data.iter()).finish()
        }
    }
}
