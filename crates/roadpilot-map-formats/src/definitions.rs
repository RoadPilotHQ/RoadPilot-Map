//! The definition records the lane model needs, taken from parsed SII units.
//!
//! Only four unit classes are read: `road_look`, `road_template_variant`,
//! `traffic_lane_data` and `country_data`. Values are kept as stored; nothing
//! is defaulted. See `docs/formats/scs-lane-model.md`.

use crate::sii::{SiiUnit, parse_tuple};

/// A road look (`def/world/road_look*.sii`).
#[derive(Debug, Clone, PartialEq)]
pub struct RoadLookDef {
    /// Unit name, `road.<token>`.
    pub name: String,
    /// Lane type names (`traffic_lane_data` units) on the left side of the
    /// road's stored direction, in definition order.
    pub lanes_left: Vec<String>,
    /// Lane type names on the right side, in definition order.
    pub lanes_right: Vec<String>,
    /// `lane_offsets_left`: one stored pair per entry. Meaning not confirmed.
    pub lane_offsets_left: Vec<(f32, f32)>,
    /// `lane_offsets_right`: one stored pair per entry. Meaning not confirmed.
    pub lane_offsets_right: Vec<(f32, f32)>,
    /// `road_size_left`, presumably the width of the left side. Not confirmed.
    pub road_size_left: Option<f32>,
    /// `road_size_right`, presumably the width of the right side. Not confirmed.
    pub road_size_right: Option<f32>,
    /// `road_offset`. Meaning not confirmed.
    pub road_offset: Option<f32>,
    /// `lane_direction` as stored (`right_hand` or `left_hand`), if present.
    pub lane_direction: Option<String>,
    /// Names of `road_template_variant` units referenced by
    /// `template_variants_left`, `template_variants_right` and `lane_variants`.
    pub variants: Vec<String>,
}

/// A road template variant (`road_template_variant`), which can replace the
/// lane lists of a look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoadTemplateVariantDef {
    pub name: String,
    /// `None` if the variant does not set the list.
    pub lanes_left: Option<Vec<String>>,
    pub lanes_right: Option<Vec<String>>,
}

/// A lane type (`traffic_lane_data` in `def/world/traffic_lane.sii`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrafficLaneDef {
    pub name: String,
}

/// A country (`country_data` in `def/country.sii`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountryDef {
    pub name: String,
    /// `country_id`, the number stored in node flags.
    pub id: u32,
    /// `leftside_traffic: true`.
    pub leftside_traffic: bool,
}

/// Why a definition unit could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionError {
    /// Unit name.
    pub unit: String,
    /// What is wrong.
    pub reason: &'static str,
}

impl std::fmt::Display for DefinitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "definition {:?}: {}", self.unit, self.reason)
    }
}

impl std::error::Error for DefinitionError {}

fn error(unit: &SiiUnit, reason: &'static str) -> DefinitionError {
    DefinitionError {
        unit: unit.name.clone(),
        reason,
    }
}

fn opt_f32(unit: &SiiUnit, key: &str) -> Result<Option<f32>, DefinitionError> {
    unit.value(key)
        .map(|v| {
            v.parse::<f32>()
                .ok()
                .filter(|f| f.is_finite())
                .ok_or_else(|| error(unit, "a numeric attribute is not a finite number"))
        })
        .transpose()
}

fn pairs(unit: &SiiUnit, key: &str) -> Result<Vec<(f32, f32)>, DefinitionError> {
    unit.values(key)
        .iter()
        .map(|v| match parse_tuple(v).as_deref() {
            Some([a, b]) => Ok((*a, *b)),
            _ => Err(error(unit, "a lane offset is not a pair of numbers")),
        })
        .collect()
}

impl RoadLookDef {
    /// Reads a `road_look` unit; `None` for units of other classes.
    ///
    /// # Errors
    ///
    /// [`DefinitionError`] for non-numeric sizes or malformed offset pairs.
    pub fn from_unit(unit: &SiiUnit) -> Option<Result<Self, DefinitionError>> {
        (unit.class == "road_look").then(|| {
            Ok(Self {
                name: unit.name.clone(),
                lanes_left: unit.values("lanes_left").to_vec(),
                lanes_right: unit.values("lanes_right").to_vec(),
                lane_offsets_left: pairs(unit, "lane_offsets_left")?,
                lane_offsets_right: pairs(unit, "lane_offsets_right")?,
                road_size_left: opt_f32(unit, "road_size_left")?,
                road_size_right: opt_f32(unit, "road_size_right")?,
                road_offset: opt_f32(unit, "road_offset")?,
                lane_direction: unit.value("lane_direction").map(str::to_owned),
                variants: [
                    "template_variants_left",
                    "template_variants_right",
                    "lane_variants",
                ]
                .iter()
                .flat_map(|k| unit.values(k).iter().cloned())
                .collect(),
            })
        })
    }
}

