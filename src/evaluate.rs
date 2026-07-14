/// Arithmetic expression evaluator for aperture macro parameters.
/// Ported from KiCad evaluate.cpp
///
/// Implements KiCad's infix evaluator for aperture macro parameter definitions
/// (e.g., $2+1, $1-$3, $3/2).
use crate::am_param::{AmParam, ParmItemType};
use crate::aperture_macro::ApertureMacro;

/// Evaluate an AM_PARAM expression using deferred parameter values from the macro.
///
/// The param_stack contains an infix expression that can reference deferred
/// parameters via PushParm items. The macro provides the parameter values.
pub fn evaluate_am_param(param: &AmParam, aperture_macro: &ApertureMacro) -> f64 {
    let mut values: Vec<f64> = Vec::new();
    let mut optype: Vec<(ParmItemType, i32)> = Vec::new();
    let mut extra_priority = 0;

    for item in &param.param_stack {
        match item.item_type {
            ParmItemType::OpenPar => {
                extra_priority += ParmItemType::OpenPar.priority();
            }
            ParmItemType::ClosePar => {
                extra_priority -= ParmItemType::ClosePar.priority();
            }
            ParmItemType::Add | ParmItemType::Sub | ParmItemType::Mul | ParmItemType::Div => {
                optype.push((item.item_type, item.item_type.priority() + extra_priority));
            }
            ParmItemType::PushValue | ParmItemType::PushParm => {
                let value = if item.item_type == ParmItemType::PushParm {
                    aperture_macro.get_local_param_value(item.ivalue)
                } else {
                    item.dvalue
                };

                values.push(value);

                if optype.len() < 2 {
                    continue;
                }

                let previous_priority = optype[optype.len() - 2].1;
                let current_priority = optype[optype.len() - 1].1;

                if current_priority > previous_priority {
                    let op2 = values.pop().unwrap_or(0.0);
                    let op1 = values.pop().unwrap_or(0.0);
                    let (op, _) = optype.pop().unwrap();
                    values.push(apply_operator(op, op1, op2));
                }
            }
            _ => {}
        }
    }

    if values.len() > optype.len() {
        optype.insert(0, (ParmItemType::PopValue, 0));
    }

    let mut result = 0.0;

    for (idx, value) in values.iter().copied().enumerate() {
        let op = optype
            .get(idx)
            .map(|(op, _)| *op)
            .unwrap_or(ParmItemType::PopValue);

        result = apply_accumulator(op, result, value);
    }

    result
}

fn apply_operator(op: ParmItemType, op1: f64, op2: f64) -> f64 {
    match op {
        ParmItemType::Add => op1 + op2,
        ParmItemType::Sub => op1 - op2,
        ParmItemType::Mul => op1 * op2,
        ParmItemType::Div => op1 / op2,
        _ => op2,
    }
}

fn apply_accumulator(op: ParmItemType, result: f64, value: f64) -> f64 {
    match op {
        ParmItemType::PopValue => value,
        ParmItemType::Add => result + value,
        ParmItemType::Sub => result - value,
        ParmItemType::Mul => result * value,
        ParmItemType::Div => result / value,
        _ => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::am_param::AmParam;

    #[test]
    fn test_evaluate_simple_value() {
        let mut param = AmParam::new();
        param.push_operator(ParmItemType::PushValue, 42.0);
        let am = ApertureMacro::new();
        assert_eq!(evaluate_am_param(&param, &am), 42.0);
    }

    #[test]
    fn test_evaluate_addition() {
        let mut param = AmParam::new();
        param.push_operator(ParmItemType::PushValue, 3.0);
        param.push_operator(ParmItemType::Add, 0.0);
        param.push_operator(ParmItemType::PushValue, 4.0);
        let am = ApertureMacro::new();
        assert_eq!(evaluate_am_param(&param, &am), 7.0);
    }

    #[test]
    fn test_evaluate_with_deferred() {
        let mut param = AmParam::new();
        param.push_operator_int(ParmItemType::PushParm, 1);
        param.push_operator(ParmItemType::Mul, 0.0);
        param.push_operator(ParmItemType::PushValue, 2.0);

        let mut am = ApertureMacro::new();
        am.local_param_values.insert(1, 5.0);
        assert_eq!(evaluate_am_param(&param, &am), 10.0);
    }
}
