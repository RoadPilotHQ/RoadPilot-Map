//! The navigation graph: lanes and prefab slots as nodes, connections with
//! their status and origin as directed edges.
//!
//! The graph only restates what the road, lane and prefab models know. It
//! never upgrades a connection: a stored internal prefab movement is
//! [`ConnectionStatus::Confirmed`], index-paired lanes are
//! [`ConnectionStatus::Candidate`], and joints whose lane pairing is not
//! known are [`ConnectionStatus::Unknown`]. See
//! `docs/navigation/navigation-graph.md`.

use std::collections::HashMap;
use std::fmt;

use crate::lanes::{
    LaneConfig, LaneId, LaneNetwork, LaneTransition, TrafficSideState, TransitionIssue,
};
use crate::network::{NodeLink, RoadNetwork};
use crate::prefabs::SlotDirection;
use crate::prefabs::{ArmIssue, ArmLink, PrefabId, PrefabNetwork, PrefabResolution, RuleRef};

/// Index of a node in a [`NavigationGraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NavNodeId(pub(crate) u32);

impl NavNodeId {
    /// The position of the node in [`NavigationGraph`] order.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// Index of an edge in a [`NavigationGraph`], in edge order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeId(pub u32);

/// A slot of a prefab node: the place where a lane enters or leaves the
/// prefab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrefabSlotRef {
    pub prefab: PrefabId,
    /// Index of the node in the prefab instance.
    pub arm: u16,
    pub slot: u8,
}

/// A place in the navigation graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NavNode {
    /// Where traffic enters a lane, in the lane's driving direction.
    LaneEntry(LaneId),
    /// Where traffic leaves a lane.
    LaneExit(LaneId),
    /// A lane enters the prefab here.
    PrefabInput(PrefabSlotRef),
    /// A lane leaves the prefab here.
    PrefabOutput(PrefabSlotRef),
}

/// How well a connection is established by the game data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ConnectionStatus {
    /// Stored explicitly in the game data.
    Confirmed,
    /// Plausible, but a part of it is not confirmed.
    Candidate,
    /// The data does not say whether the connection exists.
    Unknown,
    /// Explicitly excluded by the game data. No source of such data is known,
    /// so the builder never produces it.
    Forbidden,
}

/// What kind of record a connection comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EdgeOriginKind {
    /// Driving along a lane of a road. `source` is the road UID.
    RoadLane,
    /// A road-to-road connection at a shared node. `source` is the node UID.
    RoadConnection,
    /// A road lane paired with a prefab input slot. `source` is the prefab
    /// node UID.
    RoadToPrefabSlot,
    /// A stored path through the curves of a prefab. `source` is the prefab
    /// UID.
    PrefabInternalCurve,
    /// A prefab output slot paired with a road lane. `source` is the prefab
    /// node UID.
    PrefabSlotToRoad,
    /// Two prefabs joined at a node. `source` is the node UID.
    PrefabToPrefab,
    /// Any other source; `source` is defined by the caller.
    Other,
}

/// The record a connection was derived from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeOrigin {
    pub kind: EdgeOriginKind,
    /// UID of the record, see [`EdgeOriginKind`].
    pub source: u64,
}

/// A directed connection. Its cost for routing is one edge; it has no
/// physical length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NavEdge {
    from: NavNodeId,
    to: NavNodeId,
    rule_set: u32,
    source: u64,
    kind: EdgeOriginKind,
    status: ConnectionStatus,
}

const NO_RULES: u32 = u32::MAX;

const _: () = assert!(std::mem::size_of::<NavEdge>() == 24);

impl NavEdge {
    #[must_use]
    pub fn from(&self) -> NavNodeId {
        self.from
    }

    #[must_use]
    pub fn to(&self) -> NavNodeId {
        self.to
    }

    #[must_use]
    pub fn status(&self) -> ConnectionStatus {
        self.status
    }

    #[must_use]
    pub fn origin(&self) -> EdgeOrigin {
        EdgeOrigin {
            kind: self.kind,
            source: self.source,
        }
    }

    fn sort_key(
        &self,
    ) -> (
        NavNodeId,
        NavNodeId,
        EdgeOriginKind,
        u64,
        ConnectionStatus,
        u32,
    ) {
        (
            self.from,
            self.to,
            self.kind,
            self.source,
            self.status,
            self.rule_set,
        )
    }
}

