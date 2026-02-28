# Scripting Language Reference

pirc includes a domain-specific scripting language inspired by mIRC scripting. Scripts automate client interactions through aliases (custom commands), event handlers, and timers.

## Getting Started

Script files use the `.pirc` extension and are loaded from `~/.pirc/scripts/`. The script engine loads all `.pirc` files in this directory on startup.

```bash
# Create the scripts directory
mkdir -p ~/.pirc/scripts/

# Create your first script
cat > ~/.pirc/scripts/hello.pirc << 'EOF'
alias hello {
    echo "Hello from pirc scripting!"
}
EOF
```

After loading, type `/hello` in the client to run the alias.

## Language Overview

A pirc script consists of three types of top-level definitions:

- **Aliases** — custom commands invoked with `/name`
- **Event handlers** — code that runs in response to IRC events
- **Timers** — code that runs at regular intervals

Comments start with `;` and continue to the end of the line.

## Aliases

Aliases define custom commands. They can be single-line or multi-line with a block:

```
; Single-line alias
alias hi msg $chan "Hello!"

; Multi-line alias with block
alias greet {
    var %target = $1
    if (%target == "") {
        echo "Usage: /greet <nickname>"
        return
    }
    msg %target "Hello, " + %target + "!"
}
```

Arguments passed to the alias are available as `$1`, `$2`, etc.

## Event Handlers

Event handlers respond to IRC events. The syntax is:

```
on <event_type>:<pattern> {
    ; handler code
}
```

The pattern is a glob that filters which events trigger the handler. Use `*` to match everything or `?` to match a single character.

### Event Types

| Event | Trigger | Context Variables |
|-------|---------|-------------------|
| `TEXT` | Message received in a channel | `$nick`, `$chan`, `$text` |
| `JOIN` | User joins a channel | `$nick`, `$chan` |
| `PART` | User leaves a channel | `$nick`, `$chan` |
| `KICK` | User is kicked from a channel | `$nick`, `$chan` |
| `QUIT` | User disconnects | `$nick` |
| `CONNECT` | Client connects to server | `$server` |
| `DISCONNECT` | Client disconnects | `$server` |
| `INVITE` | Invited to a channel | `$nick`, `$chan` |
| `NOTICE` | Notice received | `$nick`, `$text` |
| `NICK` | User changes nickname | `$nick` |
| `TOPIC` | Channel topic changes | `$nick`, `$chan`, `$text` |
| `MODE` | Mode change | `$nick`, `$chan` |
| `CTCP` | CTCP message received | `$nick`, `$text` |
| `ACTION` | Action message (/me) | `$nick`, `$chan`, `$text` |
| `NUMERIC` | Numeric reply from server | `$text` |

### Examples

```
; Auto-greet users joining #welcome
on JOIN:#welcome {
    if ($nick != $me) {
        msg $chan "Welcome, " + $nick + "!"
    }
}

; Respond to keywords in any channel
on TEXT:*hello* {
    msg $chan "Hey there, " + $nick + "!"
}

; Log all parts
on PART:* {
    echo $nick + " left " + $chan
}
```

## Timers

Timers execute code at regular intervals:

```
timer <name> <interval_seconds> <repeat_count> {
    ; timer code
}
```

Set `repeat_count` to `0` for infinite repetition.

```
; Send status every 5 minutes, forever
timer status 300 0 {
    msg #status "Bot is alive at $time"
}

; One-shot timer: run once after 10 seconds
timer welcome 10 1 {
    msg #general "I'm here!"
}
```

## Variables

### Local Variables

Local variables are prefixed with `%` and scoped to the enclosing alias, event, or timer block:

```
var %count = 0
set %count (%count + 1)
echo "Count: " + %count
```

### Global Variables

Global variables are prefixed with `%%` and persist across invocations:

```
set %%greet_count (%%greet_count + 1)
echo "Total greetings: " + %%greet_count
```

### Declaration vs Assignment

- `var %x = <expr>` — declares a new local variable
- `set %x <expr>` — assigns to an existing variable (local or global)

## Expressions

### Data Types

- **Integers:** `42`, `0`, `-1`
- **Floats:** `3.14`, `0.5`
- **Strings:** `"hello"`, `"value: $var"`
- **Booleans:** `true`, `false`

### Operators

Listed from lowest to highest precedence:

| Precedence | Operators | Description |
|------------|-----------|-------------|
| 1 | `\|\|` | Logical OR |
| 2 | `&&` | Logical AND |
| 3 | `==` `!=` | Equality |
| 4 | `<` `>` `<=` `>=` | Comparison |
| 5 | `+` `-` | Addition / subtraction / string concatenation |
| 6 | `*` `/` `%` | Multiplication / division / modulo |
| 7 | `!` `-` (unary) | Logical NOT / negation |

