use super::helpers::*;
use crate::ast::{self, Statement};
use crate::interpreter::environment::Environment;
use crate::interpreter::{CommandHandler, Interpreter, RuntimeError, Value};

#[test]
fn exec_var_decl_local() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let stmts = vec![
        var_decl("x", false, int_expr(10)),
        expr_stmt(var_expr("x")),
    ];
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_local("x"), Some(Value::Int(10)));
}

#[test]
fn exec_var_decl_global() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let stmts = vec![var_decl("count", true, int_expr(0))];
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_global("count"), Some(Value::Int(0)));
}

#[test]
fn exec_set_local() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let stmts = vec![
        var_decl("x", false, int_expr(1)),
        set_stmt("x", false, int_expr(99)),
    ];
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_local("x"), Some(Value::Int(99)));
}

#[test]
fn exec_set_global() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let stmts = vec![
        var_decl("g", true, int_expr(0)),
        set_stmt("g", true, int_expr(42)),
    ];
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_global("g"), Some(Value::Int(42)));
}

#[test]
fn exec_if_true_branch() {
    let stmts = vec![
        var_decl("result", false, str_expr("none")),
        if_stmt(
            bool_expr(true),
            vec![set_stmt("result", false, str_expr("then"))],
            vec![],
            Some(vec![set_stmt("result", false, str_expr("else"))]),
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_local("result"), Some(Value::String("then".into())));
}

#[test]
fn exec_if_false_to_else() {
    let stmts = vec![
        var_decl("result", false, str_expr("none")),
        if_stmt(
            bool_expr(false),
            vec![set_stmt("result", false, str_expr("then"))],
            vec![],
            Some(vec![set_stmt("result", false, str_expr("else"))]),
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_local("result"), Some(Value::String("else".into())));
}

#[test]
fn exec_if_elseif() {
    let stmts = vec![
        var_decl("result", false, str_expr("none")),
        if_stmt(
            bool_expr(false),
            vec![set_stmt("result", false, str_expr("if"))],
            vec![ast::ElseIfBranch {
                condition: bool_expr(true),
                body: vec![set_stmt("result", false, str_expr("elseif"))],
                span: span(),
            }],
            Some(vec![set_stmt("result", false, str_expr("else"))]),
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(
        env.get_local("result"),
        Some(Value::String("elseif".into()))
    );
}

#[test]
fn exec_if_no_match_no_else() {
    let stmts = vec![
        var_decl("result", false, str_expr("unchanged")),
        if_stmt(
            bool_expr(false),
            vec![set_stmt("result", false, str_expr("changed"))],
            vec![],
            None,
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(
        env.get_local("result"),
        Some(Value::String("unchanged".into()))
    );
}

#[test]
fn exec_while_basic() {
    // var %i = 0; while (%i < 5) { set %i (%i + 1) }
    let stmts = vec![
        var_decl("i", false, int_expr(0)),
        while_stmt(
            binary(var_expr("i"), ast::BinaryOp::Lt, int_expr(5)),
            vec![set_stmt(
                "i",
                false,
                binary(var_expr("i"), ast::BinaryOp::Add, int_expr(1)),
            )],
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_local("i"), Some(Value::Int(5)));
}

#[test]
fn exec_while_with_break() {
    // var %i = 0; while (true) { if (%i == 3) { break }; set %i (%i + 1) }
    let stmts = vec![
        var_decl("i", false, int_expr(0)),
        while_stmt(
            bool_expr(true),
            vec![
                if_stmt(
                    binary(var_expr("i"), ast::BinaryOp::Eq, int_expr(3)),
                    vec![Statement::Break(span())],
                    vec![],
                    None,
                ),
                set_stmt(
                    "i",
                    false,
                    binary(var_expr("i"), ast::BinaryOp::Add, int_expr(1)),
                ),
            ],
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_local("i"), Some(Value::Int(3)));
}

#[test]
fn exec_while_with_continue() {
    // Count only odd numbers: var %sum = 0; var %i = 0;
    // while (%i < 6) { set %i (%i + 1); if (%i % 2 == 0) { continue }; set %sum (%sum + %i) }
    let stmts = vec![
        var_decl("sum", false, int_expr(0)),
        var_decl("i", false, int_expr(0)),
        while_stmt(
            binary(var_expr("i"), ast::BinaryOp::Lt, int_expr(6)),
            vec![
                set_stmt(
                    "i",
                    false,
                    binary(var_expr("i"), ast::BinaryOp::Add, int_expr(1)),
                ),
                if_stmt(
                    binary(
                        binary(var_expr("i"), ast::BinaryOp::Mod, int_expr(2)),
                        ast::BinaryOp::Eq,
                        int_expr(0),
                    ),
                    vec![Statement::Continue(span())],
                    vec![],
                    None,
                ),
                set_stmt(
                    "sum",
                    false,
                    binary(var_expr("sum"), ast::BinaryOp::Add, var_expr("i")),
                ),
            ],
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    // 1 + 3 + 5 = 9
    assert_eq!(env.get_local("sum"), Some(Value::Int(9)));
}

#[test]
fn exec_return_value() {
    let stmts = vec![
        return_stmt(Some(int_expr(42))),
        // This should not be reached
        var_decl("x", false, int_expr(99)),
    ];
    let result = exec(&stmts);
    assert!(matches!(result, Err(RuntimeError::Return(Value::Int(42)))));
}

#[test]
fn exec_return_no_value() {
    let stmts = vec![return_stmt(None)];
    let result = exec(&stmts);
    assert!(matches!(result, Err(RuntimeError::Return(Value::Null))));
}

#[test]
fn exec_break_outside_loop() {
    let stmts = vec![Statement::Break(span())];
    let result = exec(&stmts);
    assert!(matches!(result, Err(RuntimeError::Break)));
}

#[test]
fn exec_continue_outside_loop() {
    let stmts = vec![Statement::Continue(span())];
    let result = exec(&stmts);
    assert!(matches!(result, Err(RuntimeError::Continue)));
}

#[test]
fn exec_command_handler() {
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let stmts = vec![cmd_stmt("echo", vec![str_expr("hello"), int_expr(42)])];
    {
        let mut interp = Interpreter::new(&mut env, &mut handler);
        interp.exec_stmts(&stmts).unwrap();
    }
    assert_eq!(handler.commands.len(), 1);
    assert_eq!(handler.commands[0].0, "echo");
    assert_eq!(handler.commands[0].1, vec![
        Value::String("hello".into()),
        Value::Int(42),
    ]);
}

#[test]
fn exec_command_stub_handler_rejects() {
    let stmts = vec![cmd_stmt("msg", vec![str_expr("#chan"), str_expr("hi")])];
    let result = exec(&stmts);
    // TestCommandHandler accepts commands, so this should succeed
    assert!(result.is_ok());

    // But RejectingHandler rejects
    let mut env = Environment::new();
    let mut handler = RejectingHandler;
    let mut interp = Interpreter::new(&mut env, &mut handler);
    let result = interp.exec_stmts(&stmts);
    assert!(matches!(result, Err(RuntimeError::UnknownCommand(_))));
}

#[test]
fn exec_nested_if_while() {
    // var %result = 0
    // var %i = 0
    // while (%i < 10) {
    //   set %i (%i + 1)
    //   if (%i % 3 == 0) {
    //     set %result (%result + %i)
    //   }
    // }
    // result should be 3 + 6 + 9 = 18
    let stmts = vec![
        var_decl("result", false, int_expr(0)),
        var_decl("i", false, int_expr(0)),
        while_stmt(
            binary(var_expr("i"), ast::BinaryOp::Lt, int_expr(10)),
            vec![
                set_stmt(
                    "i",
                    false,
                    binary(var_expr("i"), ast::BinaryOp::Add, int_expr(1)),
                ),
                if_stmt(
                    binary(
                        binary(var_expr("i"), ast::BinaryOp::Mod, int_expr(3)),
                        ast::BinaryOp::Eq,
                        int_expr(0),
                    ),
                    vec![set_stmt(
                        "result",
                        false,
                        binary(var_expr("result"), ast::BinaryOp::Add, var_expr("i")),
                    )],
                    vec![],
                    None,
                ),
            ],
        ),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    let mut interp = Interpreter::new(&mut env, &mut handler);
    interp.exec_stmts(&stmts).unwrap();
    assert_eq!(env.get_local("result"), Some(Value::Int(18)));
}

#[test]
fn exec_expr_statement_discards_value() {
    // An expression statement evaluates but discards its result
    let stmts = vec![expr_stmt(int_expr(42))];
    let result = exec(&stmts).unwrap();
    assert_eq!(result, Value::Null);
}

#[test]
fn exec_global_variable_roundtrip() {
    let stmts = vec![
        var_decl("counter", true, int_expr(0)),
        set_stmt("counter", true, int_expr(42)),
    ];
    let mut env = Environment::new();
    let mut handler = TestCommandHandler::new();
    {
        let mut interp = Interpreter::new(&mut env, &mut handler);
        interp.exec_stmts(&stmts).unwrap();
    }
    assert_eq!(env.get_global("counter"), Some(Value::Int(42)));

    // Verify we can read it back through the interpreter
    let mut handler2 = TestCommandHandler::new();
    let mut interp2 = Interpreter::new(&mut env, &mut handler2);
    let result = interp2.eval_expr(&global_var_expr("counter")).unwrap();
    assert_eq!(result, Value::Int(42));
}
