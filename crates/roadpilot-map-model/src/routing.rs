//! Route search on a [`NavigationGraph`].
//!
//! The cost of a route is **algorithmic**, not physical: the number of
//! candidate edges first, then the number of edges. No road length is used
//! (its unit is not confirmed), and no distance, time or speed is computed.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fmt;

use crate::navigation::{
    ConnectionStatus, EdgeId, EdgeOrigin, NavNode, NavNodeId, NavigationGraph,
};

/// Which connections a route may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoutingMode {
    /// Only confirmed connections.
    ConfirmedOnly,
    /// Confirmed and candidate connections. Unknown and forbidden
    /// connections stay excluded.
    AllowCandidates,
}

impl RoutingMode {
    fn allows(self, status: ConnectionStatus) -> bool {
        match self {
            Self::ConfirmedOnly => status == ConnectionStatus::Confirmed,
            Self::AllowCandidates => matches!(
                status,
                ConnectionStatus::Confirmed | ConnectionStatus::Candidate
            ),
        }
    }
}

/// The cost of a route. Dimensionless routing cost, **not a distance**.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RouteCost {
    /// Number of candidate edges. Compared first.
    pub candidate_edges: u32,
    /// Number of edges. Compared second.
    pub edges: u32,
}

/// How far a route is confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteConfirmation {
    /// Every edge of the route is confirmed. This does not mean that the
    /// route is drivable: signals, traffic rules and driving rules are not
    /// evaluated.
    AllConnectionsConfirmed,
    /// The route uses candidate connections and is provisional.
    Provisional { candidate_edges: u32 },
}

/// One edge of a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteStep {
    pub edge: EdgeId,
    pub from: NavNode,
    pub to: NavNode,
    pub status: ConnectionStatus,
    pub origin: EdgeOrigin,
}

/// A route through the navigation graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub mode: RoutingMode,
    pub start: NavNode,
    pub goal: NavNode,
    pub steps: Vec<RouteStep>,
    pub cost: RouteCost,
}

impl Route {
    /// Whether the route is fully confirmed or provisional.
    #[must_use]
    pub fn confirmation(&self) -> RouteConfirmation {
        if self.cost.candidate_edges == 0 {
            RouteConfirmation::AllConnectionsConfirmed
        } else {
            RouteConfirmation::Provisional {
                candidate_edges: self.cost.candidate_edges,
            }
        }
    }

    /// Positions (in `steps`) of the candidate edges.
    #[must_use]
    pub fn candidate_positions(&self) -> Vec<usize> {
        self.steps
            .iter()
            .enumerate()
            .filter(|(_, s)| s.status == ConnectionStatus::Candidate)
            .map(|(i, _)| i)
            .collect()
    }
}

/// Why no route was returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RouteError {
    /// The start is not a node of the graph.
    UnknownStart(NavNode),
    /// The goal is not a node of the graph.
    UnknownGoal(NavNode),
    /// No route exists that uses confirmed connections only. A route with
    /// candidate connections may exist.
    NoConfirmedRoute,
    /// No route exists even when candidate connections are allowed.
    NoRoute,
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownStart(n) => write!(f, "start {n:?} is not in the navigation graph"),
            Self::UnknownGoal(n) => write!(f, "goal {n:?} is not in the navigation graph"),
            Self::NoConfirmedRoute => write!(f, "no fully confirmed route was found"),
            Self::NoRoute => write!(f, "no route was found"),
        }
    }
}

impl std::error::Error for RouteError {}

const NONE: u32 = u32::MAX;
const CANDIDATE_UNIT: u64 = 1 << 32;

/// Result of one search: cost per node, the edge that reached it, and the
/// nodes in the order they were settled.
struct Search {
    cost: Vec<u64>,
    via: Vec<u32>,
    settled: Vec<NavNodeId>,
}

fn unpack(cost: u64) -> RouteCost {
    RouteCost {
        candidate_edges: u32::try_from(cost >> 32).unwrap_or(u32::MAX),
        edges: u32::try_from(cost & 0xFFFF_FFFF).unwrap_or(u32::MAX),
    }
}

