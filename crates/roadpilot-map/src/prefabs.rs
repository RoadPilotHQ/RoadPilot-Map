//! Building the prefab model from prefab items, prefab models and their
//! `.ppd` descriptors.

use std::collections::BTreeMap;

use roadpilot_map_formats::definitions::{PrefabModelDef, TrafficRuleDef};
use roadpilot_map_formats::ppd::{PrefabDescriptor, parse_ppd};
use roadpilot_map_formats::sii::SiiUnit;
use roadpilot_map_formats::{BaseSector, ItemData, Token};
use roadpilot_map_model::{
    NodeId, PrefabCurve, PrefabDefinition, PrefabDefinitionNode, PrefabError, PrefabId,
    PrefabInstance, PrefabNetwork, PrefabResolution, PrefabSemaphore, RoadNetwork, RuleRef,
};

use crate::lanes::DefinitionsError;

/// The definitions the prefab model needs: `prefab_model` and
/// `traffic_rule_data` units, indexed by unit name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrefabDefinitions {
    models: BTreeMap<String, PrefabModelDef>,
    rules: BTreeMap<String, TrafficRuleDef>,
}

impl PrefabDefinitions {
    /// Collects `prefab_model` and `traffic_rule_data` units. Other units
    /// are ignored.
    ///
    /// # Errors
    ///
    /// [`DefinitionsError`] for unreadable or duplicated units.
    pub fn from_units<'a>(
        units: impl IntoIterator<Item = &'a SiiUnit>,
    ) -> Result<Self, DefinitionsError> {
        let mut defs = Self::default();
        for unit in units {
            if let Some(model) = PrefabModelDef::from_unit(unit) {
                let model = model.map_err(DefinitionsError::Invalid)?;
                if defs.models.insert(model.name.clone(), model).is_some() {
                    return Err(DefinitionsError::Duplicate {
                        class: "prefab_model",
                        name: unit.name.clone(),
                    });
                }
            } else if let Some(rule) = TrafficRuleDef::from_unit(unit) {
                if defs.rules.insert(rule.name.clone(), rule).is_some() {
                    return Err(DefinitionsError::Duplicate {
                        class: "traffic_rule_data",
                        name: unit.name.clone(),
                    });
                }
            }
        }
        Ok(defs)
    }

    /// Number of prefab models and traffic rules.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        (self.models.len(), self.rules.len())
    }

    /// The model with this unit name.
    #[must_use]
    pub fn model(&self, name: &str) -> Option<&PrefabModelDef> {
        self.models.get(name)
    }

    /// The traffic rule with this unit name.
    #[must_use]
    pub fn rule(&self, name: &str) -> Option<&TrafficRuleDef> {
        self.rules.get(name)
    }
}

fn token_name(prefix: &str, token: Token) -> String {
    format!("{prefix}.{}", token.name().unwrap_or_default())
}

fn definition(model: &str, d: &PrefabDescriptor, defs: &PrefabDefinitions) -> PrefabDefinition {
    PrefabDefinition {
        model: model.to_owned(),
        nodes: d
            .nodes
            .iter()
            .map(|n| PrefabDefinitionNode {
                input_slots: n.input_slots,
                output_slots: n.output_slots,
            })
            .collect(),
        curves: d
            .curves
            .iter()
            .map(|c| PrefabCurve {
                next: c.next.clone(),
                prev: c.prev.clone(),
                traffic_rule: c.traffic_rule.map(|t| {
                    let name = token_name("traffic_rule", t);
                    if defs.rules.contains_key(&name) {
                        RuleRef::Resolved(name)
                    } else {
                        RuleRef::Unresolved(name)
                    }
                }),
            })
            .collect(),
        semaphores: d
            .semaphores
            .iter()
            .map(|s| PrefabSemaphore {
                position: s.position,
                rotation: s.rotation,
                profile_token: s.profile.0,
            })
            .collect(),
    }
}

/// Largest allowed difference between a horizontal node distance on the map
/// and in the descriptor, world units.
const GEOMETRY_TOLERANCE: f64 = 0.05;

