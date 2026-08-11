use super::*;
use crate::harness::vals::*;
use kdl_script::types::{
    AliasTy, ArrayTy, CArithmeticTy, PrimitiveTy, RefTy, RustArithmeticTy, Ty, TyIdx,
};
use std::fmt::Write;

impl RustcToolchain {
    pub fn init_leaf_value(
        &self,
        f: &mut Fivemat,
        state: &TestState,
        ty: TyIdx,
        val: &Value,
        alias: Option<&str>,
    ) -> Result<(), GenerateError> {
        match state.types.realize_ty(ty) {
            // Primitives are the only "real" values with actual bytes that advance val_idx
            Ty::Primitive(prim) => match prim {
                PrimitiveTy::RustArithmeticTy(rust_arith_ty) => match rust_arith_ty {
                    RustArithmeticTy::I8 => write!(f, "{}i8", val.generate_i8())?,
                    RustArithmeticTy::I16 => write!(f, "{}i16", val.generate_i16())?,
                    RustArithmeticTy::I32 => write!(f, "{}i32", val.generate_i32())?,
                    RustArithmeticTy::I64 => write!(f, "{}i64", val.generate_i64())?,
                    RustArithmeticTy::I128 => write!(f, "{}i128", val.generate_i128())?,
                    RustArithmeticTy::U8 => write!(f, "{}u8", val.generate_u8())?,
                    RustArithmeticTy::U16 => write!(f, "{}u16", val.generate_u16())?,
                    RustArithmeticTy::U32 => write!(f, "{}u32", val.generate_u32())?,
                    RustArithmeticTy::U64 => write!(f, "{}u64", val.generate_u64())?,
                    RustArithmeticTy::U128 => write!(f, "{}u128", val.generate_u128())?,

                    RustArithmeticTy::F32 => write!(f, "f32::from_bits({})", val.generate_u32())?,
                    RustArithmeticTy::F64 => write!(f, "f64::from_bits({})", val.generate_u64())?,
                    RustArithmeticTy::I256 => {
                        Err(UnsupportedError::Other("rust doesn't have i256".to_owned()))?
                    }
                    RustArithmeticTy::U256 => {
                        Err(UnsupportedError::Other("rust doesn't have u256".to_owned()))?
                    }
                    RustArithmeticTy::F16 => write!(f, "f16::from_bits({})", val.generate_u16())?,
                    RustArithmeticTy::F16b => Err(UnsupportedError::Other(
                        "rust f16b support isn't implemented yet".to_owned(),
                    ))?,
                    RustArithmeticTy::F128 => {
                        write!(f, "f128::from_bits({})", val.generate_u128())?
                    }
                },
                PrimitiveTy::Bool => write!(f, "{}", val.generate_bool())?,
                PrimitiveTy::Ptr => {
                    if true {
                        write!(f, "{:#X}u64 as *mut ()", val.generate_u64())?
                    } else {
                        write!(f, "{:#X}u32 as *mut ()", val.generate_u32())?
                    }
                }

                PrimitiveTy::CArithmeticTy(c_arith_ty) => match c_arith_ty {
                    // Use an as-cast to truncate the value if it is too big for the target.
                    CArithmeticTy::Char => {
                        write!(f, "({}u8 as core::ffi::c_char)", val.generate_u8())?
                    }
                    CArithmeticTy::SignedChar => {
                        write!(f, "({}u8 as core::ffi::c_schar)", val.generate_u8())?
                    }
                    CArithmeticTy::UnsignedChar => {
                        write!(f, "({}u8 as core::ffi::c_uchar)", val.generate_u8())?
                    }
                    CArithmeticTy::Short => {
                        write!(f, "({}u16 as core::ffi::c_short)", val.generate_u16())?
                    }
                    CArithmeticTy::UnsignedShort => {
                        write!(f, "({}u16 as core::ffi::c_ushort)", val.generate_u16())?
                    }
                    CArithmeticTy::Int => {
                        write!(f, "({}u32 as core::ffi::c_int)", val.generate_u32())?
                    }
                    CArithmeticTy::UnsignedInt => {
                        write!(f, "({}u32 as core::ffi::c_uint)", val.generate_u32())?
                    }
                    CArithmeticTy::Long => {
                        write!(f, "({}u64 as core::ffi::c_long)", val.generate_u64())?
                    }
                    CArithmeticTy::UnsignedLong => {
                        write!(f, "({}u64 as core::ffi::c_ulong)", val.generate_u64())?
                    }
                    CArithmeticTy::LongLong => {
                        write!(f, "({}u64 as core::ffi::c_longlong)", val.generate_u64())?
                    }
                    CArithmeticTy::UnsignedLongLong => {
                        write!(f, "({}u64 as core::ffi::c_ulonglong)", val.generate_u64())?
                    }

                    CArithmeticTy::Float => {
                        write!(f, "core::ffi::c_float::from_bits({})", val.generate_u32())?
                    }
                    CArithmeticTy::Double => {
                        if self.platform_info.target.target_arch == platforms::Arch::Avr {
                            write!(f, "core::ffi::c_double::from_bits({})", val.generate_u32())?
                        } else {
                            write!(f, "core::ffi::c_double::from_bits({})", val.generate_u64())?
                        }
                    }
                    CArithmeticTy::LongDouble => {
                        // FIXME: use core::ffi::c_longdouble once it exists.
                        return Err(UnsupportedError::Other(
                            "rust doesn't have c_longdouble".to_owned(),
                        ))?;
                    }
                },
            },
            Ty::Enum(enum_ty) => {
                let name = alias.unwrap_or(&enum_ty.name);
                if let Some(variant) = val.select_val(&enum_ty.variants) {
                    let variant_name = &variant.name;
                    write!(f, "{name}::{variant_name}")?;
                }
            }
            _ => unreachable!("only primitives and enums should be passed to generate_leaf_value"),
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn init_value(
        &self,
        f: &mut Fivemat,
        state: &TestState,
        ty: TyIdx,
        vals: &mut ArgValuesIter,
        alias: Option<&str>,
        ref_temp_name: &str,
        extra_decls: &mut Vec<String>,
    ) -> Result<(), GenerateError> {
        match state.types.realize_ty(ty) {
            // Primitives and Enums are the only "real" values with actual bytes
            Ty::Primitive(_) | Ty::Enum(_) => {
                let val = vals.next_val();
                self.init_leaf_value(f, state, ty, &val, alias)?;
            }
            Ty::Empty => {
                write!(f, "()")?;
            }
            Ty::Ref(RefTy { pointee_ty }) => {
                // The value is a mutable reference to a temporary
                write!(f, "&mut {ref_temp_name}")?;
                // Now do the rest of the recursion on constructing the temporary
                let mut ref_temp = String::new();
                let mut ref_temp_f = Fivemat::new(&mut ref_temp, INDENT);
                write!(&mut ref_temp_f, "let mut {ref_temp_name} = ")?;
                let ref_temp_name = format!("{ref_temp_name}_");
                self.init_value(
                    &mut ref_temp_f,
                    state,
                    *pointee_ty,
                    vals,
                    alias,
                    &ref_temp_name,
                    extra_decls,
                )?;
                write!(&mut ref_temp_f, ";")?;
                extra_decls.push(ref_temp);
            }
            Ty::Array(ArrayTy { elem_ty, len }) => {
                write!(f, "[")?;
                for arr_idx in 0..*len {
                    if arr_idx > 0 {
                        write!(f, ", ")?;
                    }
                    let ref_temp_name = format!("{ref_temp_name}{arr_idx}_");
                    self.init_value(f, state, *elem_ty, vals, alias, &ref_temp_name, extra_decls)?;
                }
                write!(f, "]")?;
            }
            // Nominal types we need to emit a decl for
            Ty::Struct(struct_ty) => {
                let name = alias.unwrap_or(&struct_ty.name);
                write!(f, "{name} {{ ")?;
                for (field_idx, field) in struct_ty.fields.iter().enumerate() {
                    if field_idx > 0 {
                        write!(f, ", ")?;
                    }
                    let field_name = &field.ident;
                    write!(f, "{field_name}: ")?;
                    let ref_temp_name = format!("{ref_temp_name}{field_name}_");
                    self.init_value(f, state, field.ty, vals, alias, &ref_temp_name, extra_decls)?;
                }
                write!(f, " }}")?;
            }
            Ty::Union(union_ty) => {
                let name = alias.unwrap_or(&union_ty.name);
                write!(f, "{name} {{ ")?;
                let tag_val = vals.next_val();
                if let Some(field) = tag_val.select_val(&union_ty.fields) {
                    let field_name = &field.ident;
                    write!(f, "{field_name}: ")?;
                    let ref_temp_name = format!("{ref_temp_name}{field_name}_");
                    self.init_value(f, state, field.ty, vals, alias, &ref_temp_name, extra_decls)?;
                }
                write!(f, " }}")?;
            }

            Ty::Tagged(tagged_ty) => {
                let name = alias.unwrap_or(&tagged_ty.name);
                let tag_val = vals.next_val();
                if let Some(variant) = tag_val.select_val(&tagged_ty.variants) {
                    let variant_name = &variant.name;
                    write!(f, "{name}::{variant_name}")?;
                    if let Some(fields) = &variant.fields {
                        write!(f, " {{ ")?;
                        for (field_idx, field) in fields.iter().enumerate() {
                            if field_idx > 0 {
                                write!(f, ", ")?;
                            }
                            let field_name = &field.ident;
                            write!(f, "{field_name}: ")?;
                            let ref_temp_name = format!("{ref_temp_name}{field_name}_");
                            self.init_value(
                                f,
                                state,
                                field.ty,
                                vals,
                                alias,
                                &ref_temp_name,
                                extra_decls,
                            )?;
                        }
                        write!(f, " }}")?;
                    }
                }
            }
            Ty::Alias(AliasTy { real, name, .. }) => {
                let alias = alias.or_else(|| Some(name));
                self.init_value(f, state, *real, vals, alias, ref_temp_name, extra_decls)?;
            }

            // Puns should be evaporated
            Ty::Pun(pun) => {
                let real_ty = state.types.resolve_pun(pun, &state.env).unwrap();
                self.init_value(f, state, real_ty, vals, alias, ref_temp_name, extra_decls)?;
            }
        };

        Ok(())
    }

    pub fn init_var(
        &self,
        f: &mut Fivemat,
        state: &TestState,
        var_name: &str,
        var_ty: TyIdx,
        mut vals: ArgValuesIter,
    ) -> Result<(), GenerateError> {
        // Generate the input
        let needs_mut = false;
        let let_mut = if needs_mut { "let mut" } else { "let" };
        let mut real_var_decl = String::new();
        let mut real_var_decl_f = Fivemat::new(&mut real_var_decl, INDENT);
        let mut extra_decls = Vec::new();
        write!(&mut real_var_decl_f, "{let_mut} {var_name} = ")?;
        let ref_temp_name = format!("{var_name}_");
        self.init_value(
            &mut real_var_decl_f,
            state,
            var_ty,
            &mut vals,
            None,
            &ref_temp_name,
            &mut extra_decls,
        )?;
        writeln!(&mut real_var_decl, ";")?;

        for decl in extra_decls {
            writeln!(f, "{}", decl)?;
        }
        writeln!(f, "{}", real_var_decl)?;
        Ok(())
    }
}