The `+` operator performs string concatenation when at least one operand is a string.

### Parenthesized Expressions

Use parentheses to control evaluation order:

```
var %result = (%a + %b) * %c
set %x (%x - 1)
```

## Control Flow

### If / Elseif / Else

```
if (%score > 90) {
    echo "Excellent!"
} elseif (%score > 70) {
    echo "Good"
} else {
    echo "Keep trying"
}
```

### While Loops

```
var %i = 5
while (%i > 0) {
    echo "Countdown: " + %i
    set %i (%i - 1)
}
echo "Go!"
```

### Return

Exit an alias early, optionally with a value:

```
alias validate {
    if ($1 == "") {
        echo "Error: no input"
        return
    }
    ; continue processing...
}
```

## Built-in Identifiers

Identifiers prefixed with `$` provide context about the current session or event:

| Identifier | Description |
|------------|-------------|
| `$me` | The client's own nickname |
| `$nick` | Nickname of the user that triggered the event |
| `$chan` | Channel where the event occurred |
| `$server` | Connected server hostname |
| `$port` | Server port number |
| `$time` | Current time as a formatted string |
| `$target` | Target of the current event |
| `$text` | Full text of the triggering message |
| `$1` .. `$N` | Positional parameters (words of `$text` or alias arguments) |

### String Interpolation

Identifiers can be used inside double-quoted strings:

```
echo "Hello $nick, welcome to $chan!"
msg $chan "The time is $time"
```

For complex expressions, use `$(<expr>)`:

```
echo "Result: $(%a + %b)"
```

## Function Calls

Built-in functions use `$name(args)` syntax:

```
var %result = $len("hello")
var %upper = $upper($1)
```

## Commands

Inside scripts, IRC commands can be used directly (the leading `/` is optional):

```
; These are equivalent:
msg #general "Hello"
/msg #general "Hello"

; Common commands:
msg <target> <text>       ; Send a message
notice <target> <text>    ; Send a notice
join <channel>            ; Join a channel
part <channel>            ; Leave a channel
nick <new_nick>           ; Change nickname
echo <text>               ; Print to local output
```

## Complete Example

```
; auto.pirc — Automation script for pirc

; Track greeting count globally
alias greet {
    var %target = $1
    if (%target == "") {
        echo "Usage: /greet <nickname>"
        return
    }
    set %%greet_count (%%greet_count + 1)
    msg %target "Hello " + %target + "! Greeting #" + %%greet_count + " from " + $me
}

; Auto-greet users joining #welcome
on JOIN:#welcome {
    if ($nick != $me) {
        msg $chan "Welcome to #welcome, " + $nick + "!"
    }
}

; Respond to "hello" in any channel
on TEXT:*hello* {
    msg $chan "Hey there, " + $nick + "!"
}

; Calculator alias
alias calc {
    var %a = $1
    var %op = $2
    var %b = $3
    if (%a == "" || %op == "" || %b == "") {
        echo "Usage: /calc <a> <op> <b>"
        return
    }
    if (%op == "+") {
        echo "Result: " + (%a + %b)
    } elseif (%op == "-") {
        echo "Result: " + (%a - %b)
    } elseif (%op == "*") {
        echo "Result: " + (%a * %b)
    } elseif (%op == "/") {
        if (%b != 0) {
            echo "Result: " + (%a / %b)
        } else {
            echo "Error: division by zero"
        }
    } else {
        echo "Unknown operator: " + %op
    }
}

; Periodic keepalive every 5 minutes
timer keepalive 300 0 {
    msg #status "Bot alive at $time"
}
```

## Grammar (EBNF)

For the complete formal grammar specification, see the source code in `pirc-scripting/src/grammar.rs`.

```ebnf
script          = { item } ;
item            = alias_def | event_def | timer_def | comment ;

alias_def       = "alias" IDENT block | "alias" IDENT statement NEWLINE ;
event_def       = "on" event_type ":" pattern block ;
timer_def       = "timer" IDENT expression expression block ;

block           = "{" { statement NEWLINE | NEWLINE } "}" ;
statement       = if_stmt | while_stmt | var_decl | set_stmt
                | return_stmt | command_stmt | comment ;

if_stmt         = "if" "(" expression ")" block
                  { "elseif" "(" expression ")" block }
                  [ "else" block ] ;
while_stmt      = "while" "(" expression ")" block ;
var_decl        = "var" variable "=" expression ;
set_stmt        = "set" variable expression ;
return_stmt     = "return" [ expression ] ;
command_stmt    = [ "/" ] IDENT { expression } ;

variable        = "%" IDENT | "%%" IDENT ;
comment         = ";" { CHAR } NEWLINE ;
```
