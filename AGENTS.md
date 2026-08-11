<!-- CODEGRAPH_START -->
## CodeGraph

In repositories indexed by CodeGraph (a `.codegraph/` directory exists at the repo root), reach for it BEFORE grep/find or reading files when you need to understand or locate code:

- **MCP tool** (when available): `codegraph_explore` answers most code questions in one call - the relevant symbols' verbatim source plus the call paths between them, including dynamic-dispatch hops grep cannot follow. Name a file or symbol in the query to read its current line-numbered source. If it is listed but deferred, load it by name via tool search.
- **Shell** (always works): `codegraph explore "<symbol names or question>"` prints the same output.

If there is no `.codegraph/` directory, skip CodeGraph entirely - indexing is the user's decision.
<!-- CODEGRAPH_END -->

## Implementation Plans

When creating or updating files under `docs/superpowers/plans/`, read and
follow `docs/agents/implementation-plan-guidelines.md`.

This repository guideline overrides generic planning templates that require
repeating the full TDD cycle, implementation snippets, verification steps, or
commit commands inside every task. Approved design specifications remain the
source of truth for technical contracts, event ordering, security boundaries,
and failure behavior.
