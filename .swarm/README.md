# Swarm Reference

Swarm is a workflow orchestration system for AI agents. It manages structured,
iterative development workflows with PostgreSQL state persistence, configurable
process rules, and template-driven action execution.

## Directory Structure

```
.swarm/
  CLAUDE.md              Agent orientation (do not modify unless working on config)
  README.md              This reference document
  config.toml            Project configuration (agent, database, git, process)
  docker-compose.yml     PostgreSQL container for state persistence
  migrations/            Database schema migrations
  system-prompt.md       System prompt template (customizable)
  workflows/             Custom workflow overrides (optional)
    <name>/
      definitions.toml   Term definitions
      roles.toml         Agent roles
      skills.toml        Reusable instruction blocks
      process.toml       Rules, rulesets, actions, hooks, aliases
      actions/           Action template files (.md)
      skel/              Project skeleton files
  scripts/               Custom bash scripts for aliases (optional)
  templates/             Custom templates (optional)
  plugins/               Plugin definitions (optional)
```

## Core Concepts

Swarm uses UPPERCASE terms defined in the active workflow's `definitions.toml`.
The standard software workflow defines:

**SPECIFICATION** - Requirements and constraints document for the project.
Written once during setup, referenced throughout development.

**GOAL** - Strategic direction set by the user. Guides planning and prioritization.

**PLAN** - Development roadmap. Contains sequential PHASEs, each with EPICs.

**PHASE** - A milestone grouping within a PLAN. Contains related EPICs.

**EPIC** - A major feature or work unit. States: open, in-progress, closed.
Auto-transitions: creating a TICKET moves it to in-progress; closing all TICKETs
moves it to closed.

**TICKET** - A discrete work item within an EPIC. Numbered T001, T002, etc.
States: open, closed, merged.

**ITERATION** - One complete workflow cycle for an EPIC. Includes action execution,
criteria validation, and result recording.

**CR (Change Request)** - Code review unit. Numbered CR #1, CR #2, etc.
States: open, closed, merged. Created after implementation for peer review.

**DECISION** - Architecture Decision Record. Documents significant technical choices
made during development.

## Workflow Lifecycle

### Setup Phase

When a project is first created, setup actions run to establish foundations:

1. **Write SPECIFICATION** - Capture requirements and constraints
2. **Set GOAL** - User sets strategic direction
3. **Create PLAN** - Generate development roadmap with PHASEs and EPICs

Setup actions typically have `run_once = true` so they execute only once.

### Work Phase (Iteration Loop)

After setup, the workflow enters an iterative loop:

1. **Process engine** determines the next action (via rules or queen)
2. **Action executor** loads the action template, renders it, and runs the agent
3. **Criteria validator** checks success conditions
4. **Iteration recorded**, loop continues

A typical work cycle:
- Select an EPIC to work on
- Create TICKETs for the EPIC
- Implement each TICKET (write code, create commits)
- Review via CR (code review)
- Triage feedback and iterate
- Close EPIC when all TICKETs are done

### Completion

The workflow ends when exit conditions are met (e.g., all EPICs closed) or
when the user stops execution.

## Process Engine

The process engine determines which action to execute next. It is configured
in `process.toml` and supports two modes:

- **Rules mode** (`process_type = "rules"`): Declarative rules evaluate project
  state to select the next action automatically.
- **Queen mode** (`process_type = "queen"`): A queen agent observes state and
  dispatches actions dynamically.

### Rules

Rules are conditions that evaluate to true or false. Two formats are supported:

**Bash rules** (legacy):
```toml
[rules.git-repo-exists]
condition = "test -d .git"
description = "Git repository initialized"
```

**Structured rules** (recommended):
```toml
[rules.has-spec]
variable = "has_spec"
comparator = "eq"
value = true
description = "SPECIFICATION exists"
```

Structured rules use one of four source types:

**variable** - System-computed values (counts, booleans, strings):
```toml
[rules.has-open-tickets]
variable = "open_ticket_count"
comparator = "gt"
value = 0
```

**state** - Current state table fields (nullable):
```toml
[rules.has-current-epic]
state = "current_epic"
comparator = "exists"
value = true
```

**metadata** - Custom workflow fields (always stored as strings):
```toml
[rules.review-mode-strict]
metadata = "reviewMode"
comparator = "eq"
value = "strict"
```

