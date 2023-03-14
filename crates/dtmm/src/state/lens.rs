use std::sync::Arc;

use druid::im::Vector;
use druid::{Data, Lens};

use super::{ModInfo, NexusInfo, State};

pub(crate) struct SelectedModLens;

impl Lens<State, Option<Arc<ModInfo>>> for SelectedModLens {
    #[tracing::instrument(name = "SelectedModLens::with", skip_all)]
    fn with<V, F: FnOnce(&Option<Arc<ModInfo>>) -> V>(&self, data: &State, f: F) -> V {
        let info = data
            .selected_mod_index
            .and_then(|i| data.mods.get(i).cloned());

        f(&info)
    }

    #[tracing::instrument(name = "SelectedModLens::with_mut", skip_all)]
    fn with_mut<V, F: FnOnce(&mut Option<Arc<ModInfo>>) -> V>(&self, data: &mut State, f: F) -> V {
        match data.selected_mod_index {
            Some(i) => {
                let mut info = data.mods.get_mut(i).cloned();
                let ret = f(&mut info);

                if let Some(new) = info {
                    // TODO: Figure out a way to check for equality and
                    // only update when needed
                    data.mods.set(i, new);
                } else {
                    data.selected_mod_index = None;
                }

                ret
            }
            None => f(&mut None),
        }
    }
}

/// A Lens that maps an `im::Vector<T>` to `im::Vector<(usize, T)>`,
/// where each element in the destination vector includes its index in the
/// source vector.
pub(crate) struct IndexedVectorLens;

impl<T: Data> Lens<Vector<T>, Vector<(usize, T)>> for IndexedVectorLens {
    #[tracing::instrument(name = "IndexedVectorLens::with", skip_all)]
    fn with<V, F: FnOnce(&Vector<(usize, T)>) -> V>(&self, values: &Vector<T>, f: F) -> V {
        let indexed = values
            .iter()
            .enumerate()
            .map(|(i, val)| (i, val.clone()))
            .collect();
        f(&indexed)
    }

    #[tracing::instrument(name = "IndexedVectorLens::with_mut", skip_all)]
    fn with_mut<V, F: FnOnce(&mut Vector<(usize, T)>) -> V>(
        &self,
        values: &mut Vector<T>,
        f: F,
    ) -> V {
        let mut indexed = values
            .iter()
            .enumerate()
            .map(|(i, val)| (i, val.clone()))
            .collect();
        let ret = f(&mut indexed);

        *values = indexed.into_iter().map(|(_i, val)| val).collect();

        ret
    }
}

/// A Lens that first checks a key in a mod's `NexusInfo`, then falls back to
/// the regular one.
pub(crate) struct NexusInfoLens<T, L, R>
where
    L: Lens<NexusInfo, T>,
    R: Lens<ModInfo, T>,
{
    value: L,
    fallback: R,
    _marker: std::marker::PhantomData<T>,
}

impl<T: Data, L, R> NexusInfoLens<T, L, R>
where
    L: Lens<NexusInfo, T>,
    R: Lens<ModInfo, T>,
{
    pub fn new(value: L, fallback: R) -> Self {
        Self {
            value,
            fallback,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T: Data, L, R> Lens<ModInfo, T> for NexusInfoLens<T, L, R>
where
    L: Lens<NexusInfo, T>,
    R: Lens<ModInfo, T>,
{
    fn with<V, F: FnOnce(&T) -> V>(&self, data: &ModInfo, f: F) -> V {
        if let Some(nexus) = &data.nexus {
            self.value.with(nexus, f)
        } else {
            self.fallback.with(data, f)
        }
    }

    fn with_mut<V, F: FnOnce(&mut T) -> V>(&self, data: &mut ModInfo, f: F) -> V {
        if let Some(nexus) = &mut data.nexus {
            self.value.with_mut(nexus, f)
        } else {
            self.fallback.with_mut(data, f)
        }
    }
}
