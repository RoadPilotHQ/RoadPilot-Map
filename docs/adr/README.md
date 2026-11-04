# Architecture decision records

An architecture decision record (ADR) documents one significant architectural decision: its context, the decision itself, and its consequences. ADRs make it possible to understand later why the project is built the way it is.

## When to write an ADR

Write an ADR when a decision:

- is hard or expensive to reverse,
- affects the public interface, the map package format or how other components integrate,
- affects legal or distribution constraints, or
- chooses between several reasonable alternatives.

Small, local implementation choices do not need an ADR.

## Process

1. Copy [template.md](template.md) to `NNNN-short-title-in-kebab-case.md`, using the next free number.
2. Fill in all sections. Set the status to `Proposed`.
3. Discuss and review the ADR together with, or before, the change it describes.
4. When accepted, set the status to `Accepted` and add it to the index below.

ADRs are not rewritten after acceptance. To change a decision, write a new ADR that supersedes the old one, and set the old one's status to `Superseded by ADR NNNN`.

## Status values

| Status | Meaning |
|---|---|
| Proposed | Under discussion, not yet binding. |
| Accepted | Binding for the project. |
| Deprecated | No longer relevant, but not replaced. |
| Superseded by ADR NNNN | Replaced by a later ADR. |

## Index

| ADR | Title | Status |
|---|---|---|
| [0001](0001-rust-library-and-cli-with-local-offline-generation.md) | Rust library and CLI with local offline generation | Accepted |
| [0002](0002-map-package-format-and-versioning.md) | Map package format and versioning | Accepted |

## Planned ADRs

These decisions are known to be needed and will each receive their own ADR:

- Coordinate system and units (the package keeps native coordinates and states the unit as unconfirmed; the decision about a transformed system is open)
- A binary package encoding, if the consuming application needs one (see [ADR 0002](0002-map-package-format-and-versioning.md))
- Spline and lane geometry representation
- Handling of DLC load order and overrides
- Final software license (after legal review)
