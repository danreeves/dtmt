use std::sync::Arc;

use druid::im::Vector;
use druid::{Data, Lens};

#[derive(Copy, Clone, Data, PartialEq)]
pub(crate) enum View {
    Mods,
    Settings,
    About,
}

impl Default for View {
    fn default() -> Self {
        Self::Mods
    }
}

#[derive(Clone, Data, Lens)]
pub(crate) struct ModInfo {
    name: String,
    description: Arc<String>,
    enabled: bool,
}
impl ModInfo {
    pub fn new() -> Self {
        Self {
            name: format!("Test Mod: {:?}", std::time::SystemTime::now()),
            description: Arc::new(String::from("A test dummy")),
            enabled: false,
        }
    }
}

impl PartialEq for ModInfo {
    fn eq(&self, other: &Self) -> bool {
        self.name.eq(&other.name)
    }
}

#[derive(Clone, Data, Default, Lens)]
pub(crate) struct State {
    current_view: View,
    mods: Vector<ModInfo>,
    selected_mod_index: Option<usize>,
}

pub(crate) struct SelectedModLens;

impl Lens<State, Option<ModInfo>> for SelectedModLens {
    fn with<V, F: FnOnce(&Option<ModInfo>) -> V>(&self, data: &State, f: F) -> V {
        let info = data
            .selected_mod_index
            .and_then(|i| data.mods.get(i).cloned());

        f(&info)
    }

    fn with_mut<V, F: FnOnce(&mut Option<ModInfo>) -> V>(&self, data: &mut State, f: F) -> V {
        let mut info = data
            .selected_mod_index
            .and_then(|i| data.mods.get_mut(i).cloned());
        f(&mut info)
    }
}

/// A Lens that maps an `im::Vector<T>` to `im::Vector<(usize, T)>`,
/// where each element in the destination vector includes its index in the
/// source vector.
pub(crate) struct IndexedVectorLens;

impl<T: Data> Lens<Vector<T>, Vector<(usize, T)>> for IndexedVectorLens {
    fn with<V, F: FnOnce(&Vector<(usize, T)>) -> V>(&self, data: &Vector<T>, f: F) -> V {
        let data = data
            .iter()
            .enumerate()
            .map(|(i, val)| (i, val.clone()))
            .collect();
        f(&data)
    }

    fn with_mut<V, F: FnOnce(&mut Vector<(usize, T)>) -> V>(
        &self,
        data: &mut Vector<T>,
        f: F,
    ) -> V {
        todo!()
    }
}

impl State {
    #[allow(non_upper_case_globals)]
    pub const selected_mod: SelectedModLens = SelectedModLens;

    pub fn new() -> Self {
        Default::default()
    }

    pub fn get_current_view(&self) -> View {
        self.current_view
    }

    pub fn set_current_view(&mut self, view: View) {
        self.current_view = view;
    }

    pub fn delete_selected_mod(&mut self) {
        let Some(index) = self.selected_mod_index else {
            return;
        };

        self.mods.remove(index);
    }

    pub fn add_mod(&mut self, info: ModInfo) {
        self.mods.push_back(info);
        self.selected_mod_index = Some(self.mods.len() - 1);
    }
}
