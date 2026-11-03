//! Opt-in tests: build the road, lane and prefab model of a whole local ETS2
//! or ATS map.
//!
//! Ignored by default and never run in CI. Reads the archives read-only,
//! keeps everything in memory, checks properties only and prints counts
//! only.
//!
//! ```powershell
//! $env:ROADPILOT_MAP_GAME_DIR = "C:\Program Files (x86)\Steam\steamapps\common\American Truck Simulator"
//! cargo test --release -p roadpilot-map --test real_installation -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use roadpilot_map::formats::sii::{SiiUnit, parse_sii};
use roadpilot_map::formats::{ItemData, parse_base};
use roadpilot_map::model::{
    ArmIssue, ArmLink, LaneConfig, LaneDirection, LaneTransition, LaneType, MovementStatus,
    NodeLink, OffsetsState, PrefabResolution, RoadId, RoadSide, RuleRef, TrafficSideState,
};
use roadpilot_map::{
    LaneDefinitions, PrefabDefinitions, build_lane_network, build_prefab_network,
    build_road_network,
};
use roadpilot_map_archive::Archive;

fn game_dir() -> PathBuf {
    let dir = std::env::var_os("ROADPILOT_MAP_GAME_DIR")
        .map(PathBuf::from)
        .expect(
            "set ROADPILOT_MAP_GAME_DIR to an ETS2 or ATS installation folder to run this test",
        );
    assert!(
        dir.join("base_map.scs").is_file(),
        "ROADPILOT_MAP_GAME_DIR ({}) does not contain base_map.scs",
        dir.display()
    );
    dir
}

/// The `.base` files of every map sector, read into memory.
fn sector_files(dir: &PathBuf) -> Vec<Vec<u8>> {
    let mut archives: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("installation folder is readable")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("scs")))
        .collect();
    archives.sort();
    let mut files = Vec::new();
    for path in &archives {
        let Ok(archive) = Archive::open(path) else {
            continue;
        };
        for map in ["map/europe", "map/usa"] {
            let Ok(children) = archive.list_dir(map) else {
                continue;
            };
            for child in children.iter().filter(|c| c.name.ends_with(".base")) {
                files.push(
                    archive
                        .read_file(&format!("{map}/{}", child.name))
                        .expect("sector file is readable"),
                );
            }
        }
    }
    files
}

#[test]
#[ignore = "reads a local game installation; set ROADPILOT_MAP_GAME_DIR and run with --ignored"]
fn builds_the_road_network_of_the_installation() {
    let files = sector_files(&game_dir());
    let sectors: Vec<_> = files
        .iter()
        .map(|f| parse_base(f).expect("sector parses"))
        .collect();
    let road_records: Vec<_> = sectors
        .iter()
        .flat_map(|s| &s.items)
        .filter_map(|i| match &i.data {
            ItemData::Road(r) => Some((i.uid, *r)),
            _ => None,
        })
        .collect();

    let net = build_road_network(&sectors).expect("the whole map builds and validates");

    // Every road record is in the model, with its references unchanged.
    assert!(!road_records.is_empty());
    assert_eq!(net.roads().count(), road_records.len());
    for (uid, record) in &road_records {
        let road = net
            .road(roadpilot_map::model::RoadId(*uid))
            .expect("road is in the model");
        assert_eq!(road.start_node.0, record.start_node);
        assert_eq!(road.end_node.0, record.end_node);
        assert_eq!(road.stored_length.to_bits(), record.length.to_bits());
    }

    // Connections are consistent in both directions.
    let mut connections = 0usize;
    for c in net.connections() {
        assert_eq!(net.predecessor(c.to), Some(c));
        let roads = net.roads_at(c.via).expect("node exists");
        assert_eq!((roads.ending, roads.starting), (Some(c.from), Some(c.to)));
        connections += 1;
    }
    let shared = net
        .nodes()
        .filter(|n| {
            matches!(
                (n.backward, n.forward),
                (NodeLink::Road(_), NodeLink::Road(_))
            )
        })
        .count();
    assert_eq!(connections, shared);

    // Geometry values are finite; the stored length is positive.
    for road in net.roads() {
        let g = net.geometry(road.id).expect("geometry");
        assert!(g.chord_length.is_finite() && g.stored_length > 0.0);
        let d = g.start_direction;
        assert!(((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]) - 1.0).abs() < 1e-6);
    }

    let unresolved = net
        .nodes()
        .flat_map(|n| [n.backward, n.forward])
        .filter(|l| matches!(l, NodeLink::Unresolved(_)))
        .count();
    eprintln!(
        "{} sectors, {} roads, {} nodes, {connections} road-to-road connections, {unresolved} unresolved node links",
        sectors.len(),
        road_records.len(),
        net.nodes().count()
    );
}

