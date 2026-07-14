/// Aperture macro definition.
/// Ported from KiCad aperture_macro.h / aperture_macro.cpp
///
/// An aperture macro defines a complex shape as a list of aperture primitives.
/// Each primitive defines a simple shape (circle, rect, regular polygon...)
/// Parameters can be immediate or deferred (defined when the macro is instanced by ADD).
use std::collections::BTreeMap;

use crate::am_param::{AmParam, AmParams};
use crate::am_primitive::{AmPrimitive, AmPrimitiveId};
use crate::geometry::{PolySet, Vec2I, add};

/// An aperture macro definition containing a list of primitives and local parameters.
#[derive(Clone, Debug)]
pub struct ApertureMacro {
    /// The name of the aperture macro (e.g., "VB_RECTANGLE")
    pub am_name: String,

    /// List of primitives defining the shape
    pub primitives_list: Vec<AmPrimitive>,

    /// Local parameter definitions ($4=$3/2, etc.)
    pub local_param_stack: AmParams,

    /// Current values of local parameters after evaluation.
    /// Key is the param id (from $n), value is the evaluated double.
    pub local_param_values: BTreeMap<i32, f64>,

    /// Current level of local param values evaluation.
    /// When a primitive is evaluated, if its local_param_level is smaller than
    /// param_level_eval, all local params must be evaluated from current param_level_eval
    /// up to local_param_level before use in this primitive.
    pub param_level_eval: i32,

    /// Cached macro shape.
    pub shape: PolySet,
}

impl ApertureMacro {
    pub fn new() -> Self {
        Self {
            am_name: String::new(),
            primitives_list: Vec::new(),
            local_param_stack: AmParams::new(),
            local_param_values: BTreeMap::new(),
            param_level_eval: 0,
            shape: PolySet::new(),
        }
    }

    /// Add a new primitive to the list
    pub fn add_primitive_to_list(&mut self, mut primitive: AmPrimitive) {
        primitive.local_param_level = self.local_param_stack.len() as i32;
        self.primitives_list.push(primitive);
    }

    /// Add a local parameter definition to the stack
    pub fn add_local_param_def_to_stack(&mut self) {
        self.local_param_stack.push(AmParam::new());
    }

    /// Get the last local parameter definition from the stack
    pub fn get_last_local_param_def_from_stack(&mut self) -> Option<&mut AmParam> {
        self.local_param_stack.last_mut()
    }

    /// Initialize m_local_param_values from the D_CODE's deferred parameters.
    /// Must be called once before trying to build the aperture macro shape.
    pub fn init_local_params(&mut self, dcode_params: &[f64]) {
        self.param_level_eval = 0;
        self.local_param_values.clear();

        // Store the D_CODE params as $1, $2, $3, etc.
        for (i, &val) in dcode_params.iter().enumerate() {
            self.local_param_values.insert((i + 1) as i32, val);
        }
    }

    /// Evaluate local parameters from current param_level_eval to aPrimitive's local_param_level.
    /// If param_level_eval >= local_param_level, does nothing.
    pub fn eval_local_params(&mut self, primitive: &AmPrimitive) {
        if self.param_level_eval >= primitive.local_param_level {
            return;
        }

        let start = self.param_level_eval.max(0) as usize;
        let end = primitive.local_param_level.max(0) as usize;

        for idx in start..end.min(self.local_param_stack.len()) {
            let param = self.local_param_stack[idx].clone();
            if param.index > 0 {
                let value = param.get_value_from_macro(&self.get_param_values_as_vec());
                self.local_param_values.insert(param.index, value);
            }
        }

        self.param_level_eval = primitive.local_param_level;
    }

    /// Get a local parameter value by index
    pub fn get_local_param_value(&self, index: i32) -> f64 {
        self.local_param_values.get(&index).copied().unwrap_or(0.0)
    }

