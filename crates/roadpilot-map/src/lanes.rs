//! Building the lane model from road looks, countries and parsed sectors.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

use roadpilot_map_formats::BaseSector;
use roadpilot_map_formats::Token;
use roadpilot_map_formats::definitions::{
    CountryDef, DefinitionError, RoadLookDef, RoadTemplateVariantDef, TrafficLaneDef,
};
use roadpilot_map_formats::sii::SiiUnit;
use roadpilot_map_model::{
    Lane, LaneConfig, LaneError, LaneId, LaneIssue, LaneNetwork, LaneType, OffsetsState, Road,
    RoadLanes, RoadNetwork, RoadSide, StoredLaneOffset, TrafficSide, TrafficSideIssue,
    TrafficSideState,
};

/// The definitions the lane model needs, indexed by unit name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LaneDefinitions {
    looks: BTreeMap<String, RoadLookDef>,
    variants: BTreeMap<String, RoadTemplateVariantDef>,
    lane_types: BTreeMap<String, TrafficLaneDef>,
    countries: BTreeMap<u32, CountryDef>,
}

/// Why the definitions could not be collected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DefinitionsError {
    /// A unit could not be read.
    Invalid(DefinitionError),
    /// Two units of the same class share a name (or two countries an ID).
    Duplicate { class: &'static str, name: String },
    /// A road look has a `lane_direction` other than `right_hand` or `left_hand`.
    UnknownLaneDirection { look: String, value: String },
}

impl fmt::Display for DefinitionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(e) => e.fmt(f),
            Self::Duplicate { class, name } => write!(f, "{class} {name:?} is defined twice"),
            Self::UnknownLaneDirection { look, value } => {
                write!(f, "road look {look:?} has unknown lane_direction {value:?}")
            }
        }
    }
}

impl std::error::Error for DefinitionsError {}

fn insert<K: Ord, V>(
    map: &mut BTreeMap<K, V>,
    key: K,
    value: V,
    class: &'static str,
    name: &str,
) -> Result<(), DefinitionsError> {
    if map.insert(key, value).is_some() {
        return Err(DefinitionsError::Duplicate {
            class,
            name: name.to_owned(),
        });
    }
    Ok(())
}

impl LaneDefinitions {
    /// Collects the relevant units from parsed SII files: `road_look`,
    /// `road_template_variant`, `traffic_lane_data` and `country_data`.
    /// Other units are ignored.
    ///
    /// # Errors
    ///
    /// [`DefinitionsError`] for unreadable or duplicated units and unknown
    /// `lane_direction` values. Nothing is silently replaced.
    pub fn from_units<'a>(
        units: impl IntoIterator<Item = &'a SiiUnit>,
    ) -> Result<Self, DefinitionsError> {
        let mut defs = Self::default();
        for unit in units {
            if let Some(look) = RoadLookDef::from_unit(unit) {
                let look = look.map_err(DefinitionsError::Invalid)?;
                if let Some(d) = &look.lane_direction {
                    if d != "right_hand" && d != "left_hand" {
                        return Err(DefinitionsError::UnknownLaneDirection {
                            look: look.name.clone(),
                            value: d.clone(),
                        });
                    }
                }
                insert(
                    &mut defs.looks,
                    look.name.clone(),
                    look,
                    "road_look",
                    &unit.name,
                )?;
            } else if let Some(v) = RoadTemplateVariantDef::from_unit(unit) {
                insert(
                    &mut defs.variants,
                    v.name.clone(),
                    v,
                    "road_template_variant",
                    &unit.name,
                )?;
            } else if let Some(t) = TrafficLaneDef::from_unit(unit) {
                insert(
                    &mut defs.lane_types,
                    t.name.clone(),
                    t,
                    "traffic_lane_data",
                    &unit.name,
                )?;
            } else if let Some(c) = CountryDef::from_unit(unit) {
                let c = c.map_err(DefinitionsError::Invalid)?;
                insert(&mut defs.countries, c.id, c, "country_data", &unit.name)?;
            }
        }
        Ok(defs)
    }

    /// Number of road looks, variants, lane types and countries.
    #[must_use]
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        (
            self.looks.len(),
            self.variants.len(),
            self.lane_types.len(),
            self.countries.len(),
        )
    }
}

/// Why the lane network could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LaneBuildError {
    /// A side has more lanes than a lane index can hold.
    TooManyLanes { look: String },
    /// The model rejected the result.
    Model(LaneError),
}