/// The definition units the lane model needs: every `road_look*` and
/// `traffic_lane*` file in `def/world` and `def/country.sii` of every
/// archive, with `@include` resolved relative to the including file.
fn lane_definition_units(dir: &PathBuf) -> Vec<SiiUnit> {
    definition_units(dir, &["road_look", "traffic_lane"], true)
}

/// The `.sii` files in `def/world` of every archive whose name starts with
/// one of `prefixes`, plus `def/country.sii` if `country` is set.
fn definition_units(dir: &PathBuf, prefixes: &[&str], country: bool) -> Vec<SiiUnit> {
    let mut archives: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("installation folder is readable")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("scs")))
        .collect();
    archives.sort();
    let mut units = Vec::new();
    for path in &archives {
        let Ok(archive) = Archive::open(path) else {
            continue;
        };
        let mut files = Vec::new();
        if let Ok(children) = archive.list_dir("def/world") {
            files.extend(
                children
                    .iter()
                    .filter(|c| !c.is_dir && c.name.ends_with(".sii"))
                    .filter(|c| prefixes.iter().any(|p| c.name.starts_with(p)))
                    .map(|c| format!("def/world/{}", c.name)),
            );
        }
        if country && archive.find("def/country.sii").ok().flatten().is_some() {
            files.push("def/country.sii".to_owned());
        }
        for file in files {
            let bytes = archive
                .read_file(&file)
                .expect("definition file is readable");
            let base = file.rsplit_once('/').map_or("", |(d, _)| d).to_owned();
            let mut include = |p: &str| {
                let full = p
                    .strip_prefix('/')
                    .map_or_else(|| format!("{base}/{p}"), str::to_owned);
                archive.read_file(&full).ok()
            };
            units.extend(parse_sii(&bytes, &mut include).expect("definition file parses"));
        }
    }
    units
}

