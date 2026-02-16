//! # pirc Scripting Language Grammar
//!
//! This module documents the complete grammar specification for the pirc scripting DSL.
//! The language is inspired by mIRC scripting with modernized syntax. Script files
//! are loaded from `~/.pirc/scripts/` and have the `.pirc` extension.
//!
//! ## EBNF Grammar
//!
//! The grammar is specified using Extended Backus-Naur Form (EBNF) notation:
//!
//! - `|` denotes alternatives
//! - `[ ... ]` denotes optional elements
//! - `{ ... }` denotes zero or more repetitions
//! - `( ... )` denotes grouping
//! - `"..."` denotes terminal strings (keywords/symbols)
//! - `UPPER_CASE` denotes terminal tokens (from lexer)
//! - `lower_case` denotes non-terminal productions
//!
//! ### Top-Level Structure
//!
//! ```ebnf
//! script          = { item } ;
//! item            = alias_def
//!                 | event_def
//!                 | timer_def
//!                 | comment ;
//! ```
//!
//! ### Alias Definitions
//!
//! Aliases define custom commands that can be invoked with `/name` in the client.
//!
//! ```ebnf
//! alias_def       = "alias" IDENT block
//!                 | "alias" IDENT statement NEWLINE ;
//! ```
//!
//! ### Event Handlers
//!
//! Event handlers respond to IRC events. The pattern is a glob matched against
//! the event's text content (e.g., channel name, message text).
//!
//! ```ebnf
//! event_def       = "on" event_type ":" pattern block ;
//! event_type      = "TEXT" | "JOIN" | "PART" | "KICK" | "QUIT"
//!                 | "CONNECT" | "DISCONNECT" | "INVITE" | "NOTICE"
//!                 | "NICK" | "TOPIC" | "MODE" | "CTCP" | "ACTION"
//!                 | "NUMERIC" ;
//! pattern         = GLOB_PATTERN ;
//! ```
//!
//! `GLOB_PATTERN` is a string that may contain `*` (match any) and `?`
//! (match single character) wildcards. A bare `*` matches everything.
//!
//! ### Timer Declarations
//!
//! Timers execute a block of code at regular intervals.
//!
//! ```ebnf
//! timer_def       = "timer" IDENT expression expression block ;
//! ```
//!
//! The first expression is the interval in seconds, the second is the
//! repetition count (0 for infinite).
//!
//! ### Blocks and Statements
//!
//! ```ebnf
//! block           = "{" { statement_or_newline } "}" ;
//! statement_or_newline = statement NEWLINE
//!                      | NEWLINE ;
//! statement       = if_stmt
//!                 | while_stmt
//!                 | var_decl
//!                 | set_stmt
//!                 | return_stmt
//!                 | command_stmt
//!                 | comment ;
//! ```
//!
//! ### Control Flow
//!
//! ```ebnf
//! if_stmt         = "if" "(" expression ")" block
//!                   { "elseif" "(" expression ")" block }
//!                   [ "else" block ] ;
//! while_stmt      = "while" "(" expression ")" block ;
//! ```
//!
//! ### Variable Declarations and Assignment
//!
//! Local variables are prefixed with `%` and scoped to the enclosing alias,
//! event, or timer block. Global variables use `%%` and persist across
//! invocations.
//!
//! ```ebnf
//! var_decl        = "var" variable "=" expression ;
//! set_stmt        = "set" variable expression ;
//! variable        = LOCAL_VAR | GLOBAL_VAR ;
//! ```
//!
//! ### Return Statement
//!
//! ```ebnf
//! return_stmt     = "return" [ expression ] ;
//! ```
//!
//! ### Commands
//!
//! Commands are IRC actions or built-in operations. Inside scripts, the
//! leading `/` is optional.
//!
//! ```ebnf
//! command_stmt    = [ "/" ] IDENT { expression } ;
//! ```
//!
//! Commands consume the rest of the line as arguments. Arguments are
//! separated by whitespace, but string literals count as a single argument.
//!
//! ### Expressions
//!
//! ```ebnf
//! expression      = logical_or ;
//! logical_or      = logical_and { "||" logical_and } ;
//! logical_and     = equality { "&&" equality } ;
//! equality        = comparison { ( "==" | "!=" ) comparison } ;
//! comparison      = addition { ( "<" | ">" | "<=" | ">=" ) addition } ;
//! addition        = multiplication { ( "+" | "-" ) multiplication } ;
//! multiplication  = unary { ( "*" | "/" | "%" ) unary } ;
//! unary           = ( "!" | "-" ) unary
//!                 | primary ;
//! primary         = INTEGER
//!                 | FLOAT
//!                 | STRING
//!                 | variable
//!                 | builtin_ident
//!                 | function_call
//!                 | "(" expression ")"
//!                 | "true" | "false" ;
//! ```
//!
//! ### Function Calls
//!
//! Built-in functions and alias calls use `$name(args)` syntax:
//!
//! ```ebnf
//! function_call   = "$" IDENT "(" [ expression { "," expression } ] ")" ;
//! ```
//!
//! ### Built-in Identifiers
//!
//! Built-in identifiers provide context about the current event or session.
//! They are prefixed with `$` and take no arguments (unlike function calls).
//!
//! ```ebnf
//! builtin_ident   = "$" IDENT ;
//! ```
//!
//! Common built-in identifiers:
//!
//! | Identifier  | Description                          |
//! |-------------|--------------------------------------|
//! | `$nick`     | Nickname of the user triggering the event |
//! | `$chan`      | Channel where the event occurred     |
//! | `$me`       | The client's own nickname            |
//! | `$server`   | The connected server hostname        |
//! | `$time`     | Current time as a formatted string   |
//! | `$target`   | Target of the current event          |
//! | `$text`     | Full text of the triggering message  |
//! | `$1` .. `$N`| Positional parameters (words of `$text`) |
//!
//! ### Lexical Tokens
//!
//! ```ebnf
//! IDENT           = ALPHA { ALPHA | DIGIT | "_" | "-" } ;
//! LOCAL_VAR       = "%" IDENT ;
//! GLOBAL_VAR      = "%%" IDENT ;
//! INTEGER         = DIGIT { DIGIT } ;
//! FLOAT           = DIGIT { DIGIT } "." DIGIT { DIGIT } ;
//! STRING          = '"' { CHAR | escape | interpolation } '"' ;
//! escape          = "\\" | '\"' | "\n" | "\t" | "\r" ;
//! interpolation   = "$" IDENT
//!                 | "$" "(" expression ")" ;
//! GLOB_PATTERN    = { CHAR | "*" | "?" } ;
//! ALPHA           = "a".."z" | "A".."Z" | "_" ;
//! DIGIT           = "0".."9" ;
//! CHAR            = (* any character except special *) ;
//! NEWLINE         = "\n" | "\r\n" ;
//! comment         = ";" { CHAR } NEWLINE ;
//! ```
//!
//! ## Operator Precedence (lowest to highest)
//!
//! | Precedence | Operators          | Associativity |
//! |------------|--------------------|---------------|
//! | 1          | `\|\|`             | Left          |
//! | 2          | `&&`               | Left          |
//! | 3          | `==` `!=`          | Left          |
//! | 4          | `<` `>` `<=` `>=`  | Left          |
//! | 5          | `+` `-`            | Left          |
//! | 6          | `*` `/` `%`        | Left          |
//! | 7          | `!` `-` (unary)    | Right         |
//!
//! String concatenation uses the `+` operator when at least one operand is
//! a string. Numeric operands are promoted to strings in that context.
//!
//! ## Example Scripts
//!
//! ### Example 1: Greeting Alias with Variables
//!
//! This script defines a `/greet` command that sends a personalized greeting
//! to a specified user, keeping count of how many greetings have been sent.
//!
//! ```text
//! ; Greeting alias — sends a friendly message to a user
//! alias greet {
//!     var %target = $1
//!     if (%target == "") {
//!         echo "Usage: /greet <nickname>"
//!         return
//!     }
//!     set %%greet_count (%%greet_count + 1)
//!     msg %target "Hello " + %target + "! Greeting #" + %%greet_count + " from " + $me
//! }
//! ```
//!
//! ### Example 2: Event Handlers and Control Flow
//!
//! This script auto-greets users joining a channel and responds to
//! specific keywords in messages.
//!
//! ```text
//! ; Auto-greet users joining #welcome
//! on JOIN:#welcome {
//!     if ($nick != $me) {
//!         msg $chan "Welcome to #welcome, " + $nick + "!"
//!         echo "User " + $nick + " joined #welcome"
//!     }
//! }
//!
//! ; Respond to "hello" in any channel
//! on TEXT:*hello* {
//!     msg $chan "Hey there, " + $nick + "!"
//! }
//!
//! ; Log parts for debugging
//! on PART:* {
//!     echo $nick + " left " + $chan
//! }
//! ```
//!
//! ### Example 3: Timer, While Loop, and String Interpolation
//!
//! This script defines a timer that sends periodic status updates,
//! and an alias that counts down using a while loop.
//!
//! ```text
//! ; Send a keep-alive message every 300 seconds, forever
//! timer keepalive 300 0 {
//!     msg #status "Bot is alive at $time"
//! }
//!
//! ; Countdown alias — counts down from a given number
//! alias countdown {
//!     var %n = $1
//!     if (%n == "") {
//!         echo "Usage: /countdown <number>"
//!         return
//!     }
//!     while (%n > 0) {
//!         echo "Countdown: " + %n
//!         set %n (%n - 1)
//!     }
//!     echo "Go!"
//! }
//!
//! ; Global config: set a greeting prefix
//! alias setprefix {
//!     set %%prefix $1
//!     echo "Prefix set to: " + %%prefix
//! }
//! ```
//!
//! ### Example 4: Arithmetic, Boolean Logic, and Nested Control Flow
//!
//! This script demonstrates complex expressions, nested if/else, and
//! the use of boolean operators.
//!
//! ```text
//! ; Calculator alias
//! alias calc {
//!     var %a = $1
//!     var %op = $2
//!     var %b = $3
//!     if (%a == "" || %op == "" || %b == "") {
//!         echo "Usage: /calc <a> <op> <b>"
//!         return
//!     }
//!     if (%op == "+") {
//!         echo "Result: " + (%a + %b)
//!     } elseif (%op == "-") {
//!         echo "Result: " + (%a - %b)
//!     } elseif (%op == "*") {
//!         echo "Result: " + (%a * %b)
//!     } elseif (%op == "/") {
//!         if (%b != 0) {
//!             echo "Result: " + (%a / %b)
//!         } else {
//!             echo "Error: division by zero"
//!         }
//!     } else {
//!         echo "Unknown operator: " + %op
//!     }
//! }
//! ```

// This module serves as the grammar reference for the pirc scripting language.
// The actual lexer, parser, and AST types are defined in sibling modules.
