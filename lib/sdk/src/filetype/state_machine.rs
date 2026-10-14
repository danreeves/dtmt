//! The `state_machine` resource: an animation state machine.
//!
//! 4,006 of these ship, from 221 bytes to 3.6 MB, and the layout below is the
//! community reader's (`bitsquid-blender-tools`, `stingray/animation_state_machine_resource.py`
//! and the files it pulls in: `animation_layer`, `animation_state`,
//! `animation_blend_settings`, `animation_transition`, `blend_set`, `ragdoll`,
//! `generic_structures`). Its DT variant is what is implemented here.
//!
//! A bundle variant stores the **bare body**; the reference reads a `DTHeader` first
//! because it is written for the standalone cooked form.
//!
//! ```text
//! layers                  Array(Layer)
//! event_names             Array(u32)
//! variable_names          Array(u32)
//! variables               Array(f32)
//! unknown_pairs           Array({f32, f32})
//! blend_sets              Array(BlendSet)
//! constraint_targets      Array(u32)
//! constraint_positions    Array({f32, f32, f32})
//! constraints             Array(byte)
//! ragdolls                Array(Ragdoll)
//! unknown_records         Array({u32 x 6, f32})
//! unknown_last            f32
//! ```
//!
//! `Array(x)` is a `u32` count followed by that many `x`, so every count is derived
//! on write from the vector it covers. `Layer` is `Array(State)` plus a `default_state`
//! index; `State` is the reference's `unk1`…`unk14` sequence, and `BlendSet`,
//! `Ragdoll`, `Marker`, `Transition`, `Switch` and `BlendSettings` are the small
//! fixed shapes the reference names. Fields the reference calls `unkN` keep that
//! name here, and `unknown_*` where the reference itself has no name - nothing is
//! renamed into a guess.
//!
//! The reader is strict: it walks the whole body and rejects anything left over
//! rather than silently stopping early, which is what makes a wrong field order show
//! up as a failure on the sweep instead of as quiet corruption.

use color_eyre::{Result, eyre::bail};

use crate::bundle::file::{BundleFile, UserFile};
use crate::bundle::filetype::BundleFileType;
use crate::murmur::IdString64;
use crate::{BundleFileVariant, Context};

/// The type id, for callers that need to recognise the payload.
pub const STATE_MACHINE_TYPE: u64 = 0xa486_d404_5106_165c;

/// One `{u32 x 6, f32}` record.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnknownRecord {
    pub words: [u32; 6],
    pub value: f32,
}

/// `stingray::BlendSet`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlendSet {
    pub bone_weights: Vec<f32>,
    pub unknown: Vec<u32>,
    pub unknown_bool: bool,
}

/// `stingray::Ragdoll`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ragdoll {
    pub dynamic_actors: Vec<u32>,
    pub keyframed_actors: Vec<u32>,
}

/// `stingray::AnimationState::Marker`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub time: f32,
    pub name: u32,
    pub marker_type: u32,
}

/// `stingray::AnimationTransition`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub event: u32,
    pub index: u32,
    pub is_conditional: bool,
}

/// `stingray::AnimationBlendSettings`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlendSettings {
    pub to: u32,
    pub blend: u32,
    pub mode: u32,
    pub on_beat: u32,
    pub unknown_1: u32,
    pub unknown_2: u32,
    pub unknown_3: u32,
}

/// `stingray::AnimationTransitionSwitchExit`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwitchExit {
    pub blend_settings_index: u32,
    pub interval_start: f32,
    pub interval_end: f32,
    pub exclude_start: bool,
    pub exclude_end: bool,
}

/// `stingray::AnimationTransitionSwitch`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Switch {
    pub bytecode: u32,
    pub exits: Vec<SwitchExit>,
}

/// `stingray::AnimationState`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub name: u64,
    pub name2: u64,
    pub name3: u64,
    pub state_type: u32,
    pub animations: Vec<u64>,
    pub probabilities: Vec<f32>,
    pub randomization_type: u32,
    pub loop_animation: bool,
    pub blend_type: u32,
    pub transitions: Vec<Transition>,
    pub blend_settings: Vec<BlendSettings>,
    pub switches: Vec<Switch>,
    pub unknown_1: u32,
    pub unknown_2: u32,
    pub timeline: Vec<Marker>,
    pub unknown_3: u32,
    pub unknown_4: u32,
    pub unknown_5: Vec<u32>,
    pub unknown_6: Vec<u32>,
    pub unknown_7: u32,
    pub unknown_8: u32,
    pub unknown_9: u32,
    pub unknown_10: u32,
    pub unknown_11: Vec<u32>,
    pub unknown_12: u32,
    pub unknown_13: u32,
    pub unknown_14: u32,
}

