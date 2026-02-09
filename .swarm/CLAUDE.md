# .swarm/ Directory - Agent Guidance

This directory contains **Swarm workflow configuration and state**.
It is NOT project source code.

## Do Not Modify

Do not modify files in this directory unless you are explicitly working on
workflow configuration, process rules, or Swarm settings. Changes here affect
how the workflow engine operates, not the project itself.

## Key Files

- **README.md** - Comprehensive Swarm reference (start here to understand the system)
- **config.toml** - Project configuration (agent, database, git, process settings)
- **docker-compose.yml** - PostgreSQL database for state persistence

## When Working on Workflow Configuration

If you ARE instructed to modify workflow settings, these resources will help:

- `swarm docs` - Built-in documentation browser
- `swarm config list` - Show all current configuration values
- `swarm process next --debug` - Debug which action runs next and why
- `swarm debug process --verbose` - Full rule evaluation trace with variable values
- `swarm process validate` - Validate process rules and rulesets
- `swarm workflows export <name>` - Export a workflow for customization
- `swarm definitions list` - List all workflow definitions
- `swarm status` - Current project state

## Customization Paths

- **Configuration**: Edit `config.toml` (see comments inside for all options)
- **Process rules**: Create/edit `process.toml` for custom rules, hooks, aliases
- **Workflow overrides**: Place files in `workflows/<name>/` to override built-in workflows
- **Action templates**: Place custom action templates in `workflows/<name>/actions/`

See README.md in this directory for the full reference.