impl RoadTemplateVariantDef {
    /// Reads a `road_template_variant` unit; `None` for other classes.
    #[must_use]
    pub fn from_unit(unit: &SiiUnit) -> Option<Self> {
        (unit.class == "road_template_variant").then(|| Self {
            name: unit.name.clone(),
            lanes_left: unit.attributes.get("lanes_left").cloned(),
            lanes_right: unit.attributes.get("lanes_right").cloned(),
        })
    }
}

impl TrafficLaneDef {
    /// Reads a `traffic_lane_data` unit; `None` for other classes.
    #[must_use]
    pub fn from_unit(unit: &SiiUnit) -> Option<Self> {
        (unit.class == "traffic_lane_data").then(|| Self {
            name: unit.name.clone(),
        })
    }
}

impl CountryDef {
    /// Reads a `country_data` unit; `None` for other classes.
    ///
    /// # Errors
    ///
    /// [`DefinitionError`] if `country_id` is missing or not a number, or
    /// `leftside_traffic` is not `true` or `false`.
    pub fn from_unit(unit: &SiiUnit) -> Option<Result<Self, DefinitionError>> {
        (unit.class == "country_data").then(|| {
            let id = unit
                .value("country_id")
                .and_then(|v| v.parse::<u32>().ok())
                .ok_or_else(|| error(unit, "country_id is missing or not a number"))?;
            let leftside_traffic = match unit.value("leftside_traffic") {
                None | Some("false") => false,
                Some("true") => true,
                Some(_) => return Err(error(unit, "leftside_traffic is not true or false")),
            };
            Ok(Self {
                name: unit.name.clone(),
                id,
                leftside_traffic,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn unit(class: &str, name: &str, attrs: &[(&str, &[&str])]) -> SiiUnit {
        SiiUnit {
            class: class.into(),
            name: name.into(),
            attributes: attrs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.iter().map(|s| (*s).to_owned()).collect()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    #[test]
    fn reads_a_road_look() {
        let u = unit(
            "road_look",
            "road.a",
            &[
                ("lanes_right", &["t.x", "t.y"]),
                ("lane_offsets_right", &["(1, 2)", "(3.5, -1)"]),
                ("road_size_right", &["4.5"]),
                ("lane_direction", &["left_hand"]),
                ("template_variants_right", &[".v1"]),
                ("lane_variants", &[".v2"]),
            ],
        );
        let look = RoadLookDef::from_unit(&u)
            .expect("road look")
            .expect("valid");
        assert!(look.lanes_left.is_empty());
        assert_eq!(look.lanes_right, ["t.x", "t.y"]);
        assert_eq!(look.lane_offsets_right, [(1.0, 2.0), (3.5, -1.0)]);
        assert_eq!(look.road_size_right, Some(4.5));
        assert_eq!(look.road_size_left, None);
        assert_eq!(look.lane_direction.as_deref(), Some("left_hand"));
        assert_eq!(look.variants, [".v1", ".v2"]);
    }

    #[test]
    fn rejects_malformed_road_look_values() {
        let bad_size = unit("road_look", "road.a", &[("road_size_left", &["wide"])]);
        assert!(matches!(RoadLookDef::from_unit(&bad_size), Some(Err(_))));
        let bad_pair = unit("road_look", "road.a", &[("lane_offsets_left", &["(1)"])]);
        assert!(matches!(RoadLookDef::from_unit(&bad_pair), Some(Err(_))));
        assert!(RoadLookDef::from_unit(&unit("other", "x", &[])).is_none());
    }

    #[test]
    fn reads_variants_lanes_and_countries() {
        let v = unit("road_template_variant", ".v", &[("lanes_left", &["t.a"])]);
        let v = RoadTemplateVariantDef::from_unit(&v).expect("variant");
        assert_eq!(v.lanes_left, Some(vec!["t.a".to_owned()]));
        assert_eq!(v.lanes_right, None);
        assert!(TrafficLaneDef::from_unit(&unit("traffic_lane_data", "t.a", &[])).is_some());
        let c = unit(
            "country_data",
            "country.data.x",
            &[("country_id", &["10"]), ("leftside_traffic", &["true"])],
        );
        let c = CountryDef::from_unit(&c).expect("country").expect("valid");
        assert_eq!((c.id, c.leftside_traffic), (10, true));
        let bad = unit(
            "country_data",
            "c",
            &[("leftside_traffic", &["maybe"]), ("country_id", &["1"])],
        );
        assert!(matches!(CountryDef::from_unit(&bad), Some(Err(_))));
        let no_id = unit("country_data", "c", &[]);
        assert!(matches!(CountryDef::from_unit(&no_id), Some(Err(_))));
    }
}