/// `stingray::AnimationLayer`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub states: Vec<State>,
    pub default_state: u32,
}

/// An animation state machine.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateMachine {
    pub layers: Vec<Layer>,
    pub event_names: Vec<u32>,
    pub variable_names: Vec<u32>,
    pub variables: Vec<f32>,
    pub unknown_pairs: Vec<[f32; 2]>,
    pub blend_sets: Vec<BlendSet>,
    pub constraint_targets: Vec<u32>,
    pub constraint_positions: Vec<[f32; 3]>,
    pub constraints: Vec<u8>,
    pub ragdolls: Vec<Ragdoll>,
    pub unknown_records: Vec<UnknownRecord>,
    pub unknown_last: f32,
}

/// A cursor over the body. Every read is bounds-checked so a short or mis-ordered
/// body fails rather than panicking.
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let bytes = self
            .data
            .get(self.at..self.at + n)
            .ok_or_else(|| color_eyre::eyre::eyre!("a state machine ends inside a field at {}", self.at))?;
        self.at += n;
        Ok(bytes)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    /// `Array(x)`: a `u32` count and that many `x`.
    fn array<T>(&mut self, one: impl Fn(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        let count = self.u32()? as usize;
        // A count is never allowed to promise more elements than there are bytes
        // left, so a corrupt count cannot make this allocate wildly.
        if count > self.data.len() - self.at {
            bail!("a state machine array declares {count} elements with too few bytes left");
        }
        let mut out = Vec::with_capacity(count);
        for _ in 0..count {
            out.push(one(self)?);
        }
        Ok(out)
    }

    fn u32_array(&mut self) -> Result<Vec<u32>> {
        self.array(Self::u32)
    }

    fn f32_array(&mut self) -> Result<Vec<f32>> {
        self.array(Self::f32)
    }

    fn u64_array(&mut self) -> Result<Vec<u64>> {
        self.array(Self::u64)
    }

    fn pairs(&mut self) -> Result<Vec<[f32; 2]>> {
        self.array(|r| Ok([r.f32()?, r.f32()?]))
    }

    fn vec3_array(&mut self) -> Result<Vec<[f32; 3]>> {
        self.array(|r| Ok([r.f32()?, r.f32()?, r.f32()?]))
    }

    fn byte_array(&mut self) -> Result<Vec<u8>> {
        let count = self.u32()? as usize;
        Ok(self.take(count)?.to_vec())
    }

    fn marker(&mut self) -> Result<Marker> {
        Ok(Marker {
            time: self.f32()?,
            name: self.u32()?,
            marker_type: self.u32()?,
        })
    }

    fn transition(&mut self) -> Result<Transition> {
        Ok(Transition {
            event: self.u32()?,
            index: self.u32()?,
            is_conditional: self.bool()?,
        })
    }

    fn blend_settings(&mut self) -> Result<BlendSettings> {
        Ok(BlendSettings {
            to: self.u32()?,
            blend: self.u32()?,
            mode: self.u32()?,
            on_beat: self.u32()?,
            unknown_1: self.u32()?,
            unknown_2: self.u32()?,
            unknown_3: self.u32()?,
        })
    }

    fn switch_exit(&mut self) -> Result<SwitchExit> {
        Ok(SwitchExit {
            blend_settings_index: self.u32()?,
            interval_start: self.f32()?,
            interval_end: self.f32()?,
            exclude_start: self.bool()?,
            exclude_end: self.bool()?,
        })
    }

    fn switch(&mut self) -> Result<Switch> {
        let bytecode = self.u32()?;
        let exits = self.array(Self::switch_exit)?;
        Ok(Switch { bytecode, exits })
    }

    fn state(&mut self) -> Result<State> {
        Ok(State {
            name: self.u64()?,
            name2: self.u64()?,
            name3: self.u64()?,
            state_type: self.u32()?,
            animations: self.u64_array()?,
            probabilities: self.f32_array()?,
            randomization_type: self.u32()?,
            loop_animation: self.bool()?,
            blend_type: self.u32()?,
            transitions: self.array(Self::transition)?,
            blend_settings: self.array(Self::blend_settings)?,
            switches: self.array(Self::switch)?,
            unknown_1: self.u32()?,
            unknown_2: self.u32()?,
            timeline: self.array(Self::marker)?,
            unknown_3: self.u32()?,
            unknown_4: self.u32()?,
            unknown_5: self.u32_array()?,
            unknown_6: self.u32_array()?,
            unknown_7: self.u32()?,
            unknown_8: self.u32()?,
            unknown_9: self.u32()?,
            unknown_10: self.u32()?,
            unknown_11: self.u32_array()?,
            unknown_12: self.u32()?,
            unknown_13: self.u32()?,
            unknown_14: self.u32()?,
        })
    }

    fn layer(&mut self) -> Result<Layer> {
        let states = self.array(Self::state)?;
        let default_state = self.u32()?;
        Ok(Layer {
            states,
            default_state,
        })
    }

    fn blend_set(&mut self) -> Result<BlendSet> {
        Ok(BlendSet {
            bone_weights: self.f32_array()?,
            unknown: self.u32_array()?,
            unknown_bool: self.bool()?,
        })
    }

    fn ragdoll(&mut self) -> Result<Ragdoll> {
        Ok(Ragdoll {
            dynamic_actors: self.u32_array()?,
            keyframed_actors: self.u32_array()?,
        })
    }

    fn unknown_record(&mut self) -> Result<UnknownRecord> {
        let mut words = [0u32; 6];
        for word in &mut words {
            *word = self.u32()?;
        }
        Ok(UnknownRecord {
            words,
            value: self.f32()?,
        })
    }
}