#[test]
#[ignore = "reads a local game installation; set ROADPILOT_MAP_GAME_DIR and run with --ignored"]
fn builds_the_lane_network_of_the_installation() {
    let dir = game_dir();
    let files = sector_files(&dir);
    let sectors: Vec<_> = files
        .iter()
        .map(|f| parse_base(f).expect("sector parses"))
        .collect();
    let units = lane_definition_units(&dir);
    let defs = LaneDefinitions::from_units(&units).expect("definitions are consistent");
    let network = build_road_network(&sectors).expect("road network builds");
    let lanes = build_lane_network(&network, &sectors, &defs).expect("lane network builds");

    // Reproducible: a second build gives the same result.
    assert_eq!(
        build_lane_network(&network, &sectors, &defs).expect("lane network builds"),
        lanes
    );

    // The road look of each road, looked up independently of the builder.
    let looks: BTreeMap<&str, &SiiUnit> = units
        .iter()
        .filter(|u| u.class == "road_look")
        .map(|u| (u.name.as_str(), u))
        .collect();

    let mut stats: BTreeMap<String, usize> = BTreeMap::new();
    let mut count = |key: String| *stats.entry(key).or_default() += 1;
    for entry in lanes.roads() {
        let look = looks
            .get(entry.look.as_str())
            .expect("every road look resolves");
        match &entry.config {
            LaneConfig::Unresolved(issue) => count(format!("lanes unresolved: {issue:?}")),
            LaneConfig::Resolved(list) => {
                count("lanes resolved".to_owned());
                // Count and order match the look; indices are 0..n per side.
                for (side, key) in [
                    (RoadSide::Left, "lanes_left"),
                    (RoadSide::Right, "lanes_right"),
                ] {
                    let on_side: Vec<_> = list.iter().filter(|l| l.id.side == side).collect();
                    assert_eq!(on_side.len(), look.values(key).len());
                    for (i, lane) in on_side.iter().enumerate() {
                        assert_eq!(usize::from(lane.id.index), i);
                        let (LaneType::Known(name) | LaneType::Uncertain(name)) = &lane.lane_type;
                        assert_eq!(name, &look.values(key)[i]);
                        assert_eq!(lane.width, None, "no lane width is invented");
                    }
                }
                let left = list.iter().filter(|l| l.id.side == RoadSide::Left).count();
                let right = list.len() - left;
                count(format!("lane counts (left, right) = ({left}, {right})"));
                if list
                    .iter()
                    .any(|l| matches!(l.lane_type, LaneType::Uncertain(_)))
                {
                    count("lane types uncertain (variants)".to_owned());
                }
            }
        }
        match entry.traffic_side {
            TrafficSideState::Known(side) => count(format!("traffic side {side:?}")),
            TrafficSideState::Unknown(issue) => count(format!("traffic side unknown: {issue:?}")),
        }
        for state in [entry.offsets_left, entry.offsets_right] {
            if let OffsetsState::CountMismatch { .. } = state {
                count("offset count differs from lane count".to_owned());
            }
        }
    }

    // Candidates never connect other roads or opposite driving directions.
    let road_of = |id: RoadId| lanes.road(id).expect("road has a lane entry");
    for connection in network.connections() {
        match lanes.transition(&connection) {
            LaneTransition::IndexCandidates(pairs) => {
                count("road connections with lane candidates".to_owned());
                for pair in pairs {
                    let d_from = road_of(pair.from.road)
                        .direction(pair.from.side)
                        .expect("direction is known");
                    let d_to = road_of(pair.to.road)
                        .direction(pair.to.side)
                        .expect("direction is known");
                    assert_eq!(d_from, d_to, "a candidate keeps the driving direction");
                    let expected = if d_from == LaneDirection::Forward {
                        (connection.from, connection.to)
                    } else {
                        (connection.to, connection.from)
                    };
                    assert_eq!((pair.from.road, pair.to.road), expected);
                }
            }
            LaneTransition::Unknown(issue) => {
                count(format!("road connections without lane pairing: {issue:?}"));
            }
        }
    }

    let (looks_n, variants_n, lane_types_n, countries_n) = defs.counts();
    eprintln!(
        "definitions: {looks_n} road looks, {variants_n} variants, {lane_types_n} lane types, {countries_n} countries"
    );
    for (key, value) in &stats {
        eprintln!("{value:>8}  {key}");
    }
    assert!(
        !stats
            .keys()
            .any(|k| k.starts_with("lanes unresolved: Missing")),
        "every look, variant and lane type resolves"
    );
}

/// Every archive of the installation, in file name order.
fn open_archives(dir: &PathBuf) -> Vec<Archive> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("installation folder is readable")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("scs")))
        .collect();
    paths.sort();
    paths.iter().filter_map(|p| Archive::open(p).ok()).collect()
}

fn resolution_key(r: &PrefabResolution) -> &'static str {
    match r {
        PrefabResolution::Resolved => "resolved",
        PrefabResolution::MissingModel => "model missing",
        PrefabResolution::MissingDescriptor(_) => "descriptor missing",
        PrefabResolution::InvalidDescriptor(_) => "descriptor invalid",
        PrefabResolution::NodeCountMismatch { .. } => "node count differs",
        PrefabResolution::OriginOutOfRange(_) => "origin out of range",
        PrefabResolution::NodeGeometryMismatch => "node geometry differs",
    }
}

fn issue_key(issue: ArmIssue) -> &'static str {
    match issue {
        ArmIssue::NoRoad => "no road attached",
        ArmIssue::PrefabUnresolved => "prefab unresolved",
        ArmIssue::RoadLanesUnknown => "road lanes or traffic side unknown",
        ArmIssue::LaneCountMismatch { .. } => "lane count differs from slot count",
    }
}