impl fmt::Display for LaneBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyLanes { look } => write!(f, "road look {look:?} has too many lanes"),
            Self::Model(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for LaneBuildError {}

/// The country IDs stored in node flags (bits 8-15 and 16-23). 0 means none.
fn node_countries(flags: u32) -> [u32; 2] {
    [(flags >> 8) & 0xFF, (flags >> 16) & 0xFF]
}

/// Builds the lanes of every road of `network`.
///
/// `sectors` must contain the nodes of all roads (their flags carry the
/// country IDs). Each road's traffic side comes from the country of its
/// nodes; it is unknown if the nodes name no country or different ones, if
/// the country is not defined, or if the road look's `lane_direction`
/// contradicts the country.
///
/// # Errors
///
/// [`LaneBuildError`] if a look has more lanes than fit a lane index, or the
/// model rejects the result.
pub fn build_lane_network(
    network: &RoadNetwork,
    sectors: &[BaseSector<'_>],
    definitions: &LaneDefinitions,
) -> Result<LaneNetwork, LaneBuildError> {
    let flags: HashMap<u64, u32> = sectors
        .iter()
        .flat_map(|s| &s.nodes)
        .map(|n| (n.uid, n.flags))
        .collect();
    let lanes = network
        .roads()
        .map(|road| road_lanes(road, &flags, definitions))
        .collect::<Result<Vec<_>, _>>()?;
    LaneNetwork::new(network, lanes).map_err(LaneBuildError::Model)
}

fn traffic_side(
    road: &Road,
    flags: &HashMap<u64, u32>,
    definitions: &LaneDefinitions,
    look: Option<&RoadLookDef>,
) -> TrafficSideState {
    let nodes = [road.start_node, road.end_node];
    let mut ids = nodes
        .iter()
        .flat_map(|n| node_countries(flags.get(&n.0).copied().unwrap_or(0)))
        .filter(|&id| id != 0);
    let Some(first) = ids.next() else {
        return TrafficSideState::Unknown(TrafficSideIssue::NoCountry);
    };
    if ids.any(|id| id != first) {
        return TrafficSideState::Unknown(TrafficSideIssue::CountryMismatch);
    }
    let Some(country) = definitions.countries.get(&first) else {
        return TrafficSideState::Unknown(TrafficSideIssue::UnknownCountry(first));
    };
    let side = if country.leftside_traffic {
        TrafficSide::Left
    } else {
        TrafficSide::Right
    };
    let look_side = look
        .and_then(|l| l.lane_direction.as_deref())
        .map(|d| match d {
            "left_hand" => TrafficSide::Left,
            _ => TrafficSide::Right,
        });
    match look_side {
        Some(s) if s != side => TrafficSideState::Unknown(TrafficSideIssue::LookConflict),
        _ => TrafficSideState::Known(side),
    }
}

fn road_lanes(
    road: &Road,
    flags: &HashMap<u64, u32>,
    definitions: &LaneDefinitions,
) -> Result<RoadLanes, LaneBuildError> {
    let look_name = format!("road.{}", Token(road.look_token).name().unwrap_or_default());
    let look = definitions.looks.get(&look_name);
    let traffic_side = traffic_side(road, flags, definitions, look);
    let mut entry = RoadLanes {
        road: road.id,
        look: look_name.clone(),
        traffic_side,
        config: LaneConfig::Unresolved(LaneIssue::MissingRoadLook(look_name.clone())),
        offsets_left: OffsetsState::NotStored,
        offsets_right: OffsetsState::NotStored,
        stored_size_left: None,
        stored_size_right: None,
    };
    let Some(look) = look else {
        return Ok(entry);
    };
    entry.stored_size_left = look.road_size_left;
    entry.stored_size_right = look.road_size_right;
    entry.config = lane_config(
        road,
        look,
        definitions,
        &mut entry.offsets_left,
        &mut entry.offsets_right,
    )?;
    Ok(entry)
}

fn lane_config(
    road: &Road,
    look: &RoadLookDef,
    definitions: &LaneDefinitions,
    offsets_left: &mut OffsetsState,
    offsets_right: &mut OffsetsState,
) -> Result<LaneConfig, LaneBuildError> {
    // Variants that set a side's lane list: same count required; different
    // types make the side's types uncertain.
    let mut uncertain = [false, false];
    for name in &look.variants {
        let Some(variant) = definitions.variants.get(name) else {
            return Ok(LaneConfig::Unresolved(LaneIssue::MissingVariant(
                name.clone(),
            )));
        };
        for (i, (base, replaced)) in [
            (&look.lanes_left, &variant.lanes_left),
            (&look.lanes_right, &variant.lanes_right),
        ]
        .into_iter()
        .enumerate()
        {
            if let Some(list) = replaced {
                if list.len() != base.len() {
                    return Ok(LaneConfig::Unresolved(LaneIssue::VariantChangesLaneCount));
                }
                if list != base {
                    uncertain[i] = true;
                }
            }
        }
    }
    let mut lanes = Vec::new();
    for (i, (side, types, offsets, state)) in [
        (
            RoadSide::Left,
            &look.lanes_left,
            &look.lane_offsets_left,
            offsets_left,
        ),
        (
            RoadSide::Right,
            &look.lanes_right,
            &look.lane_offsets_right,
            offsets_right,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        *state = match offsets.len() {
            0 => OffsetsState::NotStored,
            n if n == types.len() => OffsetsState::Assigned,
            n => OffsetsState::CountMismatch {
                offsets: n,
                lanes: types.len(),
            },
        };
        for (index, name) in types.iter().enumerate() {
            if !definitions.lane_types.contains_key(name) {
                return Ok(LaneConfig::Unresolved(LaneIssue::MissingLaneType(
                    name.clone(),
                )));
            }
            let index = u16::try_from(index).map_err(|_| LaneBuildError::TooManyLanes {
                look: look.name.clone(),
            })?;
            let offset = (*state == OffsetsState::Assigned)
                .then(|| offsets.get(usize::from(index)))
                .flatten()
                .map(|&(first, second)| StoredLaneOffset { first, second });
            lanes.push(Lane {
                id: LaneId {
                    road: road.id,
                    side,
                    index,
                },
                lane_type: if uncertain[i] {
                    LaneType::Uncertain(name.clone())
                } else {
                    LaneType::Known(name.clone())
                },
                offset,
                width: None,
            });
        }
    }
    Ok(LaneConfig::Resolved(lanes))
}
