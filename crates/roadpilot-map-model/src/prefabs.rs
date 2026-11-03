//! Prefab instances (junctions and other road pieces), their definitions,
//! and the lane movements through them.
//!
//! SCS has no separate junction record: a junction is a **prefab instance**,
//! and larger junctions are several prefabs joined node to node. A prefab
//! **definition** (from a `.ppd` descriptor) describes the prefab in local
//! coordinates; an **instance** places it on the map with its own node UIDs.
//!
//! Movement status (see `docs/formats/scs-prefab-junctions.md`):
//!
//! - [`PrefabMovement`]: a path through the prefab's navigation curves from
//!   an input slot to an output slot. The curve links are stored explicitly,
//!   so these movements are **confirmed** as stored connections. Whether a
//!   vehicle may use them also depends on traffic rules, which are attached
//!   but not interpreted.
//! - [`LaneMovement`]: a road lane into the prefab, a confirmed internal
//!   movement, and a road lane out of it. Road lanes are paired with slots
//!   by index, and the lane order is not confirmed, so these are
//!   **candidates**.
//! - Everything that cannot be decided is reported with a reason. The
//!   format has no confirmed way to mark a movement as forbidden, so no
//!   movement is reported as forbidden.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::lanes::{LaneConfig, LaneDirection, LaneId, LaneNetwork, TrafficSideState};
use crate::network::{NodeId, NodeLink, RoadId, RoadNetwork};

/// Number of input and output lane slots of a prefab node.
pub const PREFAB_LANE_SLOTS: usize = 8;

/// UID of a prefab instance (its map item UID).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrefabId(pub u64);

/// A traffic rule reference, as a `traffic_rule_data` unit name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RuleRef {
    /// The rule is defined.
    Resolved(String),
    /// The name is referenced but no such rule is defined.
    Unresolved(String),
}

/// A node of a prefab definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefabDefinitionNode {
    /// Curves that start here: lanes entering the prefab.
    pub input_slots: [Option<u16>; PREFAB_LANE_SLOTS],
    /// Curves that end here: lanes leaving the prefab.
    pub output_slots: [Option<u16>; PREFAB_LANE_SLOTS],
}

/// A navigation curve of a prefab definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefabCurve {
    pub next: Vec<u16>,
    pub prev: Vec<u16>,
    pub traffic_rule: Option<RuleRef>,
}

/// A static traffic light of a prefab definition (local coordinates). Its
/// current state is runtime information and not part of the map data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrefabSemaphore {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    /// Token of the profile, as stored. Which `tr_semaphore_profile` unit it
    /// selects is not confirmed.
    pub profile_token: u64,
}

/// A prefab definition, keyed by its `prefab_model` name.
#[derive(Debug, Clone, PartialEq)]
pub struct PrefabDefinition {
    pub model: String,
    pub nodes: Vec<PrefabDefinitionNode>,
    pub curves: Vec<PrefabCurve>,
    pub semaphores: Vec<PrefabSemaphore>,
}

/// Whether an instance could be tied to its definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefabResolution {
    Resolved,
    /// The `prefab_model` is not defined.
    MissingModel,
    /// The model's descriptor file does not exist.
    MissingDescriptor(String),
    /// The descriptor could not be parsed.
    InvalidDescriptor(String),
    /// The instance has another number of nodes than the definition.
    NodeCountMismatch {
        instance: usize,
        definition: usize,
    },
    /// The stored origin is not a node index of the definition.
    OriginOutOfRange(u8),
    /// Mapping instance nodes to definition nodes by the origin does not keep
    /// the horizontal distances between the nodes, so the mapping is not
    /// trusted.
    NodeGeometryMismatch,
}

/// A prefab placed on the map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefabInstance {
    pub id: PrefabId,
    /// `prefab_model` name, `prefab.<token>`.
    pub model: String,
    /// Instance nodes in stored order.
    pub nodes: Vec<NodeId>,
    /// Instance node `i` is definition node `(i + origin) % n`.
    pub origin: u8,
    pub resolution: PrefabResolution,
}

impl PrefabInstance {
    /// The definition node of instance node `index`, if resolved.
    #[must_use]
    pub fn definition_node(&self, index: usize) -> Option<usize> {
        let n = self.nodes.len();
        (self.resolution == PrefabResolution::Resolved && index < n)
            .then(|| (index + usize::from(self.origin)) % n)
    }