/// Whether the mapping of instance node `i` to descriptor node
/// `(i + origin) % n` keeps every horizontal (X, Z) distance between two
/// nodes. Heights are not compared. Missing nodes are left to the network
/// validation.
fn geometry_fits(network: &RoadNetwork, nodes: &[u64], origin: u8, d: &PrefabDescriptor) -> bool {
    let n = nodes.len();
    let Some(map) = nodes
        .iter()
        .map(|&u| network.node(NodeId(u)).map(|node| node.position.world()))
        .collect::<Option<Vec<_>>>()
    else {
        return true;
    };
    let def = |i: usize| {
        d.nodes[(i + usize::from(origin)) % n]
            .position
            .map(f64::from)
    };
    let horizontal = |a: [f64; 3], b: [f64; 3]| (a[0] - b[0]).hypot(a[2] - b[2]);
    (0..n).all(|i| {
        (i + 1..n).all(|j| {
            (horizontal(map[i], map[j]) - horizontal(def(i), def(j))).abs() < GEOMETRY_TOLERANCE
        })
    })
}

/// Builds the [`PrefabNetwork`] of the prefab items in `sectors`.
///
/// `load` returns the bytes of an archive file (the `.ppd` descriptor of a
/// prefab model), or `None` if it does not exist. Each descriptor is loaded
/// and parsed once. An instance whose model, descriptor or node mapping
/// cannot be established is kept with the reason in
/// [`PrefabInstance::resolution`]; nothing is guessed.
///
/// # Errors
///
/// The [`PrefabError`] of [`PrefabNetwork::new`], for example a prefab node
/// that does not refer back to its prefab.
pub fn build_prefab_network(
    network: &RoadNetwork,
    sectors: &[BaseSector<'_>],
    defs: &PrefabDefinitions,
    mut load: impl FnMut(&str) -> Option<Vec<u8>>,
) -> Result<PrefabNetwork, PrefabError> {
    // Descriptor per path: parsed, or the reason it is not available.
    let mut descriptors: BTreeMap<String, Result<PrefabDescriptor, PrefabResolution>> =
        BTreeMap::new();
    let mut definitions: BTreeMap<String, PrefabDefinition> = BTreeMap::new();
    let mut instances = Vec::new();
    for item in sectors.iter().flat_map(|s| &s.items) {
        let ItemData::Prefab(prefab) = &item.data else {
            continue;
        };
        let model = token_name("prefab", prefab.model);
        let resolution = match defs.models.get(&model) {
            None => PrefabResolution::MissingModel,
            Some(m) => {
                let parsed = descriptors
                    .entry(m.descriptor.clone())
                    .or_insert_with(|| match load(&m.descriptor) {
                        None => Err(PrefabResolution::MissingDescriptor(m.descriptor.clone())),
                        Some(bytes) => parse_ppd(&bytes)
                            .map_err(|e| PrefabResolution::InvalidDescriptor(e.to_string())),
                    });
                match parsed {
                    Err(reason) => reason.clone(),
                    Ok(d) if d.nodes.len() != prefab.nodes.len() => {
                        PrefabResolution::NodeCountMismatch {
                            instance: prefab.nodes.len(),
                            definition: d.nodes.len(),
                        }
                    }
                    Ok(d) if usize::from(prefab.origin) >= d.nodes.len() => {
                        PrefabResolution::OriginOutOfRange(prefab.origin)
                    }
                    Ok(d) if !geometry_fits(network, &prefab.nodes, prefab.origin, d) => {
                        PrefabResolution::NodeGeometryMismatch
                    }
                    Ok(d) => {
                        definitions
                            .entry(model.clone())
                            .or_insert_with(|| definition(&model, d, defs));
                        PrefabResolution::Resolved
                    }
                }
            }
        };
        instances.push(PrefabInstance {
            id: PrefabId(item.uid),
            model,
            nodes: prefab.nodes.iter().map(|&n| NodeId(n)).collect(),
            origin: prefab.origin,
            resolution,
        });
    }
    PrefabNetwork::new(network, definitions.into_values(), instances)
}
