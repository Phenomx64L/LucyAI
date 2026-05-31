# Kappa Graph (κ(G)) — research mirror for Lucy memory v1.6.0

Snapshot of [`aaronsb/knowledge-graph-system`](https://github.com/aaronsb/knowledge-graph-system)
captured at 2026-05-30 (commit `main`). 30 files / 20,264 lines.

This is **read-only reference material**. It does NOT execute as part of
Lucy's build. It exists so engineers planning v1.6.0 can grep the source
of truth without re-fetching from the internet.

## What this project is

Kappa Graph builds a **semantic knowledge graph with grounding scores**: it
extracts concepts from documents, computes how well-supported each claim is
via the ratio of supporting vs. contradicting evidence, preserves contested
evidence instead of deleting it, and chains every claim back to the original
source text.

Stack: PostgreSQL + Apache AGE (openCypher), pgvector, a Rust Postgres
extension for in-memory BFS, FastAPI ingest, React+D3 viz, FUSE filesystem
mount, MCP server.

It is NOT what Lucy should become — Lucy is a single-user desktop SQLite
app and that's correct. But several of Kappa's algorithmic ideas port
cleanly into Lucy's existing memory pipeline.

## Directory layout in this mirror

```
docs/research/kappa-graph/
├── README.md            ← this file
├── schema/              ← raw schema definitions
│   ├── init.cypher        178 lines · Neo4j-flavor constraints + vector/FTS indexes
│   ├── 00_baseline.sql    996 lines · the Apache AGE / PostgreSQL baseline
│   └── 11_graph_accel.sql  19 lines · graph_accel Rust-extension hookup
├── adrs/                ← 23 Architecture Decision Records
└── reference/           ← overview + meta docs
```

## Why each ADR was downloaded — Lucy integration map

Sorted by **value-to-Lucy / effort ratio**, highest first.

### Tier 1 — directly portable into v1.6.0 (1 release each)

| File | What it gives Lucy |
|---|---|
| `adrs/ADR-044-probabilistic-truth-convergence.md` | The **grounding-score formula** `support_w / (support_w + contradict_w)`. Computed query-time, never cached. Filter default 0.20. Drop-in for `agent_memories` + `memory_core`. |
| `adrs/ADR-058-polarity-axis-triangulation.md` | **Polarity scoring via embeddings** without LLM calls per edge. Define N anchor pairs (e.g. `supports`/`contradicts`), average their difference vectors, project edge embeddings onto the resulting axis. Continuous score `[-1, +1]`. Replaces the binary `event_kind` hard-coding in `chip_memory.rs`. |
| `adrs/ADR-068-source-text-embeddings.md` | What gets embedded and how. Cheap context for ADR-058 implementation. |
| `adrs/ADR-070-polarity-axis-analysis.md` | Diagnostics / analysis follow-up to ADR-058. Useful for the visualization side of Lucy's polarity scores. |

### Tier 2 — worth a sprint, slightly bigger lift

| File | What it gives Lucy |
|---|---|
| `adrs/ADR-200-annealing-ontologies.md` | **Self-organizing graph structure** — concepts cluster into ontologies via energy minimization. Reframes Lucy's tag system as first-class graph clusters. The 733 lines here are worth a deep read. |
| `adrs/ADR-048-vocabulary-metadata-as-graph.md` | Edge types as **first-class nodes** with their own properties. Lets you query "what relationships does Lucy use most" and consolidate synonyms. |
| `adrs/ADR-052-vocabulary-expansion-consolidation-cycle.md` | The **"dreaming" pattern**: expansion phase lets LLM propose edge types freely; consolidation phase merges semantically similar types via embedding cosine similarity. Cron-friendly. |
| `adrs/ADR-046-grounding-aware-vocabulary-management.md` | Apply ADR-044 grounding to the vocabulary itself (low-grounding edge types get auto-deprecated). |
| `adrs/ADR-047-probabilistic-vocabulary-categorization.md` | Classify new edge types by embedding similarity to category prototypes. Solves "where does this new edge belong in the taxonomy". |
| `adrs/ADR-063-semantic-diversity-authenticity.md` | **Semantic diversity as authenticity signal** — concepts cited from many independent semantic neighborhoods get higher trust. Resists echo-chamber memories. |
| `adrs/ADR-065-vocabulary-based-provenance-relationships.md` | Provenance edges become part of the vocabulary, so you can ask "give me only claims cited by an academic source" via normal graph queries. |

### Tier 3 — aspirational / not shippable near term

| File | Why kept |
|---|---|
| `adrs/ADR-022-semantic-relationship-taxonomy.md` | Their 30-type taxonomy — useful as a vocabulary seed even if Lucy generates its own dynamically. |
| `adrs/ADR-025-dynamic-relationship-vocabulary.md` | How dynamic vocabulary works end-to-end. Long read; only the mechanism matters for Lucy. |
| `adrs/ADR-026-autonomous-vocabulary-curation.md` | The "Lucy curates her own vocabulary" idea — depends on having the rest of the stack working first. |
| `adrs/ADR-030-concept-deduplication.md` | Quality validation around concept dedup. Relevant when Lucy's memory grows past ~10k entries. |
| `adrs/ADR-032.1`, `032.2` | Skipped (numbered .1/.2 implementation notes; the parent ADRs ship the meat). |
| `adrs/ADR-039-local-embedding-service.md` | Local embedding worker architecture. Lucy already does embeddings via Anthropic / Ollama; only the worker-isolation pattern is worth noting. |
| `adrs/ADR-041-ai-extraction-config.md` | Extraction prompts config. Lucy has its own extraction flow. |
| `adrs/ADR-045-unified-embedding-generation.md` | All embeddings flow through one worker. Operational hygiene, not a product feature. |
| `adrs/ADR-053-eager-vocabulary-categorization.md` | Variant of ADR-047. Read after ADR-047. |
| `adrs/ADR-059-llm-determined-relationship-direction.md` | LLM picks edge directionality. Pairs with ADR-058 if implemented. |
| `adrs/ADR-072-concept-matching-strategies.md` | Matching strategies during ingestion. Relevant if Lucy adds doc-ingest. |
| `adrs/ADR-077-vocabulary-explorers.md` | UI patterns for browsing the vocabulary graph. Useful for `MemoryBrowserView` v2. |
| `adrs/ADR-089-deterministic-node-edge-creation.md` | Deterministic IDs for replayability. Lucy's `memory_core` already does this; reference for tightening. |

### Schema files

- `schema/init.cypher` — every constraint and index Neo4j ever needed. The
  vector index params (`vector.dimensions: 1536`, `cosine`) and the full-text
  indexes (`instance_fulltext` on quote text, `concept_fulltext` on labels)
  port directly to Lucy's SQLite via `sqlite-vec` and FTS5.
- `schema/00_baseline.sql` — the Apache AGE schema. **Lucy does NOT use
  Postgres**, but this is the canonical reference for the data model
  (nodes/edges/properties). Read for shape, not for syntax.
- `schema/11_graph_accel.sql` — the Rust extension boundary. 19 lines.
  Skim only if interested in their in-memory BFS approach.

### Reference docs

- `reference/README.md` — project overview (their public README).
- `reference/docs_reference_ARCHITECTURE_OVERVIEW.md` — the system from
  10,000 ft. Read this first if you don't know the project.
- `reference/docs_architecture_INDEX.md` — the full ADR catalog (107 ADRs).
  Use this to find anything not mirrored here.
- `reference/docs_guides_EPISTEMIC-STATUS-FILTERING.md` — concrete user-facing
  example of grounding filters in action. Useful when designing Lucy's
  Settings → Memory confidence slider.

## Proposed Lucy integration plan

### v1.6.0 — "Grounding + provenance"

1. Schema migration: add `confidence FLOAT DEFAULT 0.5` to `agent_memories`
   and `memory_core`. Add a new `memory_evidence(memory_id, kind, weight)`
   table (kind = `support`/`contradict`).
2. Implement `compute_grounding(memory_id) -> f32` at query time per
   ADR-044. **Do NOT cache.**
3. Filter `confidence > 0.20` by default when injecting memories into the
   prompt. Expose threshold in Settings → Memory.
4. New `memory_instances(memory_id, quote_text, source_kind, source_ref,
   offset_start, offset_end)` with FTS5 — drives "where did Lucy learn this".
5. `MemoryBrowserView` chip: `◉ 87% confianza` + a "View instances" expand.

### v1.6.1 — "Polarity triangulation"

1. Create `$lib/memory-polarity.ts` with 5 SP/EN anchor pairs.
2. Boot-time job: compute the polarity axis once, cache in Rust process
   memory with epoch invalidation on `vocabulary_changed`.
3. Each `log_chip_event` now records `polarity_score` = projection onto axis.
4. Layer 3 scoring formula changes from
   `Σ clicks − 0.6·Σ dismisses`
   to
   `Σ confidence_i · polarity_score_i`.
5. Drop the hard-coded `event_kind` mapping in `chip_memory.rs`.

### v1.6.2 — "Annealing ontologies" (aspirational)

If v1.6.0/v1.6.1 land cleanly, read ADR-200 end-to-end and prototype a
nightly job that clusters Lucy's memories by embedding cosine similarity
into self-named ontologies (low-energy assignments win). Wire results into
`MemoryGraphView` as colored clusters.

## What we deliberately did NOT mirror

- 80+ ADRs about Postgres/Apache AGE deployment, RBAC, OAuth, scheduled jobs,
  CDN deployment — irrelevant to a desktop SQLite app.
- The `web/`, `cli/`, `operator/`, `docker/` directories — only the
  algorithmic ideas were the goal.
- ADRs in Draft status not on our integration map.

If you need anything that ISN'T here, fetch with:

```bash
curl -O https://raw.githubusercontent.com/aaronsb/knowledge-graph-system/main/<path>
```

## License note

The upstream project's license at the time of mirroring is in
`reference/README.md`. This research mirror is for internal Lucy reference
only; any code we re-implement is our own, but credit the ideas back to
ADR numbers in commit messages.
