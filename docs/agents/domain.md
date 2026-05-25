# Domain Docs

This is a single-context repository.

## Before exploring, read these

- **`CONTEXT.md`** at the repository root.
- **`docs/adr/`** for ADRs relevant to the area being changed.
- **`ARCHITECTURE.md`** when work touches runtime structure, Discord integration, persistence, backup, packaging, or deployment.

If these files do not exist, proceed silently. The producer skill `/grill-with-docs` creates them when domain terminology or architectural decisions are resolved.

## Relationship to Architecture

`CONTEXT.md` is the domain glossary and must remain free of implementation decisions. `ARCHITECTURE.md` is supporting design documentation; it is currently an initial architecture proposal, with its explicitly confirmed owner decisions available to derive accepted ADRs. Treat accepted ADRs as settled decisions unless a change deliberately revisits them.

## File structure

```text
/
|-- CONTEXT.md
|-- docs/adr/
`-- src/
```

## Use the glossary's vocabulary

When output names a domain concept, use the term defined in `CONTEXT.md`. If a needed concept is absent, note it for `/grill-with-docs`.

## Flag ADR conflicts

If proposed work contradicts an existing ADR, surface the conflict explicitly rather than silently overriding it.
