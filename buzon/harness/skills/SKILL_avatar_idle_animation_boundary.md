# Avatar Idle Animation Boundary

## Recognize an idle animation request

- Treat a request to change the waiting animation or the idle animation as different from a request for dancing videos.

- Do not treat a general request for another animation as a request for dancing videos when the user names the idle animation or the waiting animation.

## Keep the approved image unchanged

- Preserve the approved still image exactly while an idle choice is pending.

- Do not generate dancing videos, dance moves, or a new avatar image for an idle animation request.

- Ask which idle behavior the user wants, or present the available idle choices, before generating anything.

- Say so plainly when no idle change tool is available instead of substituting a dance video.

## Available connectors

- Uses native avatar tools; no account connector is required.

- Use `muse.connector_search` to find other connectors and `muse.skill_search` to find related skills.

- If nothing here fits, look on the web for a maintained API or MCP server and set it up with [Custom Connectors and MCPs](/opt/muse/skills/custom-connector-setup/SKILL.md), or build a custom connector.

## Alternatives for better results

- Leave the approved still image unchanged while the user chooses an idle behavior.
