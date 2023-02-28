use druid::im::Vector;
use druid::{Data, Lens};

use super::{ModInfo, State};

pub(crate) struct SelectedModLens;

impl Lens<State, Option<ModInfo>> for SelectedModLens {
    #[tracing::instrument(name = "SelectedModLens::with", skip_all)]
    fn with<V, F: FnOnce(&Option<ModInfo>) -> V>(&self, data: &State, f: F) -> V {
        let info = data
            .selected_mod_index
            .and_then(|i| data.mods.get(i).cloned());

        f(&info)
    }

    #[tracing::instrument(name = "SelectedModLens::with_mut", skip_all)]
    fn with_mut<V, F: FnOnce(&mut Option<ModInfo>) -> V>(&self, data: &mut State, f: F) -> V {
        match data.selected_mod_index {
            Some(i) => {
                let mut info = data.mods.get_mut(i).cloned();
                let ret = f(&mut info);

                if let Some(info) = info {
                    // TODO: Figure out a way to check for equality and
                    // only update when needed
                    data.mods.set(i, info);
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
