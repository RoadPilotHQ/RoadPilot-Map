# RoadPilot-Map

RoadPilot-Map is the planned map processing and geographic data infrastructure of the RoadPilot ecosystem.

> **Status: early planning.** This repository does not contain source code yet. Responsibilities, architecture and roadmap below describe the **planned** scope, not existing functionality.

Architecture documentation, decision records and open questions are in [`docs/`](docs/README.md).

## Purpose

This repository is meant to turn ETS2 and ATS map data into structured, versioned road information (geometry, lanes, curvature and road networks) that other RoadPilot components can use. It is kept separate so that map processing can evolve and be released independently of the desktop application.

## Key responsibilities

- ETS2 and ATS map data
- SCS map extraction and parsing
- Road and lane geometry
- Lane curvature
- Road networks and intersections
- Map data versioning
- Map processing interfaces

## Architecture overview

The following structure is a proposal and may change as development starts:

- **Extraction**: reading map data from SCS game files.
- **Parsing**: converting extracted data into an internal map model.
- **Geometry**: road and lane geometry, including lane curvature.
- **Network**: road networks and intersections as a connected graph.
- **Versioning**: tracking map data across game and DLC versions.
- **Interfaces**: a documented format or API through which other components consume map data.

## Integration with the RoadPilot ecosystem

RoadPilot is split into independent repositories. Each one is developed, versioned and released on its own; there is no monorepo and no shared source code.

| Repository | Role |
|---|---|
| [RoadPilot](https://github.com/RoadPilotHQ/RoadPilot) | Main desktop application |
| **RoadPilot-Map** (this repository) | Map processing and geographic data |
| [RoadPilot-Language](https://github.com/RoadPilotHQ/RoadPilot-Language) | Translations and localization |
| [RoadPilot-Visualization](https://github.com/RoadPilotHQ/RoadPilot-Visualization) | Visualization components and development tools |
| [RoadPilot-Plugin](https://github.com/RoadPilotHQ/RoadPilot-Plugin) | Plugin architecture and SDK |
| [RoadPilot-Website](https://github.com/RoadPilotHQ/RoadPilot-Website) | Official website |
| [RoadPilot-Discord](https://github.com/RoadPilotHQ/RoadPilot-Discord) | Discord community infrastructure |
| [RoadPilot-Docs](https://github.com/RoadPilotHQ/RoadPilot-Docs) | Central documentation |

RoadPilot-Map is intended to provide map data to the RoadPilot application and to RoadPilot-Visualization through a documented interface. The interface has not been designed yet.

## Development status

| Area | Status |
|---|---|
| Source code | Not started |
| Architecture | In planning |
| Releases | None |

## Planned roadmap

Planned work, in no fixed order and without dates:

- [ ] Document the structure of the relevant SCS map data
- [ ] Design the internal map model
- [ ] Define the map data format and versioning scheme
- [ ] Implement extraction and parsing
- [ ] Compute road and lane geometry and curvature
- [ ] Build the road network and intersection model

## Contributing

RoadPilot is at an early stage and the contribution process is still being defined. Shared guidelines are planned in [RoadPilot-Docs](https://github.com/RoadPilotHQ/RoadPilot-Docs).

Until then, please open an issue or start a discussion before starting any work, so it can be coordinated. All contributions, issues and documentation must be written in English.

## License

Licensing is currently undecided. No license has been selected for this repository yet. Until a license is published here, no permission is granted to use, copy, modify or distribute its contents beyond what the GitHub Terms of Service allow.

---

Euro Truck Simulator 2 and American Truck Simulator are trademarks of SCS Software. RoadPilot is not an official SCS Software product.