    /// The instance node of definition node `definition`, if resolved.
    #[must_use]
    pub fn instance_node(&self, definition: usize) -> Option<usize> {
        let n = self.nodes.len();
        (self.resolution == PrefabResolution::Resolved && definition < n)
            .then(|| (definition + n - usize::from(self.origin) % n) % n)
    }
}

/// What lies on the other side of a prefab node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArmLink {
    /// A road; `road_ends_here` is true if the node is the road's end node.
    Road { road: RoadId, road_ends_here: bool },
    /// Another prefab instance joined directly.
    Prefab(PrefabId),
    /// Another existing item that is neither a road nor a prefab.
    Item(u64),
    /// A UID that matches no loaded item.
    Unresolved(u64),
    /// Nothing.
    None,
}

/// One node of a prefab instance and what is attached to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrefabArm {
    /// Index of the node in the instance.
    pub index: usize,
    pub node: NodeId,
    pub definition_node: Option<usize>,
    pub link: ArmLink,
}

/// A stored path through a prefab from an input slot to an output slot
/// (confirmed by the explicit curve links).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrefabMovement {
    /// Instance node index and input slot where the path starts.
    pub from_arm: usize,
    pub from_slot: usize,
    /// Instance node index and output slot where the path ends.
    pub to_arm: usize,
    pub to_slot: usize,
    /// Traffic rules on any curve of any path between the two slots.
    pub traffic_rules: Vec<RuleRef>,
}

/// Status of a lane movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MovementStatus {
    /// Stored explicitly in the game data.
    Confirmed,
    /// Plausible, but at least one part is not confirmed.
    Candidate,
}

/// A movement from a road lane through a prefab into another road lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneMovement {
    pub prefab: PrefabId,
    pub from: LaneId,
    pub to: LaneId,
    /// The internal movement it uses.
    pub internal: PrefabMovement,
    pub status: MovementStatus,
}

/// Why the road lanes at a prefab arm cannot be paired with its slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArmIssue {
    /// No road is attached to this node.
    NoRoad,
    /// The prefab is not resolved.
    PrefabUnresolved,
    /// The road's lanes are unresolved or its traffic side is unknown.
    RoadLanesUnknown,
    /// The number of road lanes differs from the number of slots.
    LaneCountMismatch {
        direction: SlotDirection,
        lanes: usize,
        slots: usize,
    },
}

/// Input slots (into the prefab) or output slots (out of it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlotDirection {
    Input,
    Output,
}

/// Lane movements through one prefab.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrefabLaneMovements {
    pub movements: Vec<LaneMovement>,
    /// Arms whose road lanes could not be paired, with the reason.
    pub unpaired_arms: Vec<(usize, ArmIssue)>,
}

/// Why a [`PrefabNetwork`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PrefabError {
    DuplicateInstance(PrefabId),
    DuplicateDefinition(String),
    /// An instance node is not in the road network.
    MissingNode {
        prefab: PrefabId,
        node: NodeId,
    },
    /// An instance node does not refer back to the instance.
    NodeNotLinked {
        prefab: PrefabId,
        node: NodeId,
    },
    /// A resolved instance has no consistent definition.
    InconsistentResolution(PrefabId),
    /// A definition has out-of-range indices or unmirrored links.
    InvalidDefinition {
        model: String,
        reason: &'static str,
    },
}

impl fmt::Display for PrefabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateInstance(p) => write!(f, "prefab {:016x} occurs twice", p.0),
            Self::DuplicateDefinition(m) => write!(f, "prefab definition {m:?} occurs twice"),
            Self::MissingNode { prefab, node } => write!(
                f,
                "prefab {:016x} refers to missing node {:016x}",
                prefab.0, node.0
            ),
            Self::NodeNotLinked { prefab, node } => write!(
                f,
                "node {:016x} does not refer back to prefab {:016x}",
                node.0, prefab.0
            ),
            Self::InconsistentResolution(p) => {
                write!(
                    f,
                    "prefab {:016x} is marked resolved without a matching definition",
                    p.0
                )
            }
            Self::InvalidDefinition { model, reason } => {
                write!(f, "prefab definition {model:?}: {reason}")
            }
        }
    }
}

impl std::error::Error for PrefabError {}