**file** - File system checks (existence, size, glob counts):
```toml
[rules.has-tests]
file = "**/*_test.rs"
comparator = "gt"
value = 0
```

#### System Variables (22 total)

Count variables (integer):
- `epic_count` - Total EPICs across all PHASEs
- `open_epic_count` - EPICs with status 'open'
- `in_progress_epic_count` - EPICs with status 'in-progress'
- `closed_epic_count` - EPICs with status 'closed'
- `ticket_count` - Total TICKETs
- `open_ticket_count` - TICKETs with state 'open'
- `closed_ticket_count` - TICKETs with state 'closed'
- `phase_count` - Total PHASEs
- `iteration_count` - Total ITERATIONs
- `cr_count` - Total CRs
- `open_cr_count` - CRs with state 'open'
- `closed_cr_count` - CRs with state 'closed' or 'merged'

Boolean variables:
- `has_spec` - SPECIFICATION exists
- `has_goal` - GOAL exists
- `has_plan` - PLAN exists
- `has_iteration_report` - Current ITERATION has a report
- `has_epic_summary` - Current EPIC has a summary

String variables:
- `last_review_status` - Last CR review status: "denied", "approved", "commented", "none"

#### State Fields

- `current_epic` - Current EPIC ID (integer, nullable)
- `current_ticket` - Current TICKET ID (integer, nullable)
- `current_cr` - Current CR number (integer, nullable)
- `current_iteration` - Current ITERATION ID (integer, nullable)
- `current_phase` - Current PHASE ID (integer, nullable)
- `current_action` - Last executed action name (string, nullable)
- `status` - Project status (string, nullable)
- `last_reviewed_cr` - Last reviewed CR number (integer, nullable)

#### Comparators (9 total)

Numeric: `gt`, `lt`, `gte`, `lte`, `eq`, `ne`
Boolean: `exists` (checks if value is set / not null)
String: `contains` (substring match), `matches` (regex pattern)

### Rulesets

Rulesets combine rules with logical operators:

```toml
[rulesets.ready-for-work]
mode = "all"    # AND - all rules must pass
rules = ["has-spec", "has-plan", "has-current-epic"]

[rulesets.needs-setup]
mode = "any"    # OR - at least one must pass
rules = ["missing-spec", "missing-plan"]

[rulesets.no-blockers]
mode = "none"   # NOT - no rules should pass
rules = ["has-open-crs", "has-failing-tests"]
```

Rulesets can reference other rulesets for nesting (up to 10 levels).
Short-circuit evaluation is used for performance.

### Actions

Actions are the executable units of a workflow. Each action has a template
(markdown file) that is rendered and passed to the AI agent.

```toml
[actions.implement]
template = "actions/implement.md"     # Path to action template
priority = 50                         # Lower = runs first (default: 100)
rulesets = ["ready-for-work"]         # Preconditions (all must pass)
rules = ["has-current-ticket"]        # Additional rule checks
run_once = false                      # Whether to execute only once
interactive = true                    # Agent-driven (true) or automated (false)
model = "opus"                        # LLM model override (optional)
context = ["epic-selection"]          # Context injection type (optional)
prompt = "Implement the ticket"       # Custom prompt (optional)
allow_consecutive = false             # Allow running same action back-to-back
```

**Success criteria** validate whether an action completed correctly:
```toml
[actions.implement]
template = "actions/implement.md"
success_criteria = ["test -f src/main.rs"]        # Bash commands
# Or: success_criteria_rulesets = ["impl-complete"]  # Ruleset reference
max_criteria_retries = 3                           # Retry count on failure
```

**Action variants** provide alternative templates based on conditions:
```toml
[actions.implement.variants]
first-time = { rulesets = ["first-implementation"], template = "actions/implement-first.md" }
```

**Dependencies** ensure prerequisite actions have run:
```toml
[actions.implement]
dependencies = ["create-tickets"]
```

**Imports** allow reusing action definitions from other workflows:
```toml
[actions.implement]
import = "softwarev2:implement"
```

### Hooks

Hooks are bash commands that run at lifecycle events:

