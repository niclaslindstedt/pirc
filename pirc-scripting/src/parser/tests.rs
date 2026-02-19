use super::*;
use crate::lexer::Lexer;

/// Helper: lex and parse source text, returning the Script AST.
fn parse_source(source: &str) -> Result<Script, ParseError> {
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().expect("lexer should succeed");
    let mut parser = Parser::new(tokens, source);
    parser.parse()
}

#[test]
fn parse_empty_script() {
    let script = parse_source("").unwrap();
    assert!(script.items.is_empty());
}

#[test]
fn parse_empty_script_with_whitespace_and_comments() {
    let script = parse_source("\n\n; comment\n\n").unwrap();
    assert!(script.items.is_empty());
}

#[test]
fn parse_alias_block_form() {
    let script = parse_source("alias greet {\n  echo hello\n}").unwrap();
    assert_eq!(script.items.len(), 1);
    match &script.items[0] {
        TopLevelItem::Alias(alias) => {
            assert_eq!(alias.name, "greet");
            assert_eq!(alias.body.len(), 1);
            match &alias.body[0] {
                Statement::Command(cmd) => {
                    assert_eq!(cmd.name, "echo");
                    assert_eq!(cmd.args.len(), 1);
                }
                other => panic!("expected Command, got {other:?}"),
            }
        }
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_alias_single_line_form() {
    let script = parse_source("alias greet echo hello").unwrap();
    assert_eq!(script.items.len(), 1);
    match &script.items[0] {
        TopLevelItem::Alias(alias) => {
            assert_eq!(alias.name, "greet");
            assert_eq!(alias.body.len(), 1);
            match &alias.body[0] {
                Statement::Command(cmd) => {
                    assert_eq!(cmd.name, "echo");
                    assert_eq!(cmd.args.len(), 1);
                }
                other => panic!("expected Command, got {other:?}"),
            }
        }
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_event_with_pattern() {
    let script = parse_source("on TEXT:*hello* {\n  msg $chan \"Hi!\"\n}").unwrap();
    assert_eq!(script.items.len(), 1);
    match &script.items[0] {
        TopLevelItem::Event(event) => {
            assert_eq!(event.event_type, EventType::Text);
            assert_eq!(event.pattern, "*hello*");
            assert_eq!(event.body.len(), 1);
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn parse_event_without_pattern() {
    let script = parse_source("on JOIN {\n  echo \"Someone joined\"\n}").unwrap();
    assert_eq!(script.items.len(), 1);
    match &script.items[0] {
        TopLevelItem::Event(event) => {
            assert_eq!(event.event_type, EventType::Join);
            assert_eq!(event.pattern, "*");
            assert_eq!(event.body.len(), 1);
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn parse_event_wildcard_pattern() {
    let script = parse_source("on PART:* {\n  echo left\n}").unwrap();
    assert_eq!(script.items.len(), 1);
    match &script.items[0] {
        TopLevelItem::Event(event) => {
            assert_eq!(event.event_type, EventType::Part);
            assert_eq!(event.pattern, "*");
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn parse_event_identifier_pattern() {
    let script = parse_source("on JOIN:welcome {\n  echo joined\n}").unwrap();
    assert_eq!(script.items.len(), 1);
    match &script.items[0] {
        TopLevelItem::Event(event) => {
            assert_eq!(event.event_type, EventType::Join);
            assert_eq!(event.pattern, "welcome");
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn parse_timer() {
    let script = parse_source("timer mytimer 5000 0 {\n  echo tick\n}").unwrap();
    assert_eq!(script.items.len(), 1);
    match &script.items[0] {
        TopLevelItem::Timer(timer) => {
            assert_eq!(timer.name, "mytimer");
            match &timer.interval {
                Expression::IntLiteral { value, .. } => assert_eq!(*value, 5000),
                other => panic!("expected IntLiteral for interval, got {other:?}"),
            }
            match &timer.repetitions {
                Expression::IntLiteral { value, .. } => assert_eq!(*value, 0),
                other => panic!("expected IntLiteral for repetitions, got {other:?}"),
            }
            assert_eq!(timer.body.len(), 1);
        }
        other => panic!("expected Timer, got {other:?}"),
    }
}

#[test]
fn parse_multiple_top_level_items() {
    let source = "\
alias greet {
  echo hello
}

on TEXT:*hi* {
  echo matched
}

timer tick 1000 0 {
  echo tick
}
";
    let script = parse_source(source).unwrap();
    assert_eq!(script.items.len(), 3);
    assert!(matches!(&script.items[0], TopLevelItem::Alias(_)));
    assert!(matches!(&script.items[1], TopLevelItem::Event(_)));
    assert!(matches!(&script.items[2], TopLevelItem::Timer(_)));
}

#[test]
fn parse_alias_with_command_args() {
    let script = parse_source("alias greet {\n  msg $chan \"Hello everyone!\"\n}").unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => {
            assert_eq!(alias.body.len(), 1);
            match &alias.body[0] {
                Statement::Command(cmd) => {
                    assert_eq!(cmd.name, "msg");
                    assert_eq!(cmd.args.len(), 2);
                    match &cmd.args[0] {
                        Expression::BuiltinId { name, .. } => assert_eq!(name, "chan"),
                        other => panic!("expected BuiltinId, got {other:?}"),
                    }
                    match &cmd.args[1] {
                        Expression::StringLiteral { value, .. } => {
                            assert_eq!(value, "Hello everyone!");
                        }
                        other => panic!("expected StringLiteral, got {other:?}"),
                    }
                }
                other => panic!("expected Command, got {other:?}"),
            }
        }
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_alias_with_slash_command() {
    let script = parse_source("alias greet {\n  /msg $chan \"hi\"\n}").unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => match &alias.body[0] {
            Statement::Command(cmd) => {
                assert_eq!(cmd.name, "msg");
                assert_eq!(cmd.args.len(), 2);
            }
            other => panic!("expected Command, got {other:?}"),
        },
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_multiple_statements_in_block() {
    let source = "\
alias test {
  echo first
  echo second
  echo third
}
";
    let script = parse_source(source).unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => {
            assert_eq!(alias.body.len(), 3);
            for stmt in &alias.body {
                assert!(matches!(stmt, Statement::Command(_)));
            }
        }
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn error_unexpected_token_at_top_level() {
    let result = parse_source("42");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, ParseError::UnexpectedToken { .. }));
}

#[test]
fn error_missing_brace_after_alias() {
    let result = parse_source("alias greet :");
    assert!(result.is_err());
}

#[test]
fn error_missing_alias_name() {
    let result = parse_source("alias { echo hi }");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, ParseError::UnexpectedToken { .. }));
}

#[test]
fn error_invalid_event_type() {
    let result = parse_source("on BOGUS:* { echo hi }");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, ParseError::InvalidEventType { .. }));
}

#[test]
fn parse_return_statement_in_alias() {
    let script = parse_source("alias greet {\n  return\n}").unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => {
            assert_eq!(alias.body.len(), 1);
            match &alias.body[0] {
                Statement::Return(ret) => assert!(ret.value.is_none()),
                other => panic!("expected Return, got {other:?}"),
            }
        }
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_break_continue_statements() {
    let script = parse_source("alias test {\n  break\n  continue\n}").unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => {
            assert_eq!(alias.body.len(), 2);
            assert!(matches!(&alias.body[0], Statement::Break(_)));
            assert!(matches!(&alias.body[1], Statement::Continue(_)));
        }
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_all_event_types() {
    let event_types = [
        ("TEXT", EventType::Text),
        ("JOIN", EventType::Join),
        ("PART", EventType::Part),
        ("KICK", EventType::Kick),
        ("QUIT", EventType::Quit),
        ("CONNECT", EventType::Connect),
        ("DISCONNECT", EventType::Disconnect),
        ("INVITE", EventType::Invite),
        ("NOTICE", EventType::Notice),
        ("NICK", EventType::Nick),
        ("TOPIC", EventType::Topic),
        ("MODE", EventType::Mode),
        ("CTCP", EventType::Ctcp),
        ("ACTION", EventType::Action),
        ("NUMERIC", EventType::Numeric),
    ];

    for (name, expected_type) in event_types {
        let source = format!("on {name}:* {{ echo hi }}");
        let script = parse_source(&source).unwrap();
        match &script.items[0] {
            TopLevelItem::Event(event) => {
                assert_eq!(event.event_type, expected_type, "failed for event type {name}");
            }
            other => panic!("expected Event for {name}, got {other:?}"),
        }
    }
}

#[test]
fn parse_case_insensitive_event_type() {
    // Event types should be case-insensitive
    let script = parse_source("on text:* { echo hi }").unwrap();
    match &script.items[0] {
        TopLevelItem::Event(event) => {
            assert_eq!(event.event_type, EventType::Text);
        }
        other => panic!("expected Event, got {other:?}"),
    }
}

#[test]
fn parse_comments_between_items() {
    let source = "\
; first alias
alias greet { echo hello }
; second alias
alias bye { echo goodbye }
";
    let script = parse_source(source).unwrap();
    assert_eq!(script.items.len(), 2);
}

#[test]
fn error_eof_in_block() {
    let result = parse_source("alias greet {");
    assert!(result.is_err());
}

#[test]
fn parse_timer_with_variable_args() {
    let script = parse_source("timer t1 %interval 10 {\n  echo hi\n}").unwrap();
    match &script.items[0] {
        TopLevelItem::Timer(timer) => {
            assert_eq!(timer.name, "t1");
            assert!(matches!(timer.interval, Expression::Variable { .. }));
            match &timer.repetitions {
                Expression::IntLiteral { value, .. } => assert_eq!(*value, 10),
                other => panic!("expected IntLiteral, got {other:?}"),
            }
        }
        other => panic!("expected Timer, got {other:?}"),
    }
}

// =======================================================================
// Statement parsing tests (T234)
// =======================================================================

#[test]
fn parse_if_statement() {
    let source = "alias test {\n  if (%x) {\n    echo yes\n  }\n}";
    let script = parse_source(source).unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => {
            assert_eq!(alias.body.len(), 1);
            match &alias.body[0] {
                Statement::If(if_stmt) => {
                    assert!(matches!(if_stmt.condition, Expression::Variable { .. }));
                    assert_eq!(if_stmt.then_body.len(), 1);
                    assert!(if_stmt.elseif_branches.is_empty());
                    assert!(if_stmt.else_body.is_none());
                }
                other => panic!("expected If, got {other:?}"),
            }
        }
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_if_else() {
    let source = "alias test {\n  if (%x) {\n    echo yes\n  } else {\n    echo no\n  }\n}";
    let script = parse_source(source).unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => match &alias.body[0] {
            Statement::If(if_stmt) => {
                assert_eq!(if_stmt.then_body.len(), 1);
                assert!(if_stmt.elseif_branches.is_empty());
                assert!(if_stmt.else_body.is_some());
                assert_eq!(if_stmt.else_body.as_ref().unwrap().len(), 1);
            }
            other => panic!("expected If, got {other:?}"),
        },
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_if_elseif_else_chain() {
    let source = "\
alias test {
  if (%x) {
    echo a
  } elseif (%y) {
    echo b
  } elseif (%z) {
    echo c
  } else {
    echo d
  }
}";
    let script = parse_source(source).unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => match &alias.body[0] {
            Statement::If(if_stmt) => {
                assert_eq!(if_stmt.then_body.len(), 1);
                assert_eq!(if_stmt.elseif_branches.len(), 2);
                assert!(if_stmt.else_body.is_some());
            }
            other => panic!("expected If, got {other:?}"),
        },
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_while_loop() {
    let source = "alias test {\n  while (%i) {\n    echo looping\n  }\n}";
    let script = parse_source(source).unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => match &alias.body[0] {
            Statement::While(while_stmt) => {
                assert!(matches!(while_stmt.condition, Expression::Variable { .. }));
                assert_eq!(while_stmt.body.len(), 1);
            }
            other => panic!("expected While, got {other:?}"),
        },
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_var_declaration() {
    let source = "alias test {\n  var %count = 0\n}";
    let script = parse_source(source).unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => match &alias.body[0] {
            Statement::VarDecl(var) => {
                assert_eq!(var.name, "count");
                assert!(!var.global);
                match &var.value {
                    Expression::IntLiteral { value, .. } => assert_eq!(*value, 0),
                    other => panic!("expected IntLiteral, got {other:?}"),
                }
            }
            other => panic!("expected VarDecl, got {other:?}"),
        },
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_set_local_variable() {
    let source = "alias test {\n  set %name \"world\"\n}";
    let script = parse_source(source).unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => match &alias.body[0] {
            Statement::Set(set) => {
                assert_eq!(set.name, "name");
                assert!(!set.global);
                assert!(matches!(set.value, Expression::StringLiteral { .. }));
            }
            other => panic!("expected Set, got {other:?}"),
        },
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_set_global_variable() {
    let source = "alias test {\n  set %%config 42\n}";
    let script = parse_source(source).unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => match &alias.body[0] {
            Statement::Set(set) => {
                assert_eq!(set.name, "config");
                assert!(set.global);
                match &set.value {
                    Expression::IntLiteral { value, .. } => assert_eq!(*value, 42),
                    other => panic!("expected IntLiteral, got {other:?}"),
                }
            }
            other => panic!("expected Set, got {other:?}"),
        },
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_return_with_value() {
    let source = "alias test {\n  return %result\n}";
    let script = parse_source(source).unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => match &alias.body[0] {
            Statement::Return(ret) => {
                assert!(ret.value.is_some());
                assert!(matches!(
                    ret.value.as_ref().unwrap(),
                    Expression::Variable { .. }
                ));
            }
            other => panic!("expected Return, got {other:?}"),
        },
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_command_with_mixed_args() {
    let source = "alias test {\n  msg $chan %name \"hello\" 42\n}";
    let script = parse_source(source).unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => match &alias.body[0] {
            Statement::Command(cmd) => {
                assert_eq!(cmd.name, "msg");
                assert_eq!(cmd.args.len(), 4);
                assert!(matches!(cmd.args[0], Expression::BuiltinId { .. }));
                assert!(matches!(cmd.args[1], Expression::Variable { .. }));
                assert!(matches!(cmd.args[2], Expression::StringLiteral { .. }));
                assert!(matches!(cmd.args[3], Expression::IntLiteral { .. }));
            }
            other => panic!("expected Command, got {other:?}"),
        },
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_nested_if_in_while() {
    let source = "\
alias test {
  while (%running) {
    if (%x) {
      echo found
    }
    echo tick
  }
}";
    let script = parse_source(source).unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => match &alias.body[0] {
            Statement::While(while_stmt) => {
                assert_eq!(while_stmt.body.len(), 2);
                assert!(matches!(&while_stmt.body[0], Statement::If(_)));
                assert!(matches!(&while_stmt.body[1], Statement::Command(_)));
            }
            other => panic!("expected While, got {other:?}"),
        },
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn parse_var_global_declaration() {
    let source = "alias test {\n  var %%gvar = true\n}";
    let script = parse_source(source).unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => match &alias.body[0] {
            Statement::VarDecl(var) => {
                assert_eq!(var.name, "gvar");
                assert!(var.global);
                assert!(matches!(var.value, Expression::BoolLiteral { value: true, .. }));
            }
            other => panic!("expected VarDecl, got {other:?}"),
        },
        other => panic!("expected Alias, got {other:?}"),
    }
}

#[test]
fn error_missing_condition_parens() {
    let result = parse_source("alias test {\n  if %x {\n    echo hi\n  }\n}");
    assert!(result.is_err());
}

#[test]
fn error_missing_closing_brace() {
    let result = parse_source("alias test {\n  if (%x) {\n    echo hi\n}");
    assert!(result.is_err());
}

#[test]
fn parse_multiple_statements_with_control_flow() {
    let source = "\
alias complex {
  var %i = 0
  while (%i) {
    if (%i) {
      echo match
      break
    }
    set %i 1
  }
  return %i
}";
    let script = parse_source(source).unwrap();
    match &script.items[0] {
        TopLevelItem::Alias(alias) => {
            assert_eq!(alias.body.len(), 3);
            assert!(matches!(&alias.body[0], Statement::VarDecl(_)));
            assert!(matches!(&alias.body[1], Statement::While(_)));
            assert!(matches!(&alias.body[2], Statement::Return(_)));
        }
        other => panic!("expected Alias, got {other:?}"),
    }
}