fn check_definition(def: &PrefabDefinition) -> Result<(), PrefabError> {
    let invalid = |reason| PrefabError::InvalidDefinition {
        model: def.model.clone(),
        reason,
    };
    let n = def.curves.len();
    let in_range = |c: &u16| usize::from(*c) < n;
    for node in &def.nodes {
        if !node
            .input_slots
            .iter()
            .chain(&node.output_slots)
            .flatten()
            .all(in_range)
        {
            return Err(invalid("a node slot refers to a missing curve"));
        }
    }
    for (i, curve) in def.curves.iter().enumerate() {
        if !curve.next.iter().chain(&curve.prev).all(in_range) {
            return Err(invalid("a curve link refers to a missing curve"));
        }
        let mirrored = curve.next.iter().all(|&nx| {
            def.curves[usize::from(nx)]
                .prev
                .iter()
                .any(|&p| usize::from(p) == i)
        });
        if !mirrored {
            return Err(invalid("a curve link is not mirrored"));
        }
    }
    Ok(())
}

/// Curves reachable from `start` following `links` (iterative, bounded by
/// the number of curves).
fn reachable(def: &PrefabDefinition, start: u16, forward: bool) -> BTreeSet<u16> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![start];
    while let Some(c) = stack.pop() {
        if !seen.insert(c) {
            continue;
        }
        if let Some(curve) = def.curves.get(usize::from(c)) {
            let links = if forward { &curve.next } else { &curve.prev };
            stack.extend(links.iter().copied().filter(|l| !seen.contains(l)));
        }
    }
    seen
}

/// Prefab definitions and the prefab instances of a map.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PrefabNetwork {
    definitions: BTreeMap<String, PrefabDefinition>,
    instances: BTreeMap<PrefabId, PrefabInstance>,
}

impl PrefabNetwork {
    /// Builds and validates the prefab network for `roads`.
    ///
    /// Every instance node must exist in `roads` and refer back to the
    /// instance in one of its slots. Resolved instances need a valid
    /// definition with the same number of nodes and an origin below it.
    ///
    /// # Errors
    ///
    /// The first [`PrefabError`] found.
    pub fn new(
        roads: &RoadNetwork,
        definitions: impl IntoIterator<Item = PrefabDefinition>,
        instances: impl IntoIterator<Item = PrefabInstance>,
    ) -> Result<Self, PrefabError> {
        let mut defs = BTreeMap::new();
        for def in definitions {
            check_definition(&def)?;
            let model = def.model.clone();
            if defs.insert(model.clone(), def).is_some() {
                return Err(PrefabError::DuplicateDefinition(model));
            }
        }
        let mut map = BTreeMap::new();
        for inst in instances {
            for &node in &inst.nodes {
                let n = roads.node(node).ok_or(PrefabError::MissingNode {
                    prefab: inst.id,
                    node,
                })?;
                let own = NodeLink::Item(inst.id.0);
                if n.backward != own && n.forward != own {
                    return Err(PrefabError::NodeNotLinked {
                        prefab: inst.id,
                        node,
                    });
                }
            }
            if inst.resolution == PrefabResolution::Resolved {
                let consistent = defs.get(&inst.model).is_some_and(|d| {
                    d.nodes.len() == inst.nodes.len() && usize::from(inst.origin) < d.nodes.len()
                });
                if !consistent {
                    return Err(PrefabError::InconsistentResolution(inst.id));
                }
            }
            let id = inst.id;
            if map.insert(id, inst).is_some() {
                return Err(PrefabError::DuplicateInstance(id));
            }
        }
        Ok(Self {
            definitions: defs,
            instances: map,
        })
    }

    #[must_use]
    pub fn instance(&self, id: PrefabId) -> Option<&PrefabInstance> {
        self.instances.get(&id)
    }

    /// All instances, ordered by UID.
    pub fn instances(&self) -> impl Iterator<Item = &PrefabInstance> {
        self.instances.values()
    }

    #[must_use]
    pub fn definition(&self, model: &str) -> Option<&PrefabDefinition> {
        self.definitions.get(model)
    }

    /// Number of definitions.
    #[must_use]
    pub fn definition_count(&self) -> usize {
        self.definitions.len()
    }

    fn resolved(&self, id: PrefabId) -> Option<(&PrefabInstance, &PrefabDefinition)> {
        let inst = self.instances.get(&id)?;
        (inst.resolution == PrefabResolution::Resolved)
            .then(|| self.definitions.get(&inst.model).map(|d| (inst, d)))
            .flatten()
    }