```toml
[[hooks]]
name = "log-session-start"
event = "session_start"                    # When swarm run starts
command = "echo 'Session started' >> log"

[[hooks]]
name = "fetch-before-implement"
event = "action_pre"                       # Before action executes
command = "git fetch origin"
actions = ["implement"]                    # Only for these actions
rules = ["has-current-ticket"]             # Only if rules pass
conditions = ["test -d .git"]              # Only if bash conditions pass
```

Events: `session_start`, `session_end`, `cycle_start`, `cycle_end`,
`action_load`, `action_pre`, `action_post`

Hook environment variables:
- `SWARM_EVENT` - The triggering event
- `SWARM_ACTION` - Current action name (action events only)
- `SWARM_CYCLE` - Work cycle number (cycle events only)
- `SWARM_EXIT_CODE` - Exit code (end/post events only)
- `SWARM_PROJECT_ID`, `SWARM_PROJECT_NAME`, `SWARM_WORKFLOW`
- `SWARM_PROJECT_STATUS`, `SWARM_ITERATION`, `SWARM_PHASE`
- `SWARM_EPIC`, `SWARM_CURRENT_TICKET`, `SWARM_CURRENT_CR`
- `SWARM_LAST_REVIEWED_CR`, `SWARM_BRANCH`, `SWARM_REPO_ROOT`

### Aliases and Scripts

**Aliases** are inline command shortcuts:
```toml
[[aliases]]
name = "fmt"
command = "cargo fmt && cargo clippy"
description = "Format and lint code"
usage = "fmt"
```

Run with: `swarm exec fmt`

Aliases support parameter substitution: `$1`, `$2`, `$@`, `$*`, `$#`

**Scripts** reference bash files in `.swarm/scripts/`:
```toml
[[scripts]]
name = "deploy"
description = "Deploy to staging"
usage = "deploy <env>"
```

The script file lives at `.swarm/scripts/deploy.sh`.
Run with: `swarm script run deploy staging`

### Workflow Variables

Named values defined in `process.toml`:
```toml
[variables]
project_root = "/path/to/project"
src_dir = "{{$project_root}}/src"        # Reference other variables
git_hash = "{{!git rev-parse HEAD}}"     # Bash command expansion
```

Variables are topologically sorted for dependency resolution.
Available as `SWARM_VAR_*` environment variables during execution.
Referenced in templates as `{{$variable_name}}`.

## Template Engine

Templates use two-pass rendering:

**Pass 1 - Variable substitution**: `{{variable_name}}`
```markdown
You are operating in {{execution_mode}} mode.
```

**Pass 2 - Command execution**: `{{!command}}`
```markdown
## Current Status
{{!swarm status}}

## Workflow Context
{{!swarm process context}}
```

**Workflow variable references**: `{{$variable_name}}`
```markdown
Source directory: {{$src_dir}}
```

Rules:
- Variable names: alphanumeric + underscore
- Undefined variables become `{{UNDEFINED:variable_name}}`
- Commands execute via `bash -c` with full environment inheritance
- Command timeout: configurable (default 30 seconds)
- Commands inherit all SWARM_* environment variables

## Configuration Reference

All settings are in `.swarm/config.toml`. Configuration loads from:
1. Default values (built-in)
2. config.toml file
3. Workflow-specific defaults (from process.toml metadata)
4. Environment variables (highest priority)

### Top-Level Fields

| Field | Type | Default | Env Override | Description |
|-------|------|---------|-------------|-------------|
| `project_name` | string | - | - | Project name for Docker labels |
| `instance_id` | string | - | - | Unique ID for container naming |
| `workflow` | string | "software" | - | Active workflow type |

### [agent]

| Field | Type | Default | Env Override | Description |
|-------|------|---------|-------------|-------------|
| `agent` | string | "claude" | `SWARM_AGENT` | Agent vendor (Claude Code) |
| `unsafe_mode` | bool | false | `SWARM_AGENT_UNSAFE` | Skip permission prompts |
| `model` | string | - | - | Model profile override |
| `extra_args` | string[] | [] | - | Additional CLI arguments |

### [database]

| Field | Type | Default | Env Override | Description |
|-------|------|---------|-------------|-------------|
| `max_connections` | u32 | 10 | `SWARM_DB_MAX_CONNECTIONS` | Connection pool size (1-1000) |
| `connection_timeout_seconds` | u32 | 30 | `SWARM_DB_CONNECTION_TIMEOUT` | Connect timeout |
| `idle_timeout_seconds` | u32 | 600 | `SWARM_DB_IDLE_TIMEOUT` | Idle connection timeout |

