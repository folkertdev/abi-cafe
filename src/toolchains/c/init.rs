use super::*;
use crate::harness::vals::{ArgValuesIter, Value};
use kdl_script::types::{
    AliasTy, ArrayTy, CArithmeticTy, PrimitiveTy, RefTy, RustArithmeticTy, Ty, TyIdx,
};
use std::fmt::Write;

impl CcToolchain {
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
                PrimitiveTy::Bool => write!(f, "{}", val.generate_bool())?,

                PrimitiveTy::Ptr => {
                    if true {
                        write!(f, "(void*){:#X}ull", val.generate_u64())?
                    } else {
                        write!(f, "(void*){:#X}ul", val.generate_u32())?
                    }
                }

                PrimitiveTy::RustArithmeticTy(rust_arith_ty) => match rust_arith_ty {
                    RustArithmeticTy::I8 => write!(f, "{}", val.generate_i8())?,
                    RustArithmeticTy::I16 => write!(f, "{}", val.generate_i16())?,
                    RustArithmeticTy::I32 => write!(f, "{}", val.generate_i32())?,
                    RustArithmeticTy::I64 => write!(f, "{}", val.generate_i64())?,
                    RustArithmeticTy::U8 => write!(f, "{}", val.generate_u8())?,
                    RustArithmeticTy::U16 => write!(f, "{}", val.generate_u16())?,
                    RustArithmeticTy::U32 => write!(f, "{}", val.generate_u32())?,
                    RustArithmeticTy::U64 => write!(f, "{}ull", val.generate_u64())?,
                    RustArithmeticTy::I128 => {
                        let val = val.generate_i128();
                        let lower = (val as u128) & 0x0000_0000_0000_0000_FFFF_FFFF_FFFF_FFFF;
                        let higher =
                            ((val as u128) & 0xFFFF_FFFF_FFFF_FFFF_0000_0000_0000_0000) >> 64;
                        write!(
                            f,
                            "((__int128_t){lower:#X}ull) | (((__int128_t){higher:#X}ull) << 64)"
                        )?
                    }
                    RustArithmeticTy::U128 => {
                        let val = val.generate_u128();
                        let lower = val & 0x0000_0000_0000_0000_FFFF_FFFF_FFFF_FFFF;
                        let higher = (val & 0xFFFF_FFFF_FFFF_FFFF_0000_0000_0000_0000) >> 64;
                        write!(
                            f,
                            "((__uint128_t){lower:#X}ull) | (((__uint128_t){higher:#X}ull) << 64)"
                        )?
                    }

                    RustArithmeticTy::F32 => {
                        let val = f32::from_bits(val.generate_u32());
                        match val.fract() {
                            0.0 => write!(f, "{val}.0f")?,
                            _ => write!(f, "{val}f")?,
                        }
                    }
                    RustArithmeticTy::F64 => {
                        let val = f64::from_bits(val.generate_u64());
                        match val.fract() {
                            0.0 => write!(f, "{val}.0")?,
                            _ => write!(f, "{val}")?,
                        }
                    }
                    RustArithmeticTy::I256 => {
                        Err(UnsupportedError::Other("c doesn't have i256?".to_owned()))?
                    }
                    RustArithmeticTy::U256 => {
                        Err(UnsupportedError::Other("c doesn't have u256?".to_owned()))?
                    }
                    RustArithmeticTy::F16 => write!(
                        f,
                        "(((union {{ uint16_t bits; {} value; }}){{ .bits = {} }}).value)",
                        // Pick `_Float16` or `__fp16` depending on the target.
                        state.tynames[&ty].0.trim_end(),
                        val.generate_u16()
                    )?,
                    RustArithmeticTy::F16b => write!(
                        f,
                        "(((union {{ uint16_t bits; __bf16 value; }}){{ .bits = {} }}).value)",
                        val.generate_u16()
                    )?,
                    RustArithmeticTy::F128 => {
                        // Pick `long double , `__float128` or `_Float128` depending on the target.
                        let (f128_ty_name, _) = &state.tynames[&ty];
                        let f128_ty_name = f128_ty_name.trim_end();

                        let val = val.generate_u128();
                        let lower = val & 0x0000_0000_0000_0000_FFFF_FFFF_FFFF_FFFF;
                        let higher = (val & 0xFFFF_FFFF_FFFF_FFFF_0000_0000_0000_0000) >> 64;

                        // Not using u128 here so it also works on 32-bit platforms.
                        let (first, second) = match self.platform.target_endian {
                            platforms::Endian::Big => (higher, lower),
                            platforms::Endian::Little => (lower, higher),
                            _ => unreachable!("non-exhaustive enum"),
                        };
                        write!(
                        f,
                        "(((union {{ uint64_t bits[2]; {f128_ty_name} value; }}){{ .bits = {{ {first:#X}ull, {second:#X}ull }} }}).value)"
                    )?
                    }
                },