    /// The nodes of a prefab instance and what is attached to each.
    #[must_use]
    pub fn arms(&self, roads: &RoadNetwork, id: PrefabId) -> Vec<PrefabArm> {
        let Some(inst) = self.instances.get(&id) else {
            return Vec::new();
        };
        let own = NodeLink::Item(id.0);
        inst.nodes
            .iter()
            .enumerate()
            .map(|(index, &node)| {
                let link = roads.node(node).map_or(ArmLink::None, |n| {
                    let other = if n.forward == own {
                        n.backward
                    } else {
                        n.forward
                    };
                    match other {
                        NodeLink::None => ArmLink::None,
                        NodeLink::Road(road) => ArmLink::Road {
                            road,
                            road_ends_here: roads.road(road).is_some_and(|r| r.end_node == node),
                        },
                        NodeLink::Item(uid) if self.instances.contains_key(&PrefabId(uid)) => {
                            ArmLink::Prefab(PrefabId(uid))
                        }
                        NodeLink::Item(uid) => ArmLink::Item(uid),
                        NodeLink::Unresolved(uid) => ArmLink::Unresolved(uid),
                    }
                });
                PrefabArm {
                    index,
                    node,
                    definition_node: inst.definition_node(index),
                    link,
                }
            })
            .collect()
    }

    /// The stored internal movements of a resolved prefab, ordered by input
    /// arm and slot. Empty for unresolved prefabs.
    #[must_use]
    pub fn movements(&self, id: PrefabId) -> Vec<PrefabMovement> {
        let Some((inst, def)) = self.resolved(id) else {
            return Vec::new();
        };
        // Curves that end at an output slot, with their (definition node, slot).
        let mut outputs: Vec<(usize, usize, u16)> = Vec::new();
        for (d, node) in def.nodes.iter().enumerate() {
            for (s, c) in node.output_slots.iter().enumerate() {
                if let Some(c) = c {
                    outputs.push((d, s, *c));
                }
            }
        }
        let backward: Vec<BTreeSet<u16>> = outputs
            .iter()
            .map(|&(_, _, c)| reachable(def, c, false))
            .collect();
        let mut result = Vec::new();
        for (d_in, node) in def.nodes.iter().enumerate() {
            for (s_in, start) in node.input_slots.iter().enumerate() {
                let Some(start) = *start else { continue };
                let forward = reachable(def, start, true);
                for (k, &(d_out, s_out, end)) in outputs.iter().enumerate() {
                    if !forward.contains(&end) {
                        continue;
                    }
                    let rules: BTreeSet<RuleRef> = forward
                        .intersection(&backward[k])
                        .filter_map(|&c| def.curves.get(usize::from(c))?.traffic_rule.clone())
                        .collect();
                    let (Some(from_arm), Some(to_arm)) =
                        (inst.instance_node(d_in), inst.instance_node(d_out))
                    else {
                        continue;
                    };
                    result.push(PrefabMovement {
                        from_arm,
                        from_slot: s_in,
                        to_arm,
                        to_slot: s_out,
                        traffic_rules: rules.into_iter().collect(),
                    });
                }
            }
        }
        result.sort_by_key(|m| (m.from_arm, m.from_slot, m.to_arm, m.to_slot));
        result
    }

    /// Road lanes paired with the slots of one arm: `(slot, lane)` for the
    /// given slot direction, or why they cannot be paired.
    fn pair_arm(
        lanes: &LaneNetwork,
        def: &PrefabDefinition,
        arm: &PrefabArm,
        direction: SlotDirection,
    ) -> Result<Vec<(usize, LaneId)>, ArmIssue> {
        let ArmLink::Road {
            road,
            road_ends_here,
        } = arm.link
        else {
            return Err(ArmIssue::NoRoad);
        };
        let d = arm.definition_node.ok_or(ArmIssue::PrefabUnresolved)?;
        let node = def.nodes.get(d).ok_or(ArmIssue::PrefabUnresolved)?;
        let entry = lanes.road(road).ok_or(ArmIssue::RoadLanesUnknown)?;
        // Lanes driving into the prefab run forward at the road's end node.
        let lane_direction = match (direction, road_ends_here) {
            (SlotDirection::Input, true) | (SlotDirection::Output, false) => LaneDirection::Forward,
            _ => LaneDirection::Backward,
        };
        if !matches!(entry.config, LaneConfig::Resolved(_))
            || !matches!(entry.traffic_side, TrafficSideState::Known(_))
        {
            return Err(ArmIssue::RoadLanesUnknown);
        }
        let road_lanes = entry.lanes_in(lane_direction);
        let slots = match direction {
            SlotDirection::Input => &node.input_slots,
            SlotDirection::Output => &node.output_slots,
        };
        let used: Vec<usize> = slots
            .iter()
            .enumerate()
            .filter_map(|(s, c)| c.map(|_| s))
            .collect();
        if used.len() != road_lanes.len() {
            return Err(ArmIssue::LaneCountMismatch {
                direction,
                lanes: road_lanes.len(),
                slots: used.len(),
            });
        }
        Ok(used
            .into_iter()
            .zip(road_lanes.iter().map(|l| l.id))
            .collect())
    }

