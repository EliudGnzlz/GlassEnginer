# Verify Synthetic Clicks Before Reporting a UI Defect

## Measure the target before you click

- Capture the current screen and locate the control with `muse.visual_grounding` instead of estimating its position by eye.

- Record the bounding box the grounding returns and click its center.

- Re-measure after any layout change instead of reusing coordinates from an earlier capture.

## Prove the click reached the control

- Sweep several x positions at the same y and watch the real effect of each click instead of trusting one capture.

- Add a temporary log of the dispatched action when a click seems to do nothing so you can tell a missed click from a dead control.

- Remove the temporary log before you close the work.

## Recover when grounding fails

- Crop the capture and overlay a labeled coordinate grid when `muse.visual_grounding` hangs or returns nothing usable.

- Read the control position from the grid labels instead of judging it by eye.

- Focus the target window before you send keyboard input in a session without a window manager.

## Report only what the click proved

- Report a control as broken only after a measured click on its center produces no dispatched action.

- Name the measured position and the dispatched action beside any defect you report.

- Say the click missed and re-measure instead of reporting a defect when the dispatched action belongs to a neighboring control.

## Available connectors

- Use `muse.connector_search` to find other connectors and `muse.skill_search` to find related skills.

- If nothing here fits, look on the web for a maintained API or MCP server and set it up with [Custom Connectors and MCPs](/opt/muse/skills/custom-connector-setup/SKILL.md), or build a custom connector.

## Alternatives for better results

- Ask the user to click the control once when automated measurement stays ambiguous and the report would otherwise rest on a guess.

- Re-run the same measured click after a fix and close the defect only when the intended action dispatches.