#[test]
#[ignore = "reads a local game installation; set ROADPILOT_MAP_GAME_DIR and run with --ignored"]
fn builds_the_prefab_network_of_the_installation() {
    let dir = game_dir();
    let files = sector_files(&dir);
    let sectors: Vec<_> = files
        .iter()
        .map(|f| parse_base(f).expect("sector parses"))
        .collect();
    let network = build_road_network(&sectors).expect("road network builds");
    let lane_defs =
        LaneDefinitions::from_units(&lane_definition_units(&dir)).expect("lane definitions");
    let lanes = build_lane_network(&network, &sectors, &lane_defs).expect("lane network builds");
    let units = definition_units(&dir, &["prefab", "traffic_rules"], false);
    let defs = PrefabDefinitions::from_units(&units).expect("prefab definitions are consistent");
    // A descriptor is read from the last archive (in name order) that has it.
    let archives = open_archives(&dir);
    let load = |path: &str| archives.iter().rev().find_map(|a| a.read_file(path).ok());
    let prefabs = build_prefab_network(&network, &sectors, &defs, load).expect("prefabs build");

    // Reproducible: a second build gives the same result.
    assert_eq!(
        build_prefab_network(&network, &sectors, &defs, load).expect("prefabs build"),
        prefabs
    );

    let mut stats: BTreeMap<String, usize> = BTreeMap::new();
    let mut add = |key: String, n: usize| *stats.entry(key).or_default() += n;
    let prefab_items = sectors
        .iter()
        .flat_map(|s| &s.items)
        .filter(|i| matches!(i.data, ItemData::Prefab(_)))
        .count();
    assert!(prefab_items > 0);
    assert_eq!(prefabs.instances().count(), prefab_items);

    for inst in prefabs.instances() {
        add(
            format!("instances: {}", resolution_key(&inst.resolution)),
            1,
        );
        let resolved = inst.resolution == PrefabResolution::Resolved;
        let def = prefabs.definition(&inst.model).filter(|_| resolved);
        assert_eq!(resolved, def.is_some());

        let arms = prefabs.arms(&network, inst.id);
        assert_eq!(arms.len(), inst.nodes.len());
        for arm in &arms {
            // The node mapping is a bijection between instance and definition.
            assert_eq!(arm.definition_node.is_some(), resolved);
            if let Some(d) = arm.definition_node {
                assert_eq!(inst.instance_node(d), Some(arm.index));
            }
            let key = match arm.link {
                ArmLink::Road {
                    road,
                    road_ends_here,
                } => {
                    let r = network.road(road).expect("road exists");
                    let node = if road_ends_here {
                        r.end_node
                    } else {
                        r.start_node
                    };
                    assert_eq!(node, arm.node, "the road ends or starts at the prefab node");
                    "road"
                }
                ArmLink::Prefab(other) => {
                    assert_ne!(other, inst.id);
                    "prefab"
                }
                ArmLink::Item(_) => "other item",
                ArmLink::Unresolved(_) => "unresolved UID",
                ArmLink::None => "nothing",
            };
            add(format!("prefab nodes attached to: {key}"), 1);
        }

        let movements = prefabs.movements(inst.id);
        let Some(def) = def else {
            assert!(movements.is_empty(), "no movements for unresolved prefabs");
            continue;
        };
        add(
            "semaphores (static), summed over resolved instances".to_owned(),
            def.semaphores.len(),
        );
        if !def.semaphores.is_empty() {
            add("resolved instances with static semaphores".to_owned(), 1);
        }
        let road_arms = arms
            .iter()
            .filter(|a| matches!(a.link, ArmLink::Road { .. }))
            .count();
        add(
            "road arm sides of resolved prefabs (input and output)".to_owned(),
            2 * road_arms,
        );
        for m in &movements {
            let from = &def.nodes[inst.definition_node(m.from_arm).expect("mapped")];
            let to = &def.nodes[inst.definition_node(m.to_arm).expect("mapped")];
            assert!(from.input_slots[m.from_slot].is_some());
            assert!(to.output_slots[m.to_slot].is_some());
            if !m.traffic_rules.is_empty() {
                add("internal movements with a traffic rule".to_owned(), 1);
            }
            for rule in &m.traffic_rules {
                assert!(
                    matches!(rule, RuleRef::Resolved(_)),
                    "every traffic rule resolves"
                );
            }
        }
        add("internal movements (confirmed)".to_owned(), movements.len());

        let lane_moves = prefabs.lane_movements(&network, &lanes, inst.id);
        let road_at = |i: usize| match arms[i].link {
            ArmLink::Road { road, .. } => Some(road),
            _ => None,
        };
        for lm in &lane_moves.movements {
            assert_eq!(lm.status, MovementStatus::Candidate);
            assert_eq!(Some(lm.from.road), road_at(lm.internal.from_arm));
            assert_eq!(Some(lm.to.road), road_at(lm.internal.to_arm));
            assert!(movements.contains(&lm.internal));
        }
        add(
            "lane movements road to road (candidate)".to_owned(),
            lane_moves.movements.len(),
        );
        for (_, issue) in &lane_moves.unpaired_arms {
            add(format!("unpaired arm sides: {}", issue_key(*issue)), 1);
        }
    }

    let (models, rules) = defs.counts();
    eprintln!(
        "definitions: {models} prefab models, {rules} traffic rules, {} prefab definitions used",
        prefabs.definition_count()
    );
    for (key, value) in &stats {
        eprintln!("{value:>8}  {key}");
    }
    assert!(
        !stats
            .keys()
            .any(|k| k.contains("descriptor invalid") || k.contains("model missing")),
        "every prefab model and descriptor resolves"
    );
}

