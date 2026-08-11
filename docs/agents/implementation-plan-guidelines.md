# Efficient Implementation Plan Guidelines

## Purpose And Precedence

This guide governs implementation plans created or updated under
`docs/superpowers/plans/`. It keeps plans concise, reviewable, and executable by
moving common procedure into global rules and reserving task bodies for
task-specific differences and dependencies.

For this repository, this guide overrides generic planning templates that
require every task to repeat the complete TDD loop, implementation snippets,
verification boilerplate, or commit commands. It does not override an approved
design specification. The specification remains the single source of truth for
interfaces, states, ordering, security boundaries, rollback, and acceptance.

## Required Structure

### 1. Title, Goal, And Architecture

Start with the feature name, the expected result, and the governing
architecture in one to three short paragraphs.

### 2. Technology And Global Constraints

List only relevant dependency versions and hard constraints, such as lock
order, window lifetime, caller authorization, absence of input synthesis, or
platform limitations. Link the approved design specification when one exists.

### 3. Core Contract Overview (Optional)

Use one central section when many tasks share new DTOs, enums, or structures.
Prefer an exact reference to the design specification's terminology or wire
contract instead of redefining it. Include code only when the design leaves a
small implementation decision unresolved and the snippet prevents ambiguity.

### 4. Global Execution Rules

State repeated workflow once:

- Every task follows TDD: add focused failing tests, confirm the intended
  failure, implement the minimum contract, rerun focused tests, and commit.
- Every task produces at least one atomic commit. Review fixes may add separate
  commits. A commit must not mix another task or pre-existing user changes.
- State common command conventions once. Each task lists only its focused test
  module, test file, or command suffix.
- State dependency order explicitly, for example
  `Task 1 -> Task 2 -> Tasks 3 and 4`.
- Each task receives specification-compliance and code-quality review before a
  dependent task begins when the active workflow provides those gates.

### 5. Task List

Every task heading must use this exact parseable form:

```markdown
### Task N: Title
```

Use checkbox items for trackable implementation work. Each task contains:

- **Files:** files created or modified.
- **Dependencies:** preceding tasks or named contracts it consumes. Omit only
  when the task is independent.
- **Distinct test coverage:** scenarios unique to this task, including the
  expected behavior. Do not repeat generic compilation or TDD steps.
- **Implementation points:** the task-specific logic, without redefining shared
  contracts or narrating standard mechanics.
- **Verify:** the exact focused command, such as
  `cargo test result_registry::tests` or `npm test -- find-core.test.ts`.

The `### Task N: Title` format is mandatory because
`subagent-driven-development` extracts task briefs from these headings.

### 6. Final Checklist (Optional)

Summarize cross-task acceptance, or cite the approved specification's
acceptance section. Include full-suite build, test, lint, and manual gates only
once.

## Writing Rules

### Remove Repetition

- Do not repeat `write failing test -> run red -> implement -> run green ->
  commit` inside every task.
- Do not repeat shared DTO, enum, or interface definitions. Define them once or
  cite the exact specification section.
- Do not list the same test scenario under multiple tasks. Assign it to the
  earliest task that owns the behavior.
- Do not repeat full `git add` and `git commit` examples per task.
- Do not include test code merely to demonstrate an ordinary assertion. State
  intent and expected behavior; generate concrete tests during TDD execution.

### Mandatory Detail Exceptions

The following must retain separate, explicit test descriptions and must not be
collapsed into a generic phrase such as "test races":

- **Concurrent linearization order**, such as readiness preparation not
  draining a queued forward.
- **Security boundaries and caller authorization**, such as `main` being
  rejected by a find-only command before protected state access.
- **Failure rollback**, such as restoring captured native state after a focus
  transfer timeout.
- **Ownership across asynchronous operations**, such as pin changing while a
  file execution is pending.
- **Event sequences with different outcomes**, such as a delayed event becoming
  stale after a newer forward replaces its owner.

Describe the initial state, event order, and expected terminal state for each
such case. Parameterize only variants that genuinely share the same ordering
and expected outcome.

### Task Size

- A task is an independently testable and reversible logical unit.
- Two to five files is a heuristic, not a limit. Configuration, capability, and
  native window lifecycle changes often require more files when they form one
  inseparable behavior.
- Dependencies must be acyclic and named. Split a task when its pieces can be
  reviewed or rolled back independently; combine tightly coupled modules when
  separating them would leave an untestable intermediate state.

### Refer To Design Specifications Precisely

- Treat approved design documents as the canonical definitions of interfaces,
  states, data flow, failure behavior, and acceptance.
- Cite exact sections in each task, for example:
  `Follow Data Flow / Result Execution and Failure Behavior`.
- Never write only `follow the design document`; a task brief must tell a fresh
  worker exactly what to read.
- Put temporary implementation decisions in the plan only when the design does
  not already resolve them.

### System Interaction And Risk

Plans must identify operations that can affect the user's system or experience.
For real focus changes, system APIs, installation, permission changes, or other
observable side effects, state whether the action controls input and require
explicit user confirmation before execution when appropriate.

## Concise Task Template

```markdown
### Task N: Component Or Behavior

**Files:** `path/a`, `path/b`

**Dependencies:** Task N-1; design sections `Data Flow / ...` and
`Failure Behavior`.

- [ ] First task-specific implementation point.
- [ ] Second task-specific implementation point.

**Distinct test coverage:** ordered scenario and expected terminal behavior;
security or rollback case owned by this task.

**Verify:** `exact focused command`
```

## Author Self-Check

- [ ] The plan starts with goal, architecture, technology, and hard constraints.
- [ ] An approved design specification is linked when one exists.
- [ ] Common TDD, commit, review, and command rules appear only once.
- [ ] Every task heading is `### Task N: Title` and uses checkboxes for work.
- [ ] Every task lists only distinctive implementation and test behavior.
- [ ] Dependencies are explicit and acyclic.
- [ ] Shared interfaces and DTOs are defined once or cited precisely.
- [ ] Every design reference names exact sections.
- [ ] Every task has an exact focused verification command.
- [ ] Linearization, authorization, rollback, async ownership, and outcome-
  changing event sequences remain explicit.
- [ ] System interactions and required user confirmation are visible.
- [ ] Atomic commits map to task boundaries without absorbing user changes.

## Integration With Agent Workflows

When using `subagent-driven-development`, extract task briefs from the mandatory
`### Task N: Title` headings. Give each worker the task brief plus the exact
design sections named by that task. The plan should coordinate dependencies and
verification; the approved design should carry the detailed behavioral
contract.
