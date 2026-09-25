//! Declarative shader-family definitions: the mod-side input to a generated
//! `shader43` section.
//!
//! A [`Family`] is what a mod writes instead of a shipped shader blob: the
//! programs (by source file), the channels the family exchanges between its
//! stages, and the material variables it accepts. [`Family::permutations`]
//! enumerates the runtime permutations - one per combination of the declared
//! optional variables - and each becomes a group in the section, compiled with
//! the flags its variables enable.
//!
//! The format is SJSON, the dialect the material and shader node files use, with
//! the names as map keys so they can be hashed straight to murmur32:
//!
//! ```sjson
//! // snoopy_ui.shader_family
//! channels = {
//!     vertex_position = {
//!         type = "float4"
//!         domain = "vertex"
//!         required = true
//!     }
//!     texture_map = {
//!         type = "texture2d"
//!         domain = "pixel"
//!         required = true
//!     }
//! }
//!
//! variables = {
//!     base_color = {
//!         type = "vector3"
//!         domain = "pixel"
//!         flag = "HAS_BASE_COLOR"
//!         default = [ 1, 1, 1 ]
//!     }
//!     opacity = {
//!         type = "scalar"
//!         domain = "pixel"
//!         flag = "HAS_OPACITY"
//!     }
//!     mod_tint = {
//!         type = "vector4"
//!         domain = "pixel"
//!     }
//! }
//!
//! programs = {
//!     vs_main = {
//!         source = "snoopymod/ui.vs.hlsl"
//!         stage = "vertex"
//!     }
//!     ps_main = {
//!         source = "snoopymod/ui.ps.hlsl"
//!         stage = "pixel"
//!     }
//! }
//! ```
//!
//! Everything the section still needs from the engine - the block header blob,
//! the engine's variable registry, the bindless conventions - is deliberately
//! not part of this file; it is carried by the toolchain instead.

use std::collections::BTreeMap;

use color_eyre::eyre;
use color_eyre::eyre::{Context, Result};
use serde::{Deserialize, Serialize};

/// The stage a channel or variable belongs to. A `vertex` channel is written by
/// the vertex program and interpolated into the pixel program; a `pixel` one is
/// a pixel-program-only value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Domain {
    /// Written by the vertex stage and read by the pixel stage.
    Vertex,
    /// Pixel stage only.
    #[default]
    Pixel,
}

/// The type of a channel or variable. The sizes match the group data's record
/// sizes: 4, 8, 12 and 16 bytes, and 64 for a 4x4 matrix. The material's own
/// spelling (`scalar`, `vector2`, ...) is accepted as an alias.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ValueType {
    /// A scalar (4 bytes).
    #[serde(alias = "scalar")]
    Float,
    /// A two-component vector (8 bytes).
    #[serde(alias = "vector2")]
    Float2,
    /// A three-component vector (12 bytes).
    #[serde(alias = "vector3")]
    Float3,
    /// A four-component vector (16 bytes).
    #[serde(alias = "vector4")]
    Float4,
    /// A 4x4 matrix (64 bytes).
    #[serde(alias = "matrix", alias = "float3x3")]
    Float4x4,
    /// A 2D texture channel.
    Texture2D,
}

impl ValueType {
    /// The number of bytes one element of this type occupies, as the group data
    /// records it.
    pub fn size(self) -> u32 {
        match self {
            Self::Float => 4,
            Self::Float2 => 8,
            Self::Float3 => 12,
            Self::Float4 | Self::Texture2D => 16,
            Self::Float4x4 => 64,
        }
    }
}

/// A channel: a named value the vertex program hands to the pixel program, or a
/// texture the material binds.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelDef {
    /// The channel's type.
    #[serde(rename = "type")]
    pub kind: ValueType,
    /// Which stage produces it.
    #[serde(default)]
    pub domain: Domain,
    /// Whether the material must provide it. A required channel is in every
    /// interface; an optional one joins only the permutations that define its
    /// variable's flag.
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    /// The variable whose flag gates this channel, when it is not the channel's
    /// own name (a `normal_map` texture can follow `normal_strength`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable: Option<String>,
    /// The DXBC semantic to bind the channel to, if not the default for its
    /// type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<String>,
}

/// A material variable: a value the material may set by name, like
/// `dev_wireframe_color` in the shipped families. A variable with a `flag` is
/// optional - it belongs to the interface only in the permutations that define
/// that flag.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VariableDef {
    /// The variable's type.
    #[serde(rename = "type")]
    pub kind: ValueType,
    /// Which stage reads it.
    #[serde(default)]
    pub domain: Domain,
    /// The permutation flag that makes the variable part of the interface. A
    /// variable without a flag is always present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag: Option<String>,
    /// The default value written into the section's default data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub default: Vec<f32>,
}