                PrimitiveTy::CArithmeticTy(c_arith_ty) => match c_arith_ty {
                    // Explicitly narrow the types (relevant on some targets).
                    CArithmeticTy::Char => write!(f, "((char){})", val.generate_u8())?,
                    CArithmeticTy::SignedChar => write!(f, "((signed char){})", val.generate_u8())?,
                    CArithmeticTy::UnsignedChar => {
                        write!(f, "((unsigned char){})", val.generate_u8())?
                    }
                    CArithmeticTy::Short => write!(f, "((short){})", val.generate_u16())?,
                    CArithmeticTy::UnsignedShort => {
                        write!(f, "((unsigned short){})", val.generate_u16())?
                    }
                    CArithmeticTy::Int => write!(f, "((int){}u)", val.generate_u32())?,
                    CArithmeticTy::UnsignedInt => {
                        write!(f, "((unsigned int){}u)", val.generate_u32())?
                    }
                    CArithmeticTy::Long => write!(f, "((long){}ull)", val.generate_u64())?,
                    CArithmeticTy::UnsignedLong => {
                        write!(f, "((unsigned long){}ull)", val.generate_u64())?
                    }
                    CArithmeticTy::LongLong => write!(f, "((long long){}ull)", val.generate_u64())?,
                    CArithmeticTy::UnsignedLongLong => {
                        write!(f, "((unsigned long long){}ull)", val.generate_u64())?
                    }

                    // Use a union as an ad-hoc bitcast.
                    CArithmeticTy::Float => write!(
                        f,
                        "(((union {{ uint32_t bits; float value; }}){{ .bits = {:#X}u }}).value)",
                        val.generate_u32()
                    )?,
                    CArithmeticTy::Double => {
                        if self.platform.target_arch == Arch::Avr {
                            write!(
                            f,
                            "(((union {{ uint32_t bits; double value; }}){{ .bits = {:#X}u }}).value)",
                            val.generate_u32()
                        )?
                        } else {
                            write!(
                            f,
                            "(((union {{ uint64_t bits; double value; }}){{ .bits = {:#X}ull }}).value)",
                            val.generate_u64()
                        )?
                        }
                    }
                    CArithmeticTy::LongDouble => {
                        let val = val.generate_u128();
                        let lower = val & 0x0000_0000_0000_0000_FFFF_FFFF_FFFF_FFFF;
                        let higher = (val & 0xFFFF_FFFF_FFFF_FFFF_0000_0000_0000_0000) >> 64;

                        // Not using u128 here so it also works on 32-bit platforms.
                        let (first, second) = match self.platform.target_endian {
                            platforms::Endian::Big => (higher, lower),
                            platforms::Endian::Little => (lower, higher),
                            _ => unreachable!("non-exhaustive enum"),
                        };
                        write!(
                        f,
                        "(((union {{ uint64_t bits[2]; long double value; }}){{ .bits = {{ {first:#X}ull, {second:#X}ull }} }}).value)"
                    )?
                    }
                },

