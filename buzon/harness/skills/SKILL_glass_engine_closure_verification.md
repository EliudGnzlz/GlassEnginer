# Verify GlassEnginer Work Before Closing It

## Run every build CI runs

- Run the editor build and the pure game build before you declare work closed.

- Run the test target for every crate and binary CI runs instead of relying on one green run.

- Treat a green editor build as proof of the editor build only and never as proof of the pure game build.

## Count tests from the output

- Quote the test count from the command output instead of repeating a count from memory or from an earlier run.

- Name the target that produced each count so a later check can repeat it.

- Stop and recount when two counts for the same target disagree instead of choosing the larger one.

## Sweep for regressions before closing

- Reconcile the converted roots and instance counts against the source package before you close an import or conversion fix.

- Name each source root you checked and the entity count it produced in the closure report.

- Treat a warning that names a dropped or unsupported asset as an open item even when it sits inside a large warning list.

- Do not close the work while a named dropped asset still has no converted root.

- When a fix touches shared foundations, re-verify through the real path the already-closed neighbor systems that share that machinery before you call the work closed.

- Name the neighbors you re-checked in the closure report because a green suite alone does not prove the old flows still work.

## Section sweep gate (permanent, approved by D3m3tr10 on 2026-11-25)

The source of truth is `~/workspace/glass-engine/INVENTARIO_BARRIDO_SECCIONES_2026-11-25.md`: §1 has one ficha per section (what it covers, how it is proven with evidence), §3 is the dependency map, §4 is this rule. The row in the map rules over memory; when a block touches two foundations, sweep the union of their rows.

- When a block opens, declare its owner section and its neighbor sections from the §3 map. Put the declaration in the first line of its plan or report.
- When a block closes, run the exact CI pipeline plus the physical sweep of the owner section and every declared neighbor, using each section's ficha in §1: scene and measurement points prepared beforehand, evidence with numbers, and a statement of what was verified in each section run.
- If the block touches the scene format or serialization, the closure requires the complete sweep of all sections, no exception for block size.
- Sections on pause are not swept unless the block touches their foundation; then run their minimum no-regression test (their ficha, trimmed to what the foundation affects).
- The closure report lists the sweep per section: pass / fail / not run with reason. A neighbor in red blocks the closure exactly like a red CI: fix it before stacking anything on top.
- Every new section is born with its ficha in the inventory before it is built (what it covers, how it is proven with evidence, notes), appended at the end in construction order.

First closure under this gate: Prefabs Fase B — owner S22 (prefabs vivos); neighbors from the §3 row «Prefabs: formato del asset o de la instancia»: S20 (importador), S1 (serialización), S14 (Inspector de instancia).

## Close only on green evidence

- Wait for the CI run for the closing commit to finish green before you call the phase or session closed.

- Record the CI run number beside the commit when you report the closure.

- Capture the scene from the same camera the user will judge it from and keep the capture with the closure report.

- Pixel-sample each named surface against its source texture or palette and quote the sampled values in the report.

- Repeat the exact step that uncovered a defect after its fix and close the fix only when that step passes again.

- Deliver the capture to the user before you call visual work closed.

- Do not close visual work on counts or warning totals alone when a named surface still needs a delivered capture.

## Report the closure with its costs

- List the token use per provider for the closed work in the closure message, separating cached from non-cached tokens whenever the provider reports them, because a high cache-hit rate makes an unsplit figure misleading.

- List the quoted test counts, the CI run number, the visual capture and the reconciled root counts beside the token line.

- Say plainly what remains open when any check is still red or unrun, including any dropped asset root, instead of calling the work closed.

## Available connectors

- Use `muse.connector_search` to find other connectors and `muse.skill_search` to find related skills.

- If nothing here fits, look on the web for a maintained API or MCP server and set it up with [Custom Connectors and MCPs](/opt/muse/skills/custom-connector-setup/SKILL.md), or build a custom connector.

## Alternatives for better results

- Split a large verification into smaller runs when one run hides which target failed.

- Take a second same-camera capture after a clean reimport when the first capture predates the fix you are closing.