/// One program of a stage, given by the source file `dtmt build` compiles.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramDef {
    /// The HLSL source, relative to the mod root.
    pub source: String,
    /// `vertex` or `pixel`.
    pub stage: String,
}

/// A whole family declaration: the programs to compile, the channels the family
/// exchanges and the material variables it accepts, keyed by name.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    /// The programs to compile, keyed by entry point name.
    #[serde(default)]
    pub programs: BTreeMap<String, ProgramDef>,
    /// The channels the family exchanges.
    #[serde(default)]
    pub channels: BTreeMap<String, ChannelDef>,
    /// The material variables the family accepts.
    #[serde(default)]
    pub variables: BTreeMap<String, VariableDef>,
}

impl Family {
    /// Parses a declaration from SJSON.
    pub fn from_sjson(sjson: &str) -> Result<Self> {
        serde_sjson::from_str(sjson)
            .map_err(|err| eyre::eyre!("failed to parse the shader family: {err}"))
    }

    /// Serialises the declaration back to SJSON.
    pub fn to_sjson(&self) -> Result<String> {
        serde_sjson::to_string(self).wrap_err("failed to write the shader family")
    }

    /// The flags the declaration can define, in name order.
    pub fn flags(&self) -> Vec<&str> {
        let mut flags: Vec<&str> = Vec::new();
        for variable in self.variables.values() {
            if let Some(flag) = &variable.flag
                && !flags.contains(&flag.as_str())
            {
                flags.push(flag);
            }
        }
        flags
    }

    /// Enumerates the runtime permutations: one per combination of the declared
    /// optional variables, in the order the flags are named (bit 0 first). Each
    /// becomes one group in the section and one program pair compiled with the
    /// flags its variables enable; the variables and channels that ride along
    /// are that group's interface.
    pub fn permutations(&self) -> Vec<Permutation> {
        let flags = self.flags();
        let count = 1u32 << flags.len();
        (0..count)
            .map(|mask| Permutation {
                mask,
                flags: flags
                    .iter()
                    .enumerate()
                    .filter(|(bit, _)| mask & (1 << bit) != 0)
                    .map(|(_, flag)| (*flag).to_string())
                    .collect(),
                variables: self
                    .variables
                    .iter()
                    .filter(|(_, variable)| match &variable.flag {
                        None => true,
                        Some(flag) => self.enabled(flag, mask, &flags),
                    })
                    .map(|(name, _)| name.clone())
                    .collect(),
                channels: self
                    .channels
                    .iter()
                    .filter(|(name, channel)| {
                        channel.required
                            || self
                                .gating_variable(name, channel)
                                .and_then(|variable| variable.flag.as_ref())
                                .is_some_and(|flag| self.enabled(flag, mask, &flags))
                    })
                    .map(|(name, _)| name.clone())
                    .collect(),
            })
            .collect()
    }

    /// The number of groups the declaration generates.
    pub fn group_count(&self) -> usize {
        1usize << self.flags().len()
    }

    /// Whether `flag` is set in `mask`.
    fn enabled(&self, flag: &str, mask: u32, flags: &[&str]) -> bool {
        flags
            .iter()
            .position(|f| *f == flag)
            .is_some_and(|bit| mask & (1 << bit) != 0)
    }

    /// The variable that gates the channel `name`: the one it names, or the
    /// variable of the same name.
    fn gating_variable<'a>(
        &'a self,
        name: &'a str,
        channel: &'a ChannelDef,
    ) -> Option<&'a VariableDef> {
        self.variables
            .get(channel.variable.as_deref().unwrap_or(name))
    }
}

/// One enumerated permutation: the flags it defines and the interface it
/// exposes. `mask` is the bit set over [`Family::flags`] and is what the
/// conditions tree indexes on.
#[derive(Clone, Debug, PartialEq)]
pub struct Permutation {
    /// The bit set over [`Family::flags`].
    pub mask: u32,
    /// The permutation flags, in declaration order.
    pub flags: Vec<String>,
    /// The variable names in the interface.
    pub variables: Vec<String>,
    /// The channel names in the interface.
    pub channels: Vec<String>,
}

