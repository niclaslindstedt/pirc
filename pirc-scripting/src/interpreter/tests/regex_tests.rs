use super::helpers::*;
use crate::interpreter::functions::RegexState;
use crate::interpreter::Value;

#[test]
fn fn_regex_match() {
    let regex_state = RegexState::new();
    let result = eval_with_regex(
        &func_call("regex", vec![str_expr("hello123"), str_expr(r"\d+")]),
        &regex_state,
    )
    .unwrap();
    assert_eq!(result, Value::Int(1));
}

#[test]
fn fn_regex_no_match() {
    let regex_state = RegexState::new();
    let result = eval_with_regex(
        &func_call("regex", vec![str_expr("hello"), str_expr(r"\d+")]),
        &regex_state,
    )
    .unwrap();
    assert_eq!(result, Value::Int(0));
}

#[test]
fn fn_regex_captures() {
    let regex_state = RegexState::new();
    let _ = eval_with_regex(
        &func_call(
            "regex",
            vec![str_expr("user123"), str_expr(r"(\w+?)(\d+)")],
        ),
        &regex_state,
    )
    .unwrap();

    // $regml(0) = full match
    let r0 = eval_with_regex(
        &func_call("regml", vec![int_expr(0)]),
        &regex_state,
    )
    .unwrap();
    assert_eq!(r0, Value::String("user123".to_string()));

    // $regml(1) = first capture group
    let r1 = eval_with_regex(
        &func_call("regml", vec![int_expr(1)]),
        &regex_state,
    )
    .unwrap();
    assert_eq!(r1, Value::String("user".to_string()));

    // $regml(2) = second capture group
    let r2 = eval_with_regex(
        &func_call("regml", vec![int_expr(2)]),
        &regex_state,
    )
    .unwrap();
    assert_eq!(r2, Value::String("123".to_string()));
}

#[test]
fn fn_regml_out_of_range() {
    let regex_state = RegexState::new();
    let result = eval_with_regex(
        &func_call("regml", vec![int_expr(99)]),
        &regex_state,
    )
    .unwrap();
    assert_eq!(result, Value::Null);
}

#[test]
fn fn_regex_invalid_pattern() {
    let regex_state = RegexState::new();
    let result = eval_with_regex(
        &func_call("regex", vec![str_expr("test"), str_expr("[invalid")]),
        &regex_state,
    );
    assert!(result.is_err());
}