/// The writer, mirroring [`Reader`] field for field.
#[derive(Default)]
struct Writer {
    out: Vec<u8>,
}

impl Writer {
    fn u8(&mut self, v: u8) {
        self.out.push(v);
    }

    fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }

    fn u32_array(&mut self, v: &[u32]) {
        self.u32(v.len() as u32);
        for value in v {
            self.u32(*value);
        }
    }

    fn f32_array(&mut self, v: &[f32]) {
        self.u32(v.len() as u32);
        for value in v {
            self.f32(*value);
        }
    }

    fn u64_array(&mut self, v: &[u64]) {
        self.u32(v.len() as u32);
        for value in v {
            self.u64(*value);
        }
    }

    fn marker(&mut self, m: &Marker) {
        self.f32(m.time);
        self.u32(m.name);
        self.u32(m.marker_type);
    }

    fn transition(&mut self, t: &Transition) {
        self.u32(t.event);
        self.u32(t.index);
        self.bool(t.is_conditional);
    }

    fn blend_settings(&mut self, b: &BlendSettings) {
        self.u32(b.to);
        self.u32(b.blend);
        self.u32(b.mode);
        self.u32(b.on_beat);
        self.u32(b.unknown_1);
        self.u32(b.unknown_2);
        self.u32(b.unknown_3);
    }

    fn switch(&mut self, s: &Switch) {
        self.u32(s.bytecode);
        self.u32(s.exits.len() as u32);
        for exit in &s.exits {
            self.u32(exit.blend_settings_index);
            self.f32(exit.interval_start);
            self.f32(exit.interval_end);
            self.bool(exit.exclude_start);
            self.bool(exit.exclude_end);
        }
    }

    fn state(&mut self, s: &State) {
        self.u64(s.name);
        self.u64(s.name2);
        self.u64(s.name3);
        self.u32(s.state_type);
        self.u64_array(&s.animations);
        self.f32_array(&s.probabilities);
        self.u32(s.randomization_type);
        self.bool(s.loop_animation);
        self.u32(s.blend_type);
        self.u32(s.transitions.len() as u32);
        for t in &s.transitions {
            self.transition(t);
        }
        self.u32(s.blend_settings.len() as u32);
        for b in &s.blend_settings {
            self.blend_settings(b);
        }
        self.u32(s.switches.len() as u32);
        for sw in &s.switches {
            self.switch(sw);
        }
        self.u32(s.unknown_1);
        self.u32(s.unknown_2);
        self.u32(s.timeline.len() as u32);
        for m in &s.timeline {
            self.marker(m);
        }
        self.u32(s.unknown_3);
        self.u32(s.unknown_4);
        self.u32_array(&s.unknown_5);
        self.u32_array(&s.unknown_6);
        self.u32(s.unknown_7);
        self.u32(s.unknown_8);
        self.u32(s.unknown_9);
        self.u32(s.unknown_10);
        self.u32_array(&s.unknown_11);
        self.u32(s.unknown_12);
        self.u32(s.unknown_13);
        self.u32(s.unknown_14);
    }

    fn blend_set(&mut self, b: &BlendSet) {
        self.f32_array(&b.bone_weights);
        self.u32_array(&b.unknown);
        self.bool(b.unknown_bool);
    }

    fn ragdoll(&mut self, r: &Ragdoll) {
        self.u32_array(&r.dynamic_actors);
        self.u32_array(&r.keyframed_actors);
    }

    fn unknown_record(&mut self, r: &UnknownRecord) {
        for word in r.words {
            self.u32(word);
        }
        self.f32(r.value);
    }
}

