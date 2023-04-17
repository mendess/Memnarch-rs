use std::{borrow::Borrow, marker::PhantomData, ops::Deref};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JsonHashMap<K, V>(Vec<(K, V)>);

impl<K, V> Deref for JsonHashMap<K, V> {
    type Target = [(K, V)];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<K, V> Default for JsonHashMap<K, V> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<K, V> JsonHashMap<K, V> {
    pub fn new() -> Self {
        Default::default()
    }
}

impl<K, V> JsonHashMap<K, V>
where
    K: PartialEq,
{
    pub fn insert(&mut self, key: K, value: V) -> bool {
        if let Some(v) = self.0.iter_mut().find(|(k, _)| k == &key) {
            v.1 = value;
            false
        } else {
            self.0.push((key, value));
            true
        }
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.0.iter().any(|(k, _)| k == key)
    }

    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: PartialEq<Q>,
    {
        self.0.iter().find_map(|(k, v)| (k == key).then_some(v))
    }

    pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
    where
        K: PartialEq<Q>,
    {
        let key = key.borrow();
        self.0.iter_mut().find_map(|(k, v)| (k == key).then_some(v))
    }

    pub fn entry(&'_ mut self, key: K) -> Entry<'_, K, V> {
        if let Some(v) = self.get_mut(&key) {
            // SAFETY: The borrow checker extends this borrow to long and the else branch doesn't
            // compile because it thinks I'm borrowing self twice. The branches are exclusive so
            // this not the case.
            let v = unsafe { &mut *(v as *mut V) };
            Entry::Occupied(OccupiedEntry(v, PhantomData))
        } else {
            Entry::Vacant(VacantEntry { map: self, key })
        }
    }
}

pub enum Entry<'a, K: 'a, V: 'a> {
    /// An occupied entry.
    Occupied(OccupiedEntry<'a, K, V>),

    /// A vacant entry.
    Vacant(VacantEntry<'a, K, V>),
}

impl<'a, K: 'a, V: 'a> Entry<'a, K, V> {
    pub fn and_modify<F>(mut self, f: F) -> Self
    where
        F: FnOnce(&mut V),
    {
        if let Entry::Occupied(o) = &mut self {
            f(o.0)
        }
        self
    }
}

impl<'a, K: 'a, V: 'a> Entry<'a, K, V>
where
    K: PartialEq,
{
    pub fn or_insert_with<F: FnOnce() -> V>(self, default: F) -> &'a mut V {
        match self {
            Entry::Occupied(o) => o.0,
            Entry::Vacant(v) => &mut v.insert(default()).1,
        }
    }
}

pub struct OccupiedEntry<'a, K, V>(&'a mut V, PhantomData<K>);

pub struct VacantEntry<'a, K, V> {
    map: &'a mut JsonHashMap<K, V>,
    key: K,
}

impl<'a, K: PartialEq, V> VacantEntry<'a, K, V> {
    pub fn insert(self, value: V) -> &'a mut (K, V) {
        self.map.insert(self.key, value);
        self.map.0.last_mut().unwrap()
    }
}