impl NavigationGraph {
    /// Dijkstra with an explicit heap: no recursion, memory bounded by the
    /// number of nodes. With `goal` set, the search stops when it is settled.
    fn search(&self, start: NavNodeId, goal: Option<NavNodeId>, mode: RoutingMode) -> Search {
        let n = self.node_count();
        let mut s = Search {
            cost: vec![u64::MAX; n],
            via: vec![NONE; n],
            settled: Vec::new(),
        };
        let mut heap = BinaryHeap::new();
        s.cost[start.index()] = 0;
        heap.push(Reverse((0u64, start.0)));
        while let Some(Reverse((cost, node))) = heap.pop() {
            let id = NavNodeId(node);
            if cost > s.cost[id.index()] {
                continue;
            }
            s.settled.push(id);
            if Some(id) == goal {
                break;
            }
            for edge_id in self.out_edge_ids(id) {
                let Some(edge) = self.edge(edge_id) else {
                    continue;
                };
                if !mode.allows(edge.status()) {
                    continue;
                }
                let step = if edge.status() == ConnectionStatus::Candidate {
                    CANDIDATE_UNIT + 1
                } else {
                    1
                };
                let Some(next) = cost.checked_add(step) else {
                    continue;
                };
                let to = edge.to();
                if next < s.cost[to.index()] {
                    s.cost[to.index()] = next;
                    s.via[to.index()] = edge_id.0;
                    heap.push(Reverse((next, to.0)));
                }
            }
        }
        s
    }

    /// The cheapest route from `start` to `goal` in the given mode.
    ///
    /// Start and goal are nodes of the graph, never positions: a lane entry,
    /// a lane exit or a prefab slot. Nothing is matched by proximity. The
    /// cheapest route has the fewest candidate edges, and among those the
    /// fewest edges. Ties are resolved by graph order, so the result is
    /// reproducible.
    ///
    /// # Errors
    ///
    /// [`RouteError::UnknownStart`] and [`RouteError::UnknownGoal`] for
    /// nodes that are not in the graph, [`RouteError::NoConfirmedRoute`] or
    /// [`RouteError::NoRoute`] if the goal is not reachable.
    pub fn route(
        &self,
        start: NavNode,
        goal: NavNode,
        mode: RoutingMode,
    ) -> Result<Route, RouteError> {
        let s = self.node_id(start).ok_or(RouteError::UnknownStart(start))?;
        let g = self.node_id(goal).ok_or(RouteError::UnknownGoal(goal))?;
        let search = self.search(s, Some(g), mode);
        if search.cost[g.index()] == u64::MAX {
            return Err(match mode {
                RoutingMode::ConfirmedOnly => RouteError::NoConfirmedRoute,
                RoutingMode::AllowCandidates => RouteError::NoRoute,
            });
        }
        let mut steps = Vec::new();
        let mut at = g;
        // Every node is visited at most once, so the loop is bounded.
        for _ in 0..self.node_count() {
            let via = search.via[at.index()];
            if via == NONE {
                break;
            }
            let id = EdgeId(via);
            let Some(edge) = self.edge(id) else { break };
            let (Some(from), Some(to)) = (self.node(edge.from()), self.node(edge.to())) else {
                break;
            };
            steps.push(RouteStep {
                edge: id,
                from,
                to,
                status: edge.status(),
                origin: edge.origin(),
            });
            at = edge.from();
        }
        steps.reverse();
        Ok(Route {
            mode,
            start,
            goal,
            steps,
            cost: unpack(search.cost[g.index()]),
        })
    }

    /// All nodes reachable from `start`, in the order of increasing routing
    /// cost (the start itself first).
    ///
    /// # Errors
    ///
    /// [`RouteError::UnknownStart`] if `start` is not in the graph.
    pub fn reachable_from(
        &self,
        start: NavNode,
        mode: RoutingMode,
    ) -> Result<Vec<NavNode>, RouteError> {
        let s = self.node_id(start).ok_or(RouteError::UnknownStart(start))?;
        Ok(self
            .search(s, None, mode)
            .settled
            .into_iter()
            .filter_map(|id| self.node(id))
            .collect())
    }
}
