/// Aperture macro parameter evaluation.
/// Ported from KiCad am_param.h / am_param.cpp
///
/// Parameters can be:
/// - Immediate values: 3.5
/// - Deferred values: $2 (replace with value from ADD command)
/// - Arithmetic expressions: $2/2+1
use crate::coord::{read_double, read_int};

/// Operator/operand types for the stack-based evaluation machine
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ParmItemType {
    #[default]
    Nop,
    PushValue,
    PushParm,
    Add,
    Sub,
    Mul,
    Div,
    OpenPar,
    ClosePar,
    PopValue,
}

impl ParmItemType {
    pub fn priority(self) -> i32 {
        match self {
            Self::Add | Self::Sub => 1,
            Self::Mul | Self::Div => 2,
            Self::OpenPar | Self::ClosePar => 3,
            _ => 0,
        }
    }

    pub fn is_operator(self) -> bool {
        matches!(self, Self::Add | Self::Sub | Self::Mul | Self::Div)
    }
}

/// A single item in the parameter evaluation stack
#[derive(Clone, Debug)]
pub struct AmParamItem {
    pub item_type: ParmItemType,
    /// Value for PushValue type
    pub dvalue: f64,
    /// Integer value for PushParm type (parameter index)
    pub ivalue: i32,
}

impl AmParamItem {
    pub fn new_operator(item_type: ParmItemType) -> Self {
        Self {
            item_type,
            dvalue: 0.0,
            ivalue: 0,
        }
    }

    pub fn new_value(value: f64) -> Self {
        Self {
            item_type: ParmItemType::PushValue,
            dvalue: value,
            ivalue: 0,
        }
    }

    pub fn new_param(index: i32) -> Self {
        Self {
            item_type: ParmItemType::PushParm,
            dvalue: 0.0,
            ivalue: index,
        }
    }

    pub fn is_operator(&self) -> bool {
        matches!(
            self.item_type,
            ParmItemType::Add | ParmItemType::Sub | ParmItemType::Mul | ParmItemType::Div
        )
    }

    pub fn is_operand(&self) -> bool {
        matches!(
            self.item_type,
            ParmItemType::PushValue | ParmItemType::PushParm
        )
    }

    pub fn is_deferred(&self) -> bool {
        self.item_type == ParmItemType::PushParm
    }
}

/// Hold a parameter value for an aperture macro.
///
/// The parameter can be a constant (immediate) or depend on deferred values
/// defined in a D_CODE by the ADD command. The actual value may need arithmetic
/// evaluation from expression items stored in param_stack.
#[derive(Clone, Debug, Default)]
pub struct AmParam {
    /// Index for local parameter definition ($n = ...)
    pub index: i32,

    /// List of operands/operators for evaluation.
    /// For $3/2: 3 items = PushParm(3), Div, PushValue(2)
    pub param_stack: Vec<AmParamItem>,
}

impl AmParam {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push an operator/operand onto the stack
    pub fn push_operator(&mut self, item_type: ParmItemType, value: f64) {
        self.param_stack.push(AmParamItem {
            item_type,
            dvalue: value,
            ivalue: 0,
        });
    }

    /// Push an operator/operand with integer value (for PUSHPARM)
    pub fn push_operator_int(&mut self, item_type: ParmItemType, value: i32) {
        self.param_stack.push(AmParamItem {
            item_type,
            dvalue: 0.0,
            ivalue: value,
        });
    }

    /// Test if this AM_PARAM holds an immediate parameter (no deferred values)
    pub fn is_immediate(&self) -> bool {
        for item in &self.param_stack {
            if item.is_deferred() {
                return false;
            }
        }
        true
    }

    /// Get the parameter index (for local param definitions like $n = ...)
    pub fn get_index(&self) -> i32 {
        self.index
    }

    pub fn set_index(&mut self, index: i32) {
        self.index = index;
    }

    /// Read one aperture macro parameter from the definition text.
    ///
    /// A parameter can be:
    /// - a number
    /// - a reference to an aperture definition parameter: $1 to $3
    /// - an arithmetic expression: $1+3 or $2x2
    /// Parameters are separated by comma or finish with *
    ///
    /// Returns true if a param was read, false otherwise.
    /// Advances text pointer past the parameter.
    pub fn read_param_from_am_def(text: &mut &str) -> Option<Self> {
        let mut param = Self::new();
        let mut idx = 0;
        let mut found = false;
        let mut end = false;

        while !end {
            let bytes = text.as_bytes();
            let ch = bytes.get(idx).copied().unwrap_or(0);

            match ch {
                b',' => {
                    idx += 1;

                    if !found {
                        break;
                    }

                    end = true;
                }
                b'\n' | b'\r' | 0 | b'*' => {
                    end = true;
                }
                b' ' => {
                    idx += 1;
                }
                b'$' => {
                    idx += 1;
                    let (ivalue, new_idx) = read_int(text, idx, false);
                    idx = new_idx;

                    if param.index < 1 {
                        param.set_index(ivalue);
                    }

                    param.push_operator_int(ParmItemType::PushParm, ivalue);
                    found = true;
                }
                b'/' => {
                    param.push_operator(ParmItemType::Div, 0.0);
                    idx += 1;
                }
                b'(' => {
                    param.push_operator(ParmItemType::OpenPar, 0.0);
                    idx += 1;
                }
                b')' => {
                    param.push_operator(ParmItemType::ClosePar, 0.0);
                    idx += 1;
                }
                b'x' | b'X' => {
                    param.push_operator(ParmItemType::Mul, 0.0);
                    idx += 1;
                }
                b'-' | b'+' => {
                    if !param.param_stack.is_empty()
                        && !param.param_stack.last().unwrap().is_operator()
                    {
                        param.push_operator(
                            if ch == b'+' {
                                ParmItemType::Add
                            } else {
                                ParmItemType::Sub
                            },
                            0.0,
                        );
                        idx += 1;
                    } else {
                        let (dvalue, new_idx) = read_double(text, idx, false);
                        param.push_operator(ParmItemType::PushValue, dvalue);
                        idx = if new_idx == idx { idx + 1 } else { new_idx };
                        found = true;
                    }
                }
                b'=' => {
                    idx += 1;
                    param.param_stack.clear();
                    found = false;
                }
                _ => {
                    let (dvalue, new_idx) = read_double(text, idx, false);
                    param.push_operator(ParmItemType::PushValue, dvalue);
                    idx = if new_idx == idx { idx + 1 } else { new_idx };
                    found = true;
                }
            }
        }

        *text = &text[idx..];

        if found { Some(param) } else { None }
    }