/// What the builder could not turn into a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GraphIssueKind {
    /// The lanes of the road are unresolved or its traffic side is unknown,
    /// so its lanes have no driving direction and are not in the graph.
    /// `source` is the road UID.
    RoadLanesNotNavigable,
    /// Two roads meet, but their lanes cannot be paired. `source` is the node
    /// UID.
    RoadTransitionUnpaired(TransitionIssue),
    /// The prefab could not be tied to its definition. `source` is the
    /// prefab UID.
    PrefabUnresolved,
    /// A road lane cannot be paired with the slots of a prefab node.
    /// `source` is the prefab UID.
    PrefabArmUnpaired { arm: u16, issue: ArmIssue },
    /// A node refers to a UID that matches no loaded item. `source` is the
    /// node UID.
    UnresolvedNodeLink,
    /// A node refers to an item that is neither a road nor a prefab. `source`
    /// is the node UID.
    NodeLinksToOtherItem,
}

/// A connection or lane that is missing from the graph, with the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GraphIssue {
    pub kind: GraphIssueKind,
    pub source: u64,
}

/// Why a [`NavigationGraph`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum NavigationError {
    DuplicateNode(NavNode),
    /// An edge refers to a node that was not added.
    MissingNode(NavNode),
    /// A forbidden connection runs parallel to another connection between
    /// the same nodes.
    ConflictingForbidden {
        from: NavNode,
        to: NavNode,
    },
    /// A count does not fit the 32-bit index space.
    TooLarge(&'static str),
}

impl fmt::Display for NavigationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateNode(n) => write!(f, "navigation node {n:?} occurs twice"),
            Self::MissingNode(n) => write!(f, "edge refers to missing navigation node {n:?}"),
            Self::ConflictingForbidden { from, to } => write!(
                f,
                "forbidden connection {from:?} -> {to:?} conflicts with another connection"
            ),
            Self::TooLarge(what) => write!(f, "too many {what} for the navigation graph"),
        }
    }
}

impl std::error::Error for NavigationError {}

/// Collects nodes, edges and issues, then builds a [`NavigationGraph`].
#[derive(Debug, Default)]
pub struct NavigationGraphBuilder {
    nodes: Vec<NavNode>,
    index: HashMap<NavNode, NavNodeId>,
    edges: Vec<NavEdge>,
    rule_sets: Vec<Box<[RuleRef]>>,
    rule_index: HashMap<Box<[RuleRef]>, u32>,
    issues: Vec<GraphIssue>,
}