impl StateMachine {
    /// Reads a state machine from its cooked bytes.
    pub fn from_binary(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        let layers = r.array(Reader::layer)?;
        let event_names = r.u32_array()?;
        let variable_names = r.u32_array()?;
        let variables = r.f32_array()?;
        let unknown_pairs = r.pairs()?;
        let blend_sets = r.array(Reader::blend_set)?;
        let constraint_targets = r.u32_array()?;
        let constraint_positions = r.vec3_array()?;
        let constraints = r.byte_array()?;
        let ragdolls = r.array(Reader::ragdoll)?;
        let unknown_records = r.array(Reader::unknown_record)?;
        let unknown_last = r.f32()?;
        // Walking the body exactly is the check that the field order is right: a
        // mis-ordered read stops somewhere other than the end.
        if r.at != bytes.len() {
            bail!(
                "a state machine's last field ends at {} of {} bytes",
                r.at,
                bytes.len()
            );
        }
        Ok(Self {
            layers,
            event_names,
            variable_names,
            variables,
            unknown_pairs,
            blend_sets,
            constraint_targets,
            constraint_positions,
            constraints,
            ragdolls,
            unknown_records,
            unknown_last,
        })
    }

    /// Writes the machine back, deriving every array count from its vector.
    pub fn to_binary(&self) -> Result<Vec<u8>> {
        let mut w = Writer::default();
        w.u32(self.layers.len() as u32);
        for layer in &self.layers {
            w.u32(layer.states.len() as u32);
            for state in &layer.states {
                w.state(state);
            }
            w.u32(layer.default_state);
        }
        w.u32_array(&self.event_names);
        w.u32_array(&self.variable_names);
        w.f32_array(&self.variables);
        w.u32(self.unknown_pairs.len() as u32);
        for pair in &self.unknown_pairs {
            w.f32(pair[0]);
            w.f32(pair[1]);
        }
        w.u32(self.blend_sets.len() as u32);
        for blend_set in &self.blend_sets {
            w.blend_set(blend_set);
        }
        w.u32_array(&self.constraint_targets);
        w.u32(self.constraint_positions.len() as u32);
        for position in &self.constraint_positions {
            for value in position {
                w.f32(*value);
            }
        }
        w.u32(self.constraints.len() as u32);
        for byte in &self.constraints {
            w.u8(*byte);
        }
        w.u32(self.ragdolls.len() as u32);
        for ragdoll in &self.ragdolls {
            w.ragdoll(ragdoll);
        }
        w.u32(self.unknown_records.len() as u32);
        for record in &self.unknown_records {
            w.unknown_record(record);
        }
        w.f32(self.unknown_last);
        Ok(w.out)
    }

    /// The resource as SJSON.
    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(self).map_err(color_eyre::Report::new)
    }

    /// Reads the resource from its SJSON.
    pub fn from_sjson(text: &str) -> Result<Self> {
        serde_sjson::from_str(text).map_err(color_eyre::Report::new)
    }
}

/// Decompiles a `state_machine` variant to its SJSON.
#[tracing::instrument(skip_all, fields(variants = variants.len()))]
pub fn decompile(_ctx: &Context, variants: &[BundleFileVariant]) -> Result<Vec<UserFile>> {
    let Some(variant) = variants.first() else {
        bail!("a state_machine resource needs a variant");
    };
    let machine = StateMachine::from_binary(variant.data())?;
    Ok(vec![UserFile::new(machine.to_sjson()?.into_bytes())])
}

/// Compiles a `state_machine` SJSON into a bundle file `name`d after the resource path.
pub fn compile(name: IdString64, sjson: &str) -> Result<BundleFile> {
    let machine = StateMachine::from_sjson(sjson)?;
    let mut variant = BundleFileVariant::new();
    variant.set_data(machine.to_binary()?);
    let mut file = BundleFile::new(name, BundleFileType::StateMachine);
    file.add_variant(variant);
    Ok(file)
}

#[cfg(test)]
mod test {
    use super::*;