    /// Evaluate the parameter value using the given aperture macro for deferred values.
    ///
    /// Uses KiCad's infix evaluator with the expression stored in param_stack.
    pub fn get_value_from_macro(&self, macro_params: &[f64]) -> f64 {
        let mut values: Vec<f64> = Vec::new();
        let mut optype: Vec<(ParmItemType, i32)> = Vec::new();
        let mut extra_priority = 0;

        for item in &self.param_stack {
            match item.item_type {
                ParmItemType::OpenPar => extra_priority += ParmItemType::OpenPar.priority(),
                ParmItemType::ClosePar => extra_priority -= ParmItemType::ClosePar.priority(),
                ParmItemType::Add | ParmItemType::Sub | ParmItemType::Mul | ParmItemType::Div => {
                    optype.push((item.item_type, item.item_type.priority() + extra_priority));
                }
                ParmItemType::PushValue | ParmItemType::PushParm => {
                    let value = if item.item_type == ParmItemType::PushParm {
                        let idx = item.ivalue as usize;

                        if idx >= 1 && idx <= macro_params.len() {
                            macro_params[idx - 1]
                        } else {
                            0.0
                        }
                    } else {
                        item.dvalue
                    };

                    values.push(value);

                    if optype.len() >= 2 {
                        let previous_priority = optype[optype.len() - 2].1;
                        let current_priority = optype[optype.len() - 1].1;

                        if current_priority > previous_priority {
                            let op2 = values.pop().unwrap_or(0.0);
                            let op1 = values.pop().unwrap_or(0.0);
                            let (op, _) = optype.pop().unwrap();
                            values.push(apply_operator(op, op1, op2));
                        }
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

/// Type alias for parameter list
pub type AmParams = Vec<AmParam>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_immediate_value() {
        let mut text = "3.5,";
        let param = AmParam::read_param_from_am_def(&mut text).unwrap();
        assert!(param.is_immediate());
        assert_eq!(param.get_value_from_macro(&[]), 3.5);
    }

    #[test]
    fn test_deferred_param() {
        let mut text = "$2,";
        let param = AmParam::read_param_from_am_def(&mut text).unwrap();
        assert!(!param.is_immediate());
        assert_eq!(param.get_value_from_macro(&[1.0, 2.5, 3.0]), 2.5);
    }

    #[test]
    fn test_simple_addition() {
        let mut text = "$1+3,";
        let param = AmParam::read_param_from_am_def(&mut text).unwrap();
        assert_eq!(param.get_value_from_macro(&[5.0]), 8.0);
    }

    #[test]
    fn test_division() {
        let mut text = "$1/2,";
        let param = AmParam::read_param_from_am_def(&mut text).unwrap();
        assert_eq!(param.get_value_from_macro(&[10.0]), 5.0);
    }

    #[test]
    fn test_operator_precedence() {
        // $1+$2x$3 should be $1 + ($2 * $3)
        let mut text = "$1+$2x$3,";
        let param = AmParam::read_param_from_am_def(&mut text).unwrap();
        assert_eq!(param.get_value_from_macro(&[1.0, 2.0, 3.0]), 7.0);
    }

    #[test]
    fn test_parentheses() {
        // ($1+$2)x$3
        let mut text = "($1+$2)x$3,";
        let param = AmParam::read_param_from_am_def(&mut text).unwrap();
        assert_eq!(param.get_value_from_macro(&[1.0, 2.0, 3.0]), 9.0);
    }

    #[test]
    fn test_star_terminates_parameter_like_kicad() {
        let mut text = "$1*$2,";
        let param = AmParam::read_param_from_am_def(&mut text).unwrap();
        assert_eq!(param.get_value_from_macro(&[3.0, 4.0]), 3.0);
        assert_eq!(text, "*$2,");
    }

    #[test]
    fn test_local_param_definition() {
        // $4=$3/2
        let mut text = "$4=$3/2*";
        let param = AmParam::read_param_from_am_def(&mut text).unwrap();
        assert_eq!(param.get_index(), 4);
        assert_eq!(param.get_value_from_macro(&[0.0, 0.0, 10.0]), 5.0);
    }
}