    /// The used slots of instance node `arm` of a resolved prefab, in slot
    /// order. Empty for unresolved prefabs and unknown nodes.
    #[must_use]
    pub fn used_slots(&self, id: PrefabId, arm: usize, direction: SlotDirection) -> Vec<usize> {
        let Some((inst, def)) = self.resolved(id) else {
            return Vec::new();
        };
        let Some(node) = inst.definition_node(arm).and_then(|d| def.nodes.get(d)) else {
            return Vec::new();
        };
        let slots = match direction {
            SlotDirection::Input => &node.input_slots,
            SlotDirection::Output => &node.output_slots,
        };
        slots
            .iter()
            .enumerate()
            .filter_map(|(s, c)| c.map(|_| s))
            .collect()
    }

    /// The road lanes paired with the used slots of `arm`, as `(slot, lane)`,
    /// or why they cannot be paired. The pairing is by index and therefore a
    /// candidate (open question LM2).
    ///
    /// # Errors
    ///
    /// The [`ArmIssue`] that prevents the pairing.
    pub fn slot_lanes(
        &self,
        lanes: &LaneNetwork,
        id: PrefabId,
        arm: &PrefabArm,
        direction: SlotDirection,
    ) -> Result<Vec<(usize, LaneId)>, ArmIssue> {
        let (_, def) = self.resolved(id).ok_or(ArmIssue::PrefabUnresolved)?;
        Self::pair_arm(lanes, def, arm, direction)
    }

    /// Candidate lane movements from road lanes through the prefab into road
    /// lanes, and the arms whose lanes could not be paired.
    #[must_use]
    pub fn lane_movements(
        &self,
        roads: &RoadNetwork,
        lanes: &LaneNetwork,
        id: PrefabId,
    ) -> PrefabLaneMovements {
        let mut out = PrefabLaneMovements::default();
        let Some((_, def)) = self.resolved(id) else {
            out.unpaired_arms = (0..self.instances.get(&id).map_or(0, |i| i.nodes.len()))
                .map(|i| (i, ArmIssue::PrefabUnresolved))
                .collect();
            return out;
        };
        let arms = self.arms(roads, id);
        let mut inputs = BTreeMap::new();
        let mut outputs = BTreeMap::new();
        for arm in &arms {
            for (direction, target) in [
                (SlotDirection::Input, &mut inputs),
                (SlotDirection::Output, &mut outputs),
            ] {
                match Self::pair_arm(lanes, def, arm, direction) {
                    Ok(pairs) => {
                        target.insert(arm.index, pairs);
                    }
                    Err(issue) => {
                        if !out.unpaired_arms.contains(&(arm.index, issue)) {
                            out.unpaired_arms.push((arm.index, issue));
                        }
                    }
                }
            }
        }
        let find = |map: &BTreeMap<usize, Vec<(usize, LaneId)>>, arm: usize, slot: usize| {
            map.get(&arm)
                .and_then(|pairs| pairs.iter().find(|(s, _)| *s == slot).map(|(_, l)| *l))
        };
        for internal in self.movements(id) {
            let (Some(from), Some(to)) = (
                find(&inputs, internal.from_arm, internal.from_slot),
                find(&outputs, internal.to_arm, internal.to_slot),
            ) else {
                continue;
            };
            out.movements.push(LaneMovement {
                prefab: id,
                from,
                to,
                internal,
                status: MovementStatus::Candidate,
            });
        }
        out
    }
}