    /// Build this macro's polygon shape at a flash position.
    ///
    /// Ported from KiCad `APERTURE_MACRO::GetApertureMacroShape`, with the caller
    /// providing the item transform as a closure equivalent to `GetABPosition`.
    pub fn get_aperture_macro_shape<F>(
        &mut self,
        dcode_params: &[f64],
        shape_pos: Vec2I,
        mut transform: F,
    ) -> PolySet
    where
        F: FnMut(Vec2I) -> Vec2I,
    {
        let mut hole_buffer = PolySet::new();
        self.shape.remove_all_contours();
        self.init_local_params(dcode_params);

        for primitive in self.primitives_list.clone() {
            if primitive.primitive_id == AmPrimitiveId::Comment {
                continue;
            }

            let macro_params = self.get_param_values_as_vec();

            if primitive.is_exposure_on(&macro_params, true) {
                self.eval_local_params(&primitive);
                let macro_params = self.get_param_values_as_vec();
                primitive.convert_basic_shape_to_polygon(&macro_params, &mut self.shape);
            } else {
                self.eval_local_params(&primitive);
                let macro_params = self.get_param_values_as_vec();
                primitive.convert_basic_shape_to_polygon(&macro_params, &mut hole_buffer);

                if hole_buffer.outline_count() > 0 {
                    self.shape.boolean_subtract(&hole_buffer);
                    hole_buffer.remove_all_contours();
                }
            }
        }

        self.shape.simplify();
        self.shape.fracture();

        for poly in &mut self.shape.polygons {
            for point in &mut poly.outline {
                *point = transform(add(*point, shape_pos));
            }

            for hole in &mut poly.holes {
                for point in hole {
                    *point = transform(add(*point, shape_pos));
                }
            }
        }

        self.shape.clone()
    }

    /// Get the local param values as a Vec for use with get_value_from_macro
    fn get_param_values_as_vec(&self) -> Vec<f64> {
        let max_key = self.local_param_values.keys().max().copied().unwrap_or(0);
        let mut result = vec![0.0; max_key as usize];
        for (&key, &val) in &self.local_param_values {
            if key >= 1 && key as usize <= result.len() {
                result[(key - 1) as usize] = val;
            }
        }
        result
    }
}

/// Comparison function for sorting aperture macros by name
pub fn aperture_macro_less_than(am1: &ApertureMacro, am2: &ApertureMacro) -> std::cmp::Ordering {
    am1.am_name.cmp(&am2.am_name)
}

/// A sorted collection of APERTURE_MACROS whose key is the name field.
pub type ApertureMacroSet = BTreeMap<String, ApertureMacro>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::am_param::{AmParamItem, ParmItemType};

    fn value_param(value: f64) -> AmParam {
        AmParam {
            index: 0,
            param_stack: vec![AmParamItem {
                item_type: ParmItemType::PushValue,
                dvalue: value,
                ivalue: 0,
            }],
        }
    }

    fn deferred_param(index: i32) -> AmParam {
        AmParam {
            index: 0,
            param_stack: vec![AmParamItem {
                item_type: ParmItemType::PushParm,
                dvalue: 0.0,
                ivalue: index,
            }],
        }
    }

    #[test]
    fn add_primitive_captures_current_local_param_level() {
        let mut aperture_macro = ApertureMacro::new();
        aperture_macro.add_local_param_def_to_stack();

        let primitive = AmPrimitive::new(true, AmPrimitiveId::Circle);
        aperture_macro.add_primitive_to_list(primitive);

        assert_eq!(aperture_macro.primitives_list[0].local_param_level, 1);
    }

    #[test]
    fn exposure_is_checked_before_local_params_are_evaluated_like_kicad() {
        let mut aperture_macro = ApertureMacro::new();
        let mut local = AmParam::new();
        local.index = 2;
        local.param_stack = vec![AmParamItem {
            item_type: ParmItemType::PushParm,
            dvalue: 0.0,
            ivalue: 1,
        }];
        aperture_macro.local_param_stack.push(local);

        let mut primitive = AmPrimitive::new(true, AmPrimitiveId::Circle);
        primitive.params = vec![
            deferred_param(2),
            value_param(1.0),
            value_param(0.0),
            value_param(0.0),
        ];
        aperture_macro.add_primitive_to_list(primitive);

        let shape =
            aperture_macro.get_aperture_macro_shape(&[1.0], Vec2I::new(0, 0), |point| point);

        assert_eq!(shape.outline_count(), 0);
        assert_eq!(aperture_macro.get_local_param_value(2), 1.0);
    }
}