impl NavigationGraphBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a node.
    ///
    /// # Errors
    ///
    /// [`NavigationError::DuplicateNode`] if it exists, and
    /// [`NavigationError::TooLarge`] beyond `u32::MAX` nodes.
    pub fn add_node(&mut self, node: NavNode) -> Result<NavNodeId, NavigationError> {
        if self.index.contains_key(&node) {
            return Err(NavigationError::DuplicateNode(node));
        }
        self.ensure_node(node)
    }

    fn ensure_node(&mut self, node: NavNode) -> Result<NavNodeId, NavigationError> {
        if let Some(&id) = self.index.get(&node) {
            return Ok(id);
        }
        let id = NavNodeId(
            u32::try_from(self.nodes.len()).map_err(|_| NavigationError::TooLarge("nodes"))?,
        );
        if id.0 == u32::MAX {
            return Err(NavigationError::TooLarge("nodes"));
        }
        self.nodes.push(node);
        self.index.insert(node, id);
        Ok(id)
    }

    /// Adds a directed connection between two existing nodes. `rules` are the
    /// traffic rules stored with it; they are kept as references and never
    /// interpreted.
    ///
    /// # Errors
    ///
    /// [`NavigationError::MissingNode`] if a node was not added.
    pub fn add_edge(
        &mut self,
        from: NavNode,
        to: NavNode,
        status: ConnectionStatus,
        origin: EdgeOrigin,
        rules: &[RuleRef],
    ) -> Result<(), NavigationError> {
        let f = *self
            .index
            .get(&from)
            .ok_or(NavigationError::MissingNode(from))?;
        let t = *self
            .index
            .get(&to)
            .ok_or(NavigationError::MissingNode(to))?;
        let rule_set = if rules.is_empty() {
            NO_RULES
        } else if let Some(&id) = self.rule_index.get(rules) {
            id
        } else {
            let id = u32::try_from(self.rule_sets.len())
                .ok()
                .filter(|&i| i != NO_RULES)
                .ok_or(NavigationError::TooLarge("rule sets"))?;
            let boxed: Box<[RuleRef]> = rules.into();
            self.rule_sets.push(boxed.clone());
            self.rule_index.insert(boxed, id);
            id
        };
        self.edges.push(NavEdge {
            from: f,
            to: t,
            rule_set,
            source: origin.source,
            kind: origin.kind,
            status,
        });
        Ok(())
    }

    /// Records something that is missing from the graph.
    pub fn add_issue(&mut self, issue: GraphIssue) {
        self.issues.push(issue);
    }

    /// Builds the graph. Edges are ordered by their start node, then
    /// deterministically by everything else. Identical edges are merged and
    /// counted. A forbidden edge may not run parallel to another edge.
    ///
    /// # Errors
    ///
    /// [`NavigationError::ConflictingForbidden`] and
    /// [`NavigationError::TooLarge`].
    pub fn finish(mut self) -> Result<NavigationGraph, NavigationError> {
        self.edges.sort_unstable_by_key(NavEdge::sort_key);
        let before = self.edges.len();
        self.edges.dedup();
        let merged_duplicates = before - self.edges.len();
        u32::try_from(self.edges.len()).map_err(|_| NavigationError::TooLarge("edges"))?;

        for group in self.edges.chunk_by(|a, b| a.from == b.from && a.to == b.to) {
            let forbidden = group
                .iter()
                .filter(|e| e.status == ConnectionStatus::Forbidden)
                .count();
            if forbidden > 0 && forbidden != group.len() {
                return Err(NavigationError::ConflictingForbidden {
                    from: self.nodes[group[0].from.index()],
                    to: self.nodes[group[0].to.index()],
                });
            }
        }

        // `offsets[n]..offsets[n + 1]` are the edges that start at node `n`.
        let mut offsets = vec![0u32; self.nodes.len() + 1];
        for e in &self.edges {
            offsets[e.from.index() + 1] += 1;
        }
        for i in 0..self.nodes.len() {
            offsets[i + 1] += offsets[i];
        }
        Ok(NavigationGraph {
            nodes: self.nodes,
            index: self.index,
            offsets,
            edges: self.edges,
            rule_sets: self.rule_sets,
            issues: self.issues,
            merged_duplicates,
        })
    }
}

/// A directed graph of lanes and prefab slots.
#[derive(Debug, Clone, PartialEq)]
pub struct NavigationGraph {
    nodes: Vec<NavNode>,
    index: HashMap<NavNode, NavNodeId>,
    offsets: Vec<u32>,
    edges: Vec<NavEdge>,
    rule_sets: Vec<Box<[RuleRef]>>,
    issues: Vec<GraphIssue>,
    merged_duplicates: usize,
}

fn slot_ref(prefab: PrefabId, arm: usize, slot: usize) -> Result<PrefabSlotRef, NavigationError> {
    Ok(PrefabSlotRef {
        prefab,
        arm: u16::try_from(arm).map_err(|_| NavigationError::TooLarge("prefab nodes"))?,
        slot: u8::try_from(slot).map_err(|_| NavigationError::TooLarge("prefab slots"))?,
    })
}