### [vendor.postgres]

| Field | Type | Default | Env Override | Description |
|-------|------|---------|-------------|-------------|
| `connection_string` | string | - | - | PostgreSQL connection URL |

Use `"env:DATABASE_URL"` to read from environment variable.

### [process]

| Field | Type | Default | Env Override | Description |
|-------|------|---------|-------------|-------------|
| `default_branch` | string | "main" | `SWARM_DEFAULT_BRANCH` | Branch for CRs and merges |
| `auto_push` | bool | false | `SWARM_AUTO_PUSH` | Auto-push commits to remote |
| `auto_create_cr` | bool | false | `SWARM_AUTO_CREATE_CR` | Auto-create CRs after iterations |
| `max_iteration_history` | u32 | 100 | `SWARM_MAX_ITERATION_HISTORY` | Max iterations in history (>0) |
| `coffee_break_minutes` | u32 | 0 | `SWARM_COFFEE_BREAK_MINUTES` | Pause between steps (0=none) |
| `max_consecutive_action_runs` | u32 | 2 | `SWARM_MAX_CONSECUTIVE_ACTION_RUNS` | Max same-action repeats (0=off) |

### [git]

| Field | Type | Default | Env Override | Description |
|-------|------|---------|-------------|-------------|
| `commit_message_template` | string | "{title}" | `SWARM_COMMIT_TEMPLATE` | Template with {title}, {iteration}, {ticket_id} |
| `sign_commits` | bool | false | `SWARM_SIGN_COMMITS` | GPG-sign commits |
| `require_linear_history` | bool | false | `SWARM_REQUIRE_LINEAR_HISTORY` | Require linear git history |
| `default_merge_method` | string | "squash" | `SWARM_DEFAULT_MERGE_METHOD` | CR merge method: merge, squash, rebase |

### [paths]

| Field | Type | Default | Env Override | Description |
|-------|------|---------|-------------|-------------|
| `swarm_dir` | string | ".swarm" | `SWARM_DIR` | Base Swarm directory |
| `actions_dir` | string | "actions" | `SWARM_ACTIONS_DIR` | Action definitions directory |
| `roles_dir` | string | "roles" | `SWARM_ROLES_DIR` | Role definitions directory |

All paths must be relative (no leading `/`).

### [console]

| Field | Type | Default | Env Override | Description |
|-------|------|---------|-------------|-------------|
| `accent_color` | string | "orange" | `SWARM_ACCENT_COLOR` | Primary color: orange, blue, green, cyan, magenta, yellow, red, white |
| `secondary_color` | string | "white" | - | Secondary color (same options) |
| `spinners_enabled` | bool | true | `SWARM_SPINNERS_ENABLED` | Show animated spinners |
| `unicode_enabled` | bool | true | - | Use unicode characters |
| `colors_enabled` | bool | true | `SWARM_COLORS_ENABLED` | Color output (set `NO_COLOR` to disable) |
| `spinner_delay_ms` | u64 | 100 | - | Minimum ms before showing spinner |

### [templates]

| Field | Type | Default | Env Override | Description |
|-------|------|---------|-------------|-------------|
| `enable_command_execution` | bool | true | - | Allow `{{!command}}` in templates |
| `command_timeout` | u64 | 30 | - | Command timeout in seconds (>0) |

### [workflow]

| Field | Type | Default | Env Override | Description |
|-------|------|---------|-------------|-------------|
| `enable_epic_priority` | bool | true | `SWARM_ENABLE_EPIC_PRIORITY` | Show priority field for EPICs |
| `enable_ticket_priority` | bool | true | `SWARM_ENABLE_TICKET_PRIORITY` | Show priority field for TICKETs |

### [commit_hooks]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `pre` | string[] | [] | Commands to run before git commits |
| `post` | string[] | [] | Commands to run after git commits |

## Customizing Workflows

### Override Resolution

When loading workflow files, Swarm checks these locations in order:
1. `.swarm/workflows/<name>/<file>` - Workflow-specific custom override
2. `.swarm/<file>` - Global custom override
3. Embedded resources (built-in workflows shipped with binary)

