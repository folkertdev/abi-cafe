use super::*;
use crate::harness::vals::*;
use kdl_script::types::{AliasTy, ArrayTy, ComplexTy, PrimitiveTy, RefTy, Ty, TyIdx};
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
                PrimitiveTy::I8 => write!(f, "{}i8", val.generate_i8())?,
                PrimitiveTy::I16 => write!(f, "{}i16", val.generate_i16())?,
                PrimitiveTy::I32 => write!(f, "{}i32", val.generate_i32())?,
                PrimitiveTy::I64 => write!(f, "{}i64", val.generate_i64())?,
                PrimitiveTy::I128 => write!(f, "{}i128", val.generate_i128())?,
                PrimitiveTy::U8 => write!(f, "{}u8", val.generate_u8())?,
                PrimitiveTy::U16 => write!(f, "{}u16", val.generate_u16())?,
                PrimitiveTy::U32 => write!(f, "{}u32", val.generate_u32())?,
                PrimitiveTy::U64 => write!(f, "{}u64", val.generate_u64())?,
                PrimitiveTy::U128 => write!(f, "{}u128", val.generate_u128())?,

                PrimitiveTy::F32 => write!(f, "f32::from_bits({})", val.generate_u32())?,
                PrimitiveTy::F64 => write!(f, "f64::from_bits({})", val.generate_u64())?,
                PrimitiveTy::Bool => write!(f, "{}", val.generate_bool())?,
                PrimitiveTy::Ptr => {
                    if true {
                        write!(f, "{:#X}u64 as *mut ()", val.generate_u64())?
                    } else {
                        write!(f, "{:#X}u32 as *mut ()", val.generate_u32())?
                    }
                }
                PrimitiveTy::I256 => {
                    Err(UnsupportedError::Other("rust doesn't have i256".to_owned()))?
                }
                PrimitiveTy::U256 => {
                    Err(UnsupportedError::Other("rust doesn't have u256".to_owned()))?
                }
                PrimitiveTy::F16 => write!(f, "f16::from_bits({})", val.generate_u16())?,
                PrimitiveTy::F128 => write!(f, "f128::from_bits({})", val.generate_u128())?,
                PrimitiveTy::Complex(complex) => {
                    let (re, im) = val.generate_complex();
                    // How wide the base type is is the target's business, so cast the
                    // bits down to it and let it truncate, exactly like the c side does
                    let ints = |base: &str| {
                        (
                            format!("{re:#X}u128 as {base}"),
                            format!("{im:#X}u128 as {base}"),
                        )
                    };
                    let (re, im) = match complex {
                        ComplexTy::Float => (
                            format!("f32::from_bits({}u32)", re as u32),
                            format!("f32::from_bits({}u32)", im as u32),
                        ),
                        ComplexTy::Double => (
                            format!("f64::from_bits({}u64)", re as u64),
                            format!("f64::from_bits({}u64)", im as u64),
                        ),
                        ComplexTy::Char => ints("core::ffi::c_char"),
                        ComplexTy::SChar => ints("core::ffi::c_schar"),
                        ComplexTy::UChar => ints("core::ffi::c_uchar"),
                        ComplexTy::Short => ints("core::ffi::c_short"),
                        ComplexTy::UShort => ints("core::ffi::c_ushort"),
                        ComplexTy::Int => ints("core::ffi::c_int"),
                        ComplexTy::UInt => ints("core::ffi::c_uint"),
                        ComplexTy::Long => ints("core::ffi::c_long"),
                        ComplexTy::ULong => ints("core::ffi::c_ulong"),
                        ComplexTy::LongLong => ints("core::ffi::c_longlong"),
                        ComplexTy::ULongLong => ints("core::ffi::c_ulonglong"),
                        ComplexTy::LongDouble => Err(UnsupportedError::Other(
                            "rust doesn't have c_longdouble yet".to_owned(),
                        ))?,
                    };
                    write!(f, "core::num::Complex::new({re}, {im})")?
                }
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
