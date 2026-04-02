# Vision

## Goal

Build a central search architecture where team-wide knowledge is stored once in
PostgreSQL and served consistently to all developers, while local developer work
still remains fast and immediately visible through overlay merge.

## Product Direction

The target user experience is:

- platform reference is indexed once and reused by everyone
- shared code baselines such as `vendor` and `develop` are published centrally
- feature branch developers do not need to publish their transient branch state
  to get relevant search results
- uncommitted local changes affect search immediately
- search status is explainable and reflects baseline selection, branch support,
  overlay state, and semantic availability

## What Success Looks Like

A developer should be able to:

1. open a project on a feature branch
2. start MCP without waiting for a full local baseline rebuild
3. search shared code from the correct baseline
4. see local branch changes override baseline results immediately
5. use one shared platform reference without per-project duplication

A team should be able to:

1. publish `vendor`, `develop`, and `reference` centrally from CI
2. control retention and visibility at snapshot level
3. inspect baseline health and runtime behavior clearly
4. avoid repeated reindexing of identical corpora on every machine

## Main Architectural Bet

The key architectural bet is:

- centralized published content belongs in PostgreSQL
- transient local development state belongs in a local overlay
- the correct runtime abstraction is a merged logical search view, not one
  physical index copied everywhere

## Why This Direction Matters

Without this shift:

- platform reference remains duplicated per project and per machine
- startup cost continues to be dominated by local baseline hydration
- centralized publication gives only partial value
- corporate shared knowledge remains operationally fragmented

With this shift:

- shared search becomes an actual team service
- local development remains fast and precise
- runtime behavior aligns with real branch workflows such as
  `vendor -> develop -> feature/*`

## Non-Goals

This architecture does not try to:

- publish every developer branch state on every MCP startup
- eliminate local runtime state completely
- collapse baseline and overlay into one physical storage backend at any cost
- optimize ranking quality before correctness and explainability are established

## Guiding Principles

1. Shared data should be stored once.
2. Local changes should be visible immediately.
3. Runtime behavior should be explainable through status and diagnostics.
4. Branch workflow should be modeled explicitly, not implied accidentally by
   storage shape.
5. Architecture should optimize for long-term operational sanity, not only for
   short-term implementation convenience.
