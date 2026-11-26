# Run Session Monitors With Each Long Block

## Tie the monitors to the active block

- Start a session watch and an hourly progress report in the same act that starts a long block.

- Bind both monitors to that block so they follow its chat and its queue.

- Keep the watch interval short enough that a cut surfaces within one interval.

- Create and check the monitors with `cron.add` and `cron.list` instead of assuming they still run.

## Retire the monitors when the queue closes

- Retire both monitors in the same act that closes the queue.

- Send no hourly report and no watch message while no block is active.

- Leave releases and design decisions that wait on the user outside the monitors.

## Resume from saved state after a cut

- Verify the last saved log, commit and test result before you relaunch anything after a restart.

- Resume from the step the saved state proves complete instead of repeating it.

- Keep each watch pass inside its time budget and report a timeout as a timeout.

## Report only from live state

- Check the repository, the running processes and the latest monitor pass before you report how work stands.

- Say a cut and a relaunch plainly with the times you verified.

- Name what remains open instead of repeating a pending item as news.

## Available connectors

- Use `muse.connector_search` to find other connectors and `muse.skill_search` to find related skills.

- If nothing here fits, look on the web for a maintained API or MCP server and set it up with [Custom Connectors and MCPs](/opt/muse/skills/custom-connector-setup/SKILL.md), or build a custom connector.

## Alternatives for better results

- Split a long block into smaller sub-blocks when one watch pass cannot verify the whole block inside its budget.

- Verify a resumed block control by control when the saved log cannot show which step finished.
