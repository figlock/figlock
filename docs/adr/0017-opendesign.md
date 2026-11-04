# ADR-0017: UI design tool: OpenDesign

- Status: Accepted
- Date: 2026-11-04
- Decision: D17 in `docs/decisions.md`

## Context

A consistent brand across web, desktop, extension and native apps is needed, and an agent-friendly design tool lets prototypes be made and iterated locally.

## Decision

Use OpenDesign (nexu-io/open-design, Apache-2.0) run locally. `design/DESIGN.md` and `design/tokens.css` are the brand contract it reads; prototypes are references, not shipped code.

## Consequences

- Product analytics are turned off and only sample data goes in; an always-on safety telemetry channel remains (`AGENTS.md`).
- HTML prototypes translate to web, desktop and extension; iOS and Android take the tokens only.
- Dependence on a third-party tool is limited to design work; nothing ships from it.

## Alternatives considered

- Figma: no direct agent loop and a cloud account; a good fallback if OpenDesign stops being maintained.