    /// A machine with one layer, one empty state, one blend set, one ragdoll and one
    /// unknown record - hand-built field by field from the layout.
    fn sample() -> Vec<u8> {
        let mut w = Writer::default();
        // layers: 1
        w.u32(1);
        // layer: states: 1
        w.u32(1);
        // state
        w.u64(0x1111_1111_1111_1111);
        w.u64(0x2222_2222_2222_2222);
        w.u64(0x3333_3333_3333_3333);
        w.u32(0); // state_type
        w.u64_array(&[9, 10]); // animations
        w.f32_array(&[0.5]); // probabilities
        w.u32(0); // randomization_type
        w.bool(true); // loop_animation
        w.u32(0); // blend_type
        w.u32(0); // transitions
        w.u32(0); // blend_settings
        w.u32(0); // switches
        w.u32(1); // unknown_1
        w.u32(2); // unknown_2
        w.u32(0); // timeline
        w.u32(3); // unknown_3
        w.u32(4); // unknown_4
        w.u32_array(&[5]); // unknown_5
        w.u32_array(&[]); // unknown_6
        w.u32(6); // unknown_7
        w.u32(7); // unknown_8
        w.u32(8); // unknown_9
        w.u32(9); // unknown_10
        w.u32_array(&[]); // unknown_11
        w.u32(10); // unknown_12
        w.u32(11); // unknown_13
        w.u32(12); // unknown_14
        w.u32(0); // default_state
        // event_names, variable_names, variables, unknown_pairs
        w.u32_array(&[1, 2]);
        w.u32_array(&[]);
        w.f32_array(&[1.5]);
        w.u32(1);
        w.f32(0.25);
        w.f32(0.75);
        // blend_sets: one
        w.u32(1);
        w.f32_array(&[0.1, 0.2]);
        w.u32_array(&[3]);
        w.bool(false);
        // constraint_targets, constraint_positions, constraints
        w.u32_array(&[]);
        w.u32(1);
        w.f32(1.0);
        w.f32(2.0);
        w.f32(3.0);
        w.u32(2);
        w.u8(0xAB);
        w.u8(0xCD);
        // ragdolls: one
        w.u32(1);
        w.u32_array(&[7]);
        w.u32_array(&[8, 9]);
        // unknown_records: one, then unknown_last
        w.u32(1);
        for word in [1u32, 2, 3, 4, 5, 6] {
            w.u32(word);
        }
        w.f32(-1.0);
        w.f32(0.5);
        w.out
    }

    #[test]
    fn a_sample_machine_round_trips() {
        let bytes = sample();
        let machine = StateMachine::from_binary(&bytes).expect("the sample parses");
        assert_eq!(machine.layers.len(), 1);
        assert_eq!(machine.layers[0].states.len(), 1);
        assert_eq!(machine.layers[0].default_state, 0);
        let state = &machine.layers[0].states[0];
        assert_eq!(state.name, 0x1111_1111_1111_1111);
        assert_eq!(state.animations, vec![9, 10]);
        assert!(state.loop_animation);
        assert_eq!(state.unknown_5, vec![5]);
        assert_eq!(machine.event_names, vec![1, 2]);
        assert_eq!(machine.unknown_pairs, vec![[0.25, 0.75]]);
        assert_eq!(machine.blend_sets[0].bone_weights, vec![0.1, 0.2]);
        assert_eq!(machine.constraints, vec![0xAB, 0xCD]);
        assert_eq!(machine.ragdolls[0].keyframed_actors, vec![8, 9]);
        assert_eq!(machine.unknown_records[0].words, [1, 2, 3, 4, 5, 6]);
        assert_eq!(machine.unknown_last, 0.5);
        assert_eq!(machine.to_binary().unwrap(), bytes, "byte-for-byte");
    }

    #[test]
    fn a_sample_machine_survives_sjson() {
        let machine = StateMachine::from_binary(&sample()).unwrap();
        let sjson = machine.to_sjson().unwrap();
        assert_eq!(StateMachine::from_sjson(&sjson).unwrap(), machine);
    }

    #[test]
    fn a_trailing_byte_is_rejected() {
        let mut bytes = sample();
        bytes.push(0);
        assert!(StateMachine::from_binary(&bytes).is_err());
    }

    #[test]
    fn a_truncated_body_is_rejected() {
        let bytes = sample();
        assert!(StateMachine::from_binary(&bytes[..bytes.len() / 2]).is_err());
    }

    #[test]
    fn a_count_larger_than_the_body_is_rejected() {
        // The layer count promises more layers than there are bytes to hold them.
        let mut bytes = sample();
        bytes[0] = 0xFF;
        assert!(StateMachine::from_binary(&bytes).is_err());
    }
}