                PrimitiveTy::Complex(c_arith_ty) => {
                    let (re, im) = val.generate_complex();
                    let base = match c_arith_ty {
                        CArithmeticTy::Float => {
                            let lit = |bits: u32| {
                                let val = f32::from_bits(bits);
                                match val.fract() {
                                    0.0 => format!("{val}.0f"),
                                    _ => format!("{val}f"),
                                }
                            };
                            let (re, im) = (lit(re as u32), lit(im as u32));
                            return Ok(write!(f, "({re} + {im}i)")?);
                        }
                        CArithmeticTy::Double | CArithmeticTy::LongDouble => {
                            let suffix = match c_arith_ty {
                                CArithmeticTy::LongDouble => "L",
                                _ => "",
                            };
                            let lit = |bits: u64| {
                                let val = f64::from_bits(bits);
                                match val.fract() {
                                    0.0 => format!("{val}.0{suffix}"),
                                    _ => format!("{val}{suffix}"),
                                }
                            };
                            let (re, im) = (lit(re as u64), lit(im as u64));
                            return Ok(write!(f, "({re} + {im}i)")?);
                        }
                        CArithmeticTy::Char => "char",
                        CArithmeticTy::SignedChar => "signed char",
                        CArithmeticTy::UnsignedChar => "unsigned char",
                        CArithmeticTy::Short => "short",
                        CArithmeticTy::UnsignedShort => "unsigned short",
                        CArithmeticTy::Int => "int",
                        CArithmeticTy::UnsignedInt => "unsigned int",
                        CArithmeticTy::Long => "long",
                        CArithmeticTy::UnsignedLong => "unsigned long",
                        CArithmeticTy::LongLong => "long long",
                        CArithmeticTy::UnsignedLongLong => "unsigned long long",
                    };
                    let (re, im) = (re as u64, im as u64);
                    write!(f, "(_Complex {base})({re:#X}ull + {im:#X}ulli)")?
                }
            },
            Ty::Enum(enum_ty) => {
                let name = alias.unwrap_or(&enum_ty.name);
                if let Some(variant) = val.select_val(&enum_ty.variants) {
                    let variant_name = &variant.name;
                    write!(f, "{name}_{variant_name}")?;
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
            Ty::Ref(RefTy { pointee_ty }) => {
                // The value is a mutable reference to a temporary
                write!(f, "&{ref_temp_name}")?;

                // Now do the rest of the recursion on constructing the temporary
                let mut ref_temp = String::new();
                let mut ref_temp_f = Fivemat::new(&mut ref_temp, INDENT);
                let (pre, post) = &state.tynames[pointee_ty];
                write!(&mut ref_temp_f, "{pre}{ref_temp_name}{post} = ")?;
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
                write!(f, "{{")?;
                for arr_idx in 0..*len {
                    if arr_idx > 0 {
                        write!(f, ", ")?;
                    }
                    let ref_temp_name = format!("{ref_temp_name}{arr_idx}_");
                    self.init_value(f, state, *elem_ty, vals, alias, &ref_temp_name, extra_decls)?;
                }
                write!(f, "}}")?;
            }
            // Nominal types we need to emit a decl for
            Ty::Struct(struct_ty) => {
                write!(f, "{{ ")?;
                for (field_idx, field) in struct_ty.fields.iter().enumerate() {
                    if field_idx > 0 {
                        write!(f, ", ")?;
                    }
                    let field_name = &field.ident;
                    write!(f, ".{field_name} = ")?;
                    let ref_temp_name = format!("{ref_temp_name}{field_name}_");
                    self.init_value(f, state, field.ty, vals, alias, &ref_temp_name, extra_decls)?;
                }
                write!(f, " }}")?;
            }
            Ty::Union(union_ty) => {
                write!(f, "{{ ")?;
                let tag_val = vals.next_val();
                if let Some(field) = tag_val.select_val(&union_ty.fields) {
                    let field_name = &field.ident;
                    write!(f, ".{field_name} = ")?;
                    let ref_temp_name = format!("{ref_temp_name}{field_name}_");
                    self.init_value(f, state, field.ty, vals, alias, &ref_temp_name, extra_decls)?;
                }
                write!(f, " }}")?;
            }

            Ty::Tagged(_tagged_ty) => {
                return Err(UnsupportedError::Other(
                    "c doesn't have tagged unions impled yet".to_owned(),
                ))?;
                /*
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
                 */
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

            Ty::Empty => {
                return Err(UnsupportedError::Other(
                    "c doesn't have empty tuples".to_owned(),
                ))?
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
        let mut real_var_decl = String::new();
        let mut real_var_decl_f = Fivemat::new(&mut real_var_decl, INDENT);
        let mut extra_decls = Vec::new();
        let (pre, post) = &state.tynames[&var_ty];
        write!(&mut real_var_decl_f, "{pre}{var_name}{post} = ")?;
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