impl Permutation {
    /// Whether the permutation defines `flag`.
    pub fn defines(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
        // The family a mod would ship.
        channels = {
            vertex_position = {
                type = "float4"
                domain = "vertex"
                required = true
            }
            texture_map = {
                type = "texture2d"
                domain = "pixel"
                required = true
            }
        }

        variables = {
            base_color = {
                type = "vector3"
                domain = "pixel"
                flag = "HAS_BASE_COLOR"
                default = [ 1, 1, 1 ]
            }
            opacity = {
                type = "scalar"
                domain = "pixel"
                flag = "HAS_OPACITY"
            }
            mod_tint = {
                type = "vector4"
                domain = "pixel"
            }
        }

        programs = {
            vs_main = {
                source = "snoopymod/ui.vs.hlsl"
                stage = "vertex"
            }
            ps_main = {
                source = "snoopymod/ui.ps.hlsl"
                stage = "pixel"
            }
        }
    "#;

    #[test]
    fn parses_a_declaration() {
        let family = Family::from_sjson(SAMPLE).expect("parse");
        assert_eq!(family.programs.len(), 2);
        assert_eq!(family.programs["vs_main"].stage, "vertex");
        assert_eq!(family.channels.len(), 2);
        assert_eq!(family.channels["texture_map"].kind, ValueType::Texture2D);
        assert_eq!(family.channels["vertex_position"].domain, Domain::Vertex);
        assert!(family.channels["texture_map"].required);
        assert_eq!(family.variables.len(), 3);
        assert_eq!(family.variables["base_color"].default, vec![1.0, 1.0, 1.0]);
        // The types agree with the group data's record sizes.
        assert_eq!(family.channels["vertex_position"].kind.size(), 16);
        assert_eq!(family.variables["mod_tint"].kind.size(), 16);
    }

    #[test]
    fn enumerates_one_group_per_optional_variable_subset() {
        let family = Family::from_sjson(SAMPLE).expect("parse");
        assert_eq!(family.flags(), vec!["HAS_BASE_COLOR", "HAS_OPACITY"]);
        // Two optional variables: four interfaces, so four groups.
        assert_eq!(family.group_count(), 4);

        let permutations = family.permutations();
        assert!(permutations[0].flags.is_empty());
        // Without the optional variables only the always-present one is left.
        assert_eq!(permutations[0].variables, vec!["mod_tint"]);
        // The required channels ride along in every permutation.
        assert_eq!(
            permutations[0].channels,
            vec!["texture_map", "vertex_position"]
        );

        assert!(permutations[1].defines("HAS_BASE_COLOR"));
        assert!(!permutations[1].defines("HAS_OPACITY"));
        assert_eq!(permutations[1].variables, vec!["base_color", "mod_tint"]);

        assert!(permutations[2].defines("HAS_OPACITY"));
        assert!(!permutations[2].defines("HAS_BASE_COLOR"));

        assert!(permutations[3].defines("HAS_BASE_COLOR"));
        assert!(permutations[3].defines("HAS_OPACITY"));
        assert_eq!(
            permutations[3].variables,
            vec!["base_color", "mod_tint", "opacity"]
        );
        // The masks are distinct, so the conditions tree can key on them.
        let masks: Vec<u32> = permutations.iter().map(|p| p.mask).collect();
        assert_eq!(masks, vec![0, 1, 2, 3]);
    }

    #[test]
    fn an_optional_channel_follows_its_variable() {
        let text = r#"
            channels = {
                texture_map = {
                    type = "texture2d"
                    required = true
                }
                normal_map = {
                    type = "texture2d"
                    variable = "normal_strength"
                }
            }
            variables = {
                normal_strength = {
                    type = "scalar"
                    flag = "HAS_NORMAL_MAP"
                }
            }
        "#;
        let family = Family::from_sjson(text).expect("parse");
        let permutations = family.permutations();
        assert_eq!(permutations.len(), 2);
        assert_eq!(permutations[0].channels, vec!["texture_map"]);
        assert_eq!(permutations[1].channels, vec!["normal_map", "texture_map"]);
    }

    #[test]
    fn round_trips_through_sjson() {
        let family = Family::from_sjson(SAMPLE).expect("parse");
        let text = family.to_sjson().expect("write");
        let again = Family::from_sjson(&text).expect("re-parse");
        assert_eq!(family, again);
    }

    #[test]
    fn an_empty_declaration_makes_one_group() {
        let family = Family::from_sjson("channels = {}").expect("parse");
        assert_eq!(family.group_count(), 1);
        assert_eq!(family.permutations()[0].mask, 0);
    }

    #[test]
    fn rejects_an_unknown_key() {
        let err = Family::from_sjson("programz = {}").expect_err("must fail");
        assert!(err.to_string().contains("programz"), "{err}");
    }
}