impl NavigationGraph {
    /// Builds the graph of a road network, its lanes and its prefabs.
    ///
    /// - A lane with a known driving direction becomes an entry and an exit
    ///   node joined by a **candidate** edge (the direction rule is measured,
    ///   not confirmed).
    /// - Road connections with paired lanes become **candidate** edges.
    /// - Stored internal prefab movements become **confirmed** edges between
    ///   prefab slots.
    /// - Road lanes paired with prefab slots become **candidate** edges.
    /// - Directly joined prefabs get **unknown** edges between their slots.
    ///
    /// Everything that cannot be connected is listed in
    /// [`NavigationGraph::issues`].
    ///
    /// # Errors
    ///
    /// [`NavigationError`] if the graph does not fit its index space.
    pub fn build(
        roads: &RoadNetwork,
        lanes: &LaneNetwork,
        prefabs: &PrefabNetwork,
    ) -> Result<Self, NavigationError> {
        use ConnectionStatus::{Candidate, Confirmed, Unknown};
        use EdgeOriginKind as K;
        let origin = |kind, source| EdgeOrigin { kind, source };
        let mut b = NavigationGraphBuilder::new();

        // Lanes.
        for entry in lanes.roads() {
            let navigable = matches!(entry.config, LaneConfig::Resolved(_))
                && matches!(entry.traffic_side, TrafficSideState::Known(_));
            if !navigable {
                b.add_issue(GraphIssue {
                    kind: GraphIssueKind::RoadLanesNotNavigable,
                    source: entry.road.0,
                });
                continue;
            }
            for lane in entry.lanes() {
                if entry.direction(lane.id.side).is_none() {
                    continue;
                }
                let (from, to) = (NavNode::LaneEntry(lane.id), NavNode::LaneExit(lane.id));
                b.ensure_node(from)?;
                b.ensure_node(to)?;
                b.add_edge(from, to, Candidate, origin(K::RoadLane, entry.road.0), &[])?;
            }
        }

        // Road-to-road lane transitions.
        for c in roads.connections() {
            match lanes.transition(&c) {
                LaneTransition::IndexCandidates(pairs) => {
                    for p in pairs {
                        b.add_edge(
                            NavNode::LaneExit(p.from),
                            NavNode::LaneEntry(p.to),
                            Candidate,
                            origin(K::RoadConnection, c.via.0),
                            &[],
                        )?;
                    }
                }
                LaneTransition::Unknown(issue) => b.add_issue(GraphIssue {
                    kind: GraphIssueKind::RoadTransitionUnpaired(issue),
                    source: c.via.0,
                }),
            }
        }

        // Nodes that refer to something that is not part of the model.
        for node in roads.nodes() {
            for link in [node.backward, node.forward] {
                let kind = match link {
                    NodeLink::Unresolved(_) => GraphIssueKind::UnresolvedNodeLink,
                    NodeLink::Item(uid) if prefabs.instance(PrefabId(uid)).is_none() => {
                        GraphIssueKind::NodeLinksToOtherItem
                    }
                    _ => continue,
                };
                b.add_issue(GraphIssue {
                    kind,
                    source: node.id.0,
                });
            }
        }

        // Prefab slot nodes first, so that joints can refer to either side.
        for inst in prefabs.instances() {
            if inst.resolution != PrefabResolution::Resolved {
                b.add_issue(GraphIssue {
                    kind: GraphIssueKind::PrefabUnresolved,
                    source: inst.id.0,
                });
                continue;
            }
            for arm in 0..inst.nodes.len() {
                for slot in prefabs.used_slots(inst.id, arm, SlotDirection::Input) {
                    b.ensure_node(NavNode::PrefabInput(slot_ref(inst.id, arm, slot)?))?;
                }
                for slot in prefabs.used_slots(inst.id, arm, SlotDirection::Output) {
                    b.ensure_node(NavNode::PrefabOutput(slot_ref(inst.id, arm, slot)?))?;
                }
            }
        }

        // Prefab connections.
        for inst in prefabs.instances() {
            if inst.resolution != PrefabResolution::Resolved {
                continue;
            }
            for m in prefabs.movements(inst.id) {
                b.add_edge(
                    NavNode::PrefabInput(slot_ref(inst.id, m.from_arm, m.from_slot)?),
                    NavNode::PrefabOutput(slot_ref(inst.id, m.to_arm, m.to_slot)?),
                    Confirmed,
                    origin(K::PrefabInternalCurve, inst.id.0),
                    &m.traffic_rules,
                )?;
            }
            for arm in prefabs.arms(roads, inst.id) {
                match arm.link {
                    ArmLink::Road { .. } => {
                        for direction in [SlotDirection::Input, SlotDirection::Output] {
                            match prefabs.slot_lanes(lanes, inst.id, &arm, direction) {
                                Ok(pairs) => {
                                    for (slot, lane) in pairs {
                                        let s = slot_ref(inst.id, arm.index, slot)?;
                                        if direction == SlotDirection::Input {
                                            b.add_edge(
                                                NavNode::LaneExit(lane),
                                                NavNode::PrefabInput(s),
                                                Candidate,
                                                origin(K::RoadToPrefabSlot, arm.node.0),
                                                &[],
                                            )?;
                                        } else {
                                            b.add_edge(
                                                NavNode::PrefabOutput(s),
                                                NavNode::LaneEntry(lane),
                                                Candidate,
                                                origin(K::PrefabSlotToRoad, arm.node.0),
                                                &[],
                                            )?;
                                        }
                                    }
                                }
                                Err(issue) => b.add_issue(GraphIssue {
                                    kind: GraphIssueKind::PrefabArmUnpaired {
                                        arm: u16::try_from(arm.index).map_err(|_| {
                                            NavigationError::TooLarge("prefab nodes")
                                        })?,
                                        issue,
                                    },
                                    source: inst.id.0,
                                }),
                            }
                        }
                    }
                    ArmLink::Prefab(other) => {
                        let Some(other_arm) = prefabs
                            .instance(other)
                            .and_then(|o| o.nodes.iter().position(|&n| n == arm.node))
                        else {
                            continue;
                        };
                        let outputs = prefabs.used_slots(inst.id, arm.index, SlotDirection::Output);
                        let inputs = prefabs.used_slots(other, other_arm, SlotDirection::Input);
                        for &o in &outputs {
                            for &i in &inputs {
                                b.add_edge(
                                    NavNode::PrefabOutput(slot_ref(inst.id, arm.index, o)?),
                                    NavNode::PrefabInput(slot_ref(other, other_arm, i)?),
                                    Unknown,
                                    origin(K::PrefabToPrefab, arm.node.0),
                                    &[],
                                )?;
                            }
                        }
                    }
                    ArmLink::Item(_) | ArmLink::Unresolved(_) | ArmLink::None => {}
                }
            }
        }
        b.finish()
    }

    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// The node with this ID, if it is in the graph.
    #[must_use]
    pub fn node(&self, id: NavNodeId) -> Option<NavNode> {
        self.nodes.get(id.index()).copied()
    }