### Exporting a Workflow

```bash
# Export the software workflow to .swarm/workflows/software/
swarm workflows export software
```

This copies all embedded files so you can edit them. Files in this location
take precedence over built-in versions.

### Creating a Custom Workflow

```bash
# Interactive wizard to create a new workflow
swarm workflows create
```

This copies the example workflow to `.swarm/workflows/<name>/` as a starting
point with heavy documentation.

### Available Workflows

- `software` / `softwarev2` - Software development (rules or queen mode)
- `legal` - Legal document workflows
- `story` - Creative writing workflows

## Key Commands

### Project Management
```
swarm init <dir>              Initialize a new Swarm project
swarm status                  Show current project state
swarm context                 Show full workflow context
swarm project create <name>   Create a new project
swarm project list            List all projects
swarm project switch <name>   Switch active project
```

### Workflow Execution
```
swarm run                     Start the workflow loop
swarm run --once              Execute one iteration only
swarm handover -m "msg"       Return control to queen (queen mode)
```

### Process Engine
```
swarm process next            Show next action to execute
swarm process next --debug    Show next action with rule evaluation
swarm process validate        Validate rules and rulesets
swarm debug process           Debug process engine state
swarm debug process --verbose Full evaluation trace with variables
```

### EPICs and TICKETs
```
swarm epic list               List all EPICs
swarm epic create --title "X" Create a new EPIC
swarm epic close <name>       Close an EPIC
swarm ticket list             List TICKETs
swarm ticket create --title "X" --body "..." --type feature
swarm ticket claim T001       Claim a TICKET for work
swarm ticket close T001       Close a TICKET
swarm ticket finish T001 -m "feat: message"  Commit and prepare for review
```

### Code Review
```
swarm cr list                 List change requests
swarm cr show <num>           Show CR details
swarm cr approve <num>        Approve and merge CR
swarm cr request-changes <num> -m "feedback"
```

### Configuration
```
swarm config list             Show all configuration values
swarm config get <key>        Get a specific value
swarm config set <key> <val>  Set a configuration value
```

### Workflow Components
```
swarm definitions list        List all definitions
swarm definitions get TERM    Get a specific definition
swarm roles list              List all roles
swarm roles get ROLE          Get role details
swarm skills list             List all skills
swarm skills get SKILL        Get skill content
swarm workflows export <name> Export workflow for customization
```

### State and Metadata
```
swarm state show              Show current state fields
swarm metadata list           List all metadata
swarm metadata get <field>    Get metadata value
swarm metadata set --field <key> --value <val>
```

### Documentation
```
swarm docs                    Browse documentation
swarm docs <topic>            Read specific topic
swarm man <command>           Show command manpage
```

### Utilities
```
swarm exec <alias>            Run a defined alias
swarm exec --list             List all aliases
swarm script list             List all scripts
swarm script run <name>       Run a script
swarm doctor                  Check project health and fix issues
swarm migrate status          Show migration status
swarm migrate up              Run pending migrations
```

## Troubleshooting

### Process Engine Not Selecting Expected Action

```bash
# See which action would run and why
swarm process next --debug

# Full rule evaluation with variable values
swarm debug process --verbose

# Validate all rules and rulesets
swarm process validate
```

### Database Connection Issues

```bash
# Check PostgreSQL container status
docker compose -f .swarm/docker-compose.yml ps

# Start database
docker compose -f .swarm/docker-compose.yml up -d

# Check migration status
swarm migrate status

# Run pending migrations
swarm migrate up
```

### Project Health Check

```bash
# Run the doctor command to detect and fix issues
swarm doctor

# Auto-fix missing boilerplate files
swarm doctor --fix
```

### Common Issues

**"No action matched"** - No rules evaluate to true for any action.
Check state with `swarm status` and rules with `swarm debug process --verbose`.

**"Max consecutive action runs"** - Same action ran too many times in a row.
Check `max_consecutive_action_runs` in config.toml (set to 0 to disable).

**"Template command failed"** - A `{{!command}}` in a template returned non-zero.
Test the command manually. Check the 30-second timeout.

**Missing files after init** - Run `swarm doctor --fix` to recreate missing
boilerplate files.
