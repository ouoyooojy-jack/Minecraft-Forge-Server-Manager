# Project agent workflow

Use these roles when work is delegated in this project. The primary Codex
agent is the main brain: it owns scope, acceptance criteria, integration, and
final decisions.

## Roles

- **Project agent A and B:** each independently plans and challenges the
  request. They then agree on a single implementation brief and independently
  accept or reject the completed result. Neither reviews its own production
  implementation.
- **Implementation agent:** makes only the approved change, updates focused
  tests, and reports verification evidence. It does not commit, push, or
  clean files.
- **Test agent:** runs the agreed checks and reports reproducible failures.
- **Git manager:** reviews `git status` and the diff, stages only approved
  files, commits only with an agreed message, and pushes only when explicitly
  authorized by the user.
- **Cleanup manager:** starts only after implementation and testing finish. It
  may remove ignored/generated artifacts such as `__pycache__/`,
  `.pytest_cache/`, `htmlcov/`, `.coverage*`, `build/`, and `dist/`; first
  inspect `git status --short` and preview with `git clean -ndX`. Never remove
  source, tests, fixtures, `downloads/`, or uncertain untracked files.

## Delivery loop

1. Main brain writes: outcome, constraints, affected areas, acceptance
   criteria, and required tests.
2. Project agents A and B independently propose a plan, discuss differences,
   and submit one approved implementation brief.
3. An implementation agent works from that brief; a test agent verifies it.
4. Both project agents review against the criteria. On a material failure,
   they record the failed criterion, revise the brief, and the main brain
   dispatches a fresh implementation agent. Repeat until accepted.
5. After acceptance, invoke cleanup. Invoke the Git manager last; commits and
   pushes remain explicit user-authorized actions.

## Brief template

```text
Outcome:
Constraints:
Acceptance criteria:
Tests required:
Files/areas in scope:
```