fn variant_name(debug: &str) -> String {
    debug
        .split(['(', '{', ' '])
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[test]
#[ignore = "reads a local game installation; set ROADPILOT_MAP_GAME_DIR and run with --ignored"]
fn builds_the_navigation_graph_of_the_installation() {
    use roadpilot_map::model::{
        ConnectionStatus, NavNode, NavigationGraph, Route, RouteConfirmation, RouteError,
        RoutingMode,
    };
    use std::time::Instant;

    let dir = game_dir();
    let files = sector_files(&dir);
    let sectors: Vec<_> = files
        .iter()
        .map(|f| parse_base(f).expect("sector parses"))
        .collect();
    let network = build_road_network(&sectors).expect("road network builds");
    let lane_defs =
        LaneDefinitions::from_units(&lane_definition_units(&dir)).expect("lane definitions");
    let lanes = build_lane_network(&network, &sectors, &lane_defs).expect("lane network builds");
    let units = definition_units(&dir, &["prefab", "traffic_rules"], false);
    let defs = PrefabDefinitions::from_units(&units).expect("prefab definitions");
    let archives = open_archives(&dir);
    let load = |path: &str| archives.iter().rev().find_map(|a| a.read_file(path).ok());
    let prefabs = build_prefab_network(&network, &sectors, &defs, load).expect("prefabs build");

    let t = Instant::now();
    let graph = NavigationGraph::build(&network, &lanes, &prefabs).expect("graph builds");
    let build_time = t.elapsed();
    assert_eq!(
        NavigationGraph::build(&network, &lanes, &prefabs).expect("graph builds"),
        graph,
        "the graph is reproducible"
    );

    let [confirmed, candidate, unknown, forbidden] = graph.status_counts();
    eprintln!(
        "navigation graph: {} nodes, {} edges ({confirmed} confirmed, {candidate} candidate, {unknown} unknown, {forbidden} forbidden), {} merged duplicates",
        graph.node_count(),
        graph.edge_count(),
        graph.merged_duplicates()
    );
    eprintln!(
        "build {build_time:?}, about {} MiB heap",
        graph.approx_heap_bytes() >> 20
    );
    assert_eq!(forbidden, 0, "no source of forbidden connections is known");
    assert!(confirmed > 0 && candidate > 0 && unknown > 0);

    let mut by_origin: BTreeMap<String, usize> = BTreeMap::new();
    for e in graph.edges() {
        let key = format!("{:?} {:?}", e.origin().kind, e.status());
        *by_origin.entry(key).or_default() += 1;
    }
    for (k, v) in &by_origin {
        eprintln!("{v:>9}  edges: {k}");
    }
    let mut issues: BTreeMap<String, usize> = BTreeMap::new();
    for i in graph.issues() {
        *issues
            .entry(variant_name(&format!("{:?}", i.kind)))
            .or_default() += 1;
    }
    for (k, v) in &issues {
        eprintln!("{v:>9}  issues: {k}");
    }
    // Only stored internal prefab movements are confirmed.
    for e in graph.edges() {
        assert_eq!(
            e.status() == ConnectionStatus::Confirmed,
            e.origin().kind == roadpilot_map::model::EdgeOriginKind::PrefabInternalCurve
        );
    }

    let check = |route: &Route| {
        let mut at = route.start;
        for s in &route.steps {
            assert_eq!(s.from, at, "steps are connected");
            at = s.to;
            match route.mode {
                RoutingMode::ConfirmedOnly => assert_eq!(s.status, ConnectionStatus::Confirmed),
                RoutingMode::AllowCandidates => assert!(matches!(
                    s.status,
                    ConnectionStatus::Confirmed | ConnectionStatus::Candidate
                )),
            }
        }
        assert_eq!(at, route.goal);
        let candidates = route
            .steps
            .iter()
            .filter(|s| s.status == ConnectionStatus::Candidate)
            .count();
        assert_eq!(candidates, route.cost.candidate_edges as usize);
        assert_eq!(
            matches!(route.confirmation(), RouteConfirmation::Provisional { .. }),
            candidates > 0
        );
    };

    // Confirmed-only: a stored prefab movement is a route of its own.
    let sample = graph
        .edges()
        .find(|e| e.status() == ConnectionStatus::Confirmed)
        .expect("a confirmed edge");
    let (a, b) = (
        graph.node(sample.from()).expect("node"),
        graph.node(sample.to()).expect("node"),
    );
    let route = graph
        .route(a, b, RoutingMode::ConfirmedOnly)
        .expect("confirmed route");
    check(&route);
    assert_eq!(
        route.confirmation(),
        RouteConfirmation::AllConnectionsConfirmed
    );

    // Starts spread over a 3 x 3 grid of the map: the first lane of each cell.
    let positions: Vec<_> = network.nodes().map(|n| n.position.world()).collect();
    let (mut min, mut max) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in &positions {
        for (k, axis) in [0usize, 2].into_iter().enumerate() {
            min[k] = min[k].min(p[axis]);
            max[k] = max[k].max(p[axis]);
        }
    }
    let cell = |p: [f64; 3]| {
        let c = |k: usize, axis: usize| {
            (((p[axis] - min[k]) / (max[k] - min[k] + 1.0) * 3.0) as usize).min(2)
        };
        (c(0, 0), c(1, 2))
    };
    let mut starts: BTreeMap<(usize, usize), NavNode> = BTreeMap::new();
    for node in graph.nodes() {
        let NavNode::LaneEntry(lane) = node else {
            continue;
        };
        let road = network.road(lane.road).expect("road");
        let p = network
            .node(road.start_node)
            .expect("node")
            .position
            .world();
        starts.entry(cell(p)).or_insert(node);
    }
    let t = Instant::now();
    let mut routes = 0;
    for (cell, start) in &starts {
        let reach = graph
            .reachable_from(*start, RoutingMode::AllowCandidates)
            .expect("start is in the graph");
        let confirmed_reach = graph
            .reachable_from(*start, RoutingMode::ConfirmedOnly)
            .expect("start is in the graph");
        let goal = reach
            .iter()
            .rev()
            .find(|n| matches!(n, NavNode::LaneExit(_)))
            .copied()
            .expect("a lane exit is reachable");
        let route = graph
            .route(*start, goal, RoutingMode::AllowCandidates)
            .expect("provisional route");
        check(&route);
        assert_eq!(
            graph.route(*start, goal, RoutingMode::AllowCandidates),
            Ok(route.clone()),
            "routes are reproducible"
        );
        // Lane directions and candidates keep a lane-to-lane route confirmed-free.
        if route.cost.candidate_edges > 0 && *start != goal {
            assert_eq!(
                graph.route(*start, goal, RoutingMode::ConfirmedOnly),
                Err(RouteError::NoConfirmedRoute)
            );
        }
        routes += 1;
        eprintln!(
            "cell {cell:?}: reachable {} (confirmed only {}), route {} edges, {} candidates",
            reach.len(),
            confirmed_reach.len(),
            route.cost.edges,
            route.cost.candidate_edges
        );
    }
    eprintln!("{routes} routes in {:?}", t.elapsed());
    assert!(routes >= 4, "starts from several map regions");
}