    /// The ID of a node, if it is in the graph.
    #[must_use]
    pub fn node_id(&self, node: NavNode) -> Option<NavNodeId> {
        self.index.get(&node).copied()
    }

    /// All nodes, in graph order.
    pub fn nodes(&self) -> impl Iterator<Item = NavNode> + '_ {
        self.nodes.iter().copied()
    }

    /// All edges, ordered by start node.
    pub fn edges(&self) -> impl Iterator<Item = &NavEdge> {
        self.edges.iter()
    }

    #[must_use]
    pub fn edge(&self, id: EdgeId) -> Option<&NavEdge> {
        self.edges.get(id.0 as usize)
    }

    /// The IDs of the edges that start at `node`, empty for unknown nodes.
    pub fn out_edge_ids(&self, node: NavNodeId) -> impl Iterator<Item = EdgeId> {
        let range = match (
            self.offsets.get(node.index()),
            self.offsets.get(node.index() + 1),
        ) {
            (Some(&a), Some(&b)) => a..b,
            _ => 0..0,
        };
        range.map(EdgeId)
    }

    /// The traffic rules stored with an edge, as references only.
    #[must_use]
    pub fn rules(&self, edge: EdgeId) -> &[RuleRef] {
        self.edge(edge)
            .and_then(|e| self.rule_sets.get(e.rule_set as usize))
            .map_or(&[], |r| r)
    }

    /// Everything that is missing from the graph, with the reason.
    #[must_use]
    pub fn issues(&self) -> &[GraphIssue] {
        &self.issues
    }

    /// Number of identical edges that were merged while building.
    #[must_use]
    pub fn merged_duplicates(&self) -> usize {
        self.merged_duplicates
    }

    /// Edge counts by status: confirmed, candidate, unknown, forbidden.
    #[must_use]
    pub fn status_counts(&self) -> [usize; 4] {
        let mut counts = [0; 4];
        for e in &self.edges {
            counts[e.status as usize] += 1;
        }
        counts
    }

    /// Approximate heap memory of the graph in bytes (lower bound: it
    /// ignores allocator overhead and hash map slack).
    #[must_use]
    pub fn approx_heap_bytes(&self) -> usize {
        use std::mem::size_of;
        self.nodes.len() * size_of::<NavNode>()
            + self.index.len() * (size_of::<NavNode>() + size_of::<NavNodeId>())
            + self.offsets.len() * size_of::<u32>()
            + self.edges.len() * size_of::<NavEdge>()
            + self.issues.len() * size_of::<GraphIssue>()
    }
}
