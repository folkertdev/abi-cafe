use super::*;
use kdl_script::parse::Attr;
use kdl_script::types::{
    AliasTy, ArrayTy, CArithmeticTy, FuncIdx, PrimitiveTy, RefTy, RustArithmeticTy, Ty, TyIdx,
};
use platforms::{Arch, Env, Os, PointerWidth};
use std::fmt::Write;

impl CcToolchain {
    pub fn generate_caller_externs(
        &self,
        f: &mut Fivemat,
        state: &TestState,
    ) -> Result<(), GenerateError> {
        for &func in &state.desired_funcs {
            self.generate_signature(f, state, func)?;
            writeln!(f, ";")?;
        }
        writeln!(f)?;
        Ok(())
    }

    pub fn generate_definitions(
        &self,
        f: &mut Fivemat,
        state: &mut TestState,
    ) -> Result<(), GenerateError> {
        self.write_harness_prefix(f, state)?;

        for def in state.defs.definitions(state.desired_funcs.iter().copied()) {
            match def {
                kdl_script::Definition::DeclareTy(ty) => {
                    debug!("declare ty {}", state.types.format_ty(ty));
                    self.generate_forward_decl(f, state, ty)?;
                }
                kdl_script::Definition::DefineTy(ty) => {
                    debug!("define ty {}", state.types.format_ty(ty));
                    self.generate_tydef(f, state, ty)?;
                }
                kdl_script::Definition::DefineFunc(_) => {
                    // we'd buffer these up to generate them all at the end,
                    // but we've already got them buffered, so... do nothing.
                }
                kdl_script::Definition::DeclareFunc(_) => {
                    // nothing to do, executable kdl-script isn't real and can't hurt us
                }
            }
        }

        Ok(())
    }

    pub fn intern_tyname(&self, state: &mut TestState, ty: TyIdx) -> Result<(), GenerateError> {
        // Don't double-intern
        if state.tynames.contains_key(&ty) {
            return Ok(());
        }

        let has_vsx = self.platform.target_endian == platforms::Endian::Little;

        let has_sse2 = cfg_select! {
            target_arch = "x86" => cfg!(target_feature = "sse2"),
            target_arch = "x86_64" => true,
            _ => false,
        };

        let is_apple = matches!(
            self.platform.target_os,
            Os::MacOS | Os::iOS | Os::TvOS | Os::WatchOS | Os::VisionOS
        );

        let is_msvc = matches!(self.platform.target_env, Env::Msvc);

        let is_64bit = matches!(self.platform.target_pointer_width, PointerWidth::U64);

        let (prefix, suffix) = match state.types.realize_ty(ty) {
            // Structural types that don't need definitions but we should
            // intern the name of
            Ty::Primitive(prim) => {
                let name = match prim {
                    PrimitiveTy::Bool => "bool ",
                    PrimitiveTy::Ptr => "void *",

                    PrimitiveTy::RustArithmeticTy(rust_arith_ty) => match rust_arith_ty {
                        RustArithmeticTy::I8 => "int8_t ",
                        RustArithmeticTy::I16 => "int16_t ",
                        RustArithmeticTy::I32 => "int32_t ",
                        RustArithmeticTy::I64 => "int64_t ",
                        RustArithmeticTy::I128 => {
                            if is_64bit {
                                "__int128_t "
                            } else {
                                Err(UnsupportedError::Other(
                                    "32-bit and 16-bit c don't have i128?".to_owned(),
                                ))?
                            }
                        }
                        RustArithmeticTy::U8 => "uint8_t ",
                        RustArithmeticTy::U16 => "uint16_t ",
                        RustArithmeticTy::U32 => "uint32_t ",
                        RustArithmeticTy::U64 => "uint64_t ",
                        RustArithmeticTy::U128 => {
                            if is_64bit {
                                "__uint128_t "
                            } else {
                                Err(UnsupportedError::Other(
                                    "32-bit and 16-bit c don't have u128?".to_owned(),
                                ))?
                            }
                        }
                        RustArithmeticTy::F32 => "float ",
                        RustArithmeticTy::F64 => "double ",
                        RustArithmeticTy::I256 => {
                            Err(UnsupportedError::Other("c doesn't have i256?".to_owned()))?
                        }
                        RustArithmeticTy::U256 => {
                            Err(UnsupportedError::Other("c doesn't have u256?".to_owned()))?
                        }
                        RustArithmeticTy::F16 => match &self.cc_flavor {
                            CCFlavor::Gcc => match self.platform.target_arch {
                                Arch::X86
                                | Arch::X86_64
                                | Arch::Arm
                                | Arch::AArch64
                                | Arch::Riscv32
                                | Arch::Riscv64 => "_Float16 ",
                                _ => Err(UnsupportedError::Other(
                                    "GCC isn't known to support f16 on this target".to_owned(),
                                ))?,
                            },
                            CCFlavor::Clang | CCFlavor::Zigcc => match self.platform.target_arch {
                                Arch::X86_64
                                | Arch::Arm
                                | Arch::AArch64
                                | Arch::Riscv32
                                | Arch::Riscv64 => "_Float16 ",
                                Arch::X86 if has_sse2 => "_Float16 ",
                                // mips has no `_Float16`, only `__fp16`.
                                arch if is_mips(arch) => "__fp16 ",
                                _ => Err(UnsupportedError::Other(
                                    "Clang isn't known to support f16 on this target".to_owned(),
                                ))?,
                            },
                            CCFlavor::Msvc => Err(UnsupportedError::Other(
                                "MSVC doesn't support f16".to_owned(),
                            ))?,
                        },
                        RustArithmeticTy::F16b => match &self.cc_flavor {
                            CCFlavor::Gcc => match self.platform.target_arch {
                                Arch::X86
                                | Arch::X86_64
                                | Arch::Arm
                                | Arch::AArch64
                                | Arch::Riscv32
                                | Arch::Riscv64 => "__bf16 ",
                                _ => Err(UnsupportedError::Other(
                                    "GCC isn't known to support f16b on this target".to_owned(),
                                ))?,
                            },
                            CCFlavor::Clang | CCFlavor::Zigcc => match self.platform.target_arch {
                                Arch::X86_64
                                | Arch::Arm
                                | Arch::AArch64
                                | Arch::Riscv32
                                | Arch::Riscv64 => "__bf16 ",
                                Arch::X86 if has_sse2 => "__bf16 ",
                                _ => Err(UnsupportedError::Other(
                                    "Clang isn't known to support f16b on this target".to_owned(),
                                ))?,
                            },
                            CCFlavor::Msvc => Err(UnsupportedError::Other(
                                "MSVC doesn't support f16b".to_owned(),
                            ))?,
                        },
                        RustArithmeticTy::F128 => match &self.cc_flavor {
                            CCFlavor::Gcc => {
                                let msg = "GCC isn't known to support f128 on this target";
                                match self.platform.target_arch {
                                    _ if is_apple => Err(UnsupportedError::Other(msg.to_owned()))?,
                                    Arch::X86
                                    | Arch::X86_64
                                    | Arch::AArch64
                                    | Arch::Sparc64
                                    | Arch::Mips64
                                    | Arch::Mips64r6
                                    | Arch::S390X
                                    | Arch::Riscv32
                                    | Arch::Riscv64
                                    | Arch::Loongarch64 => "_Float128 ",
                                    Arch::PowerPc64 if has_vsx => "_Float128 ",
                                    Arch::Sparc => "long double ",
                                    _ => Err(UnsupportedError::Other(msg.to_owned()))?,
                                }
                            }

                            CCFlavor::Clang | CCFlavor::Zigcc => {
                                let msg = "Clang isn't known to support f128 on this target";
                                match self.platform.target_arch {
                                    _ if is_apple || is_msvc => {
                                        Err(UnsupportedError::Other(msg.to_owned()))?
                                    }
                                    Arch::X86 | Arch::X86_64 => "__float128 ",
                                    Arch::PowerPc64 if has_vsx => "_Float128 ",
                                    Arch::Mips64 | Arch::Mips64r6 | Arch::S390X => "_Float128 ",

                                    // F128 coincides with long double.
                                    Arch::AArch64
                                    | Arch::Riscv32
                                    | Arch::Riscv64
                                    | Arch::Loongarch64
                                    | Arch::Sparc
                                    | Arch::Sparc64 => "long double ",
                                    _ => Err(UnsupportedError::Other(msg.to_owned()))?,
                                }
                            }

                            CCFlavor::Msvc => Err(UnsupportedError::Other(
                                "MSVC doesn't support f128".to_owned(),
                            ))?,
                        },
                    },

                    PrimitiveTy::CArithmeticTy(c_arith_ty) => match c_arith_ty {
                        CArithmeticTy::Char => "char ",
                        CArithmeticTy::SignedChar => "signed char ",
                        CArithmeticTy::UnsignedChar => "unsigned char ",
                        CArithmeticTy::Short => "short ",
                        CArithmeticTy::UnsignedShort => "unsigned short ",
                        CArithmeticTy::Int => "int ",
                        CArithmeticTy::UnsignedInt => "unsigned int ",
                        CArithmeticTy::Long => "long ",
                        CArithmeticTy::UnsignedLong => "unsigned long ",
                        CArithmeticTy::LongLong => "long long ",
                        CArithmeticTy::UnsignedLongLong => "unsigned long long ",
                        CArithmeticTy::Float => "float ",
                        CArithmeticTy::Double => "double ",
                        CArithmeticTy::LongDouble => "long double ",
                    },
                };
                (name.to_owned(), None)
            }
            Ty::Array(ArrayTy { elem_ty, len }) => {
                let (pre, post) = &state.tynames[elem_ty];
                (pre.clone(), Some(format!("[{len}]{post}")))
            }
            Ty::Ref(RefTy { pointee_ty }) => {
                let (pre, post) = &state.tynames[pointee_ty];
                // If the last type modifier was postfix (an array dimension)
                // Then we need to introduce a set of parens to make this pointer
                // bind more tightly
                let was_postfix = matches!(state.types.realize_ty(*pointee_ty), Ty::Array(_));
                if was_postfix {
                    (format!("{pre}(*"), Some(format!("){post}")))
                } else {
                    (format!("{pre}*"), Some(post.clone()))
                }
            }
            // Nominal types we need to emit a decl for
            Ty::Struct(struct_ty) => (format!("{} ", struct_ty.name), None),
            Ty::Union(union_ty) => (format!("{} ", union_ty.name), None),
            Ty::Enum(enum_ty) => (format!("{} ", enum_ty.name), None),
            Ty::Tagged(tagged_ty) => (format!("{} ", tagged_ty.name), None),
            Ty::Alias(alias_ty) => (format!("{} ", alias_ty.name), None),
            // Puns should be evaporated
            Ty::Pun(pun) => {
                let real_ty = state.types.resolve_pun(pun, &state.env).unwrap();
                let (pre, post) = state.tynames[&real_ty].clone();
                (pre, Some(post))
            }
            Ty::Empty => {
                return Err(UnsupportedError::Other(
                    "c doesn't have empty tuples".to_owned(),
                ))?
            }
        };

        state
            .tynames
            .insert(ty, (prefix, suffix.unwrap_or_default()));

        Ok(())
    }

    pub fn generate_forward_decl(
        &self,
        f: &mut Fivemat,
        state: &mut TestState,
        ty: TyIdx,
    ) -> Result<(), GenerateError> {
        // Make sure our own name is interned
        self.intern_tyname(state, ty)?;

        match state.types.realize_ty(ty) {
            // Nominal types we need to emit a decl for
            Ty::Struct(struct_ty) => {
                let ty_name = &struct_ty.name;
                writeln!(f, "typedef struct {ty_name} {ty_name};")?;
            }
            Ty::Union(union_ty) => {
                let ty_name = &union_ty.name;
                writeln!(f, "typedef union {ty_name} {ty_name};")?;
            }
            Ty::Enum(enum_ty) => {
                let ty_name = &enum_ty.name;
                writeln!(f, "typedef enum {ty_name} {ty_name};")?;
            }
            Ty::Tagged(tagged_ty) => {
                let ty_name = &tagged_ty.name;
                writeln!(f, "typedef struct {ty_name} {ty_name};")?;
            }
            Ty::Alias(AliasTy { name, real, attrs }) => {
                if !attrs.is_empty() {
                    return Err(UnsupportedError::Other(
                        "don't yet know how to apply attrs to aliases".to_string(),
                    ))?;
                }
                let (pre, post) = &state.tynames[real];
                writeln!(f, "typedef {pre}{name}{post};\n")?;
            }
            Ty::Pun(..) => {
                // Puns should be evaporated by the type name interner
            }
            Ty::Primitive(prim) => {
                match prim {
                    PrimitiveTy::RustArithmeticTy(
                        RustArithmeticTy::I8
                        | RustArithmeticTy::I16
                        | RustArithmeticTy::I32
                        | RustArithmeticTy::I64
                        | RustArithmeticTy::I128
                        | RustArithmeticTy::I256
                        | RustArithmeticTy::U8
                        | RustArithmeticTy::U16
                        | RustArithmeticTy::U32
                        | RustArithmeticTy::U64
                        | RustArithmeticTy::U128
                        | RustArithmeticTy::U256
                        | RustArithmeticTy::F16
                        | RustArithmeticTy::F16b
                        | RustArithmeticTy::F32
                        | RustArithmeticTy::F64
                        | RustArithmeticTy::F128,
                    )
                    | PrimitiveTy::Bool
                    | PrimitiveTy::Ptr => {
                        // Builtin
                    }

                    PrimitiveTy::CArithmeticTy(
                        CArithmeticTy::Char
                        | CArithmeticTy::SignedChar
                        | CArithmeticTy::UnsignedChar
                        | CArithmeticTy::Short
                        | CArithmeticTy::UnsignedShort
                        | CArithmeticTy::Int
                        | CArithmeticTy::UnsignedInt
                        | CArithmeticTy::Long
                        | CArithmeticTy::UnsignedLong
                        | CArithmeticTy::LongLong
                        | CArithmeticTy::UnsignedLongLong
                        | CArithmeticTy::Float
                        | CArithmeticTy::Double
                        | CArithmeticTy::LongDouble,
                    ) => {
                        // Builtin
                    }
                };
            }
            Ty::Array(ArrayTy { .. }) => {
                // Builtin
            }
            Ty::Ref(RefTy { .. }) => {
                // Builtin
            }
            Ty::Empty => {
                return Err(UnsupportedError::Other(
                    "c doesn't have empty tuples".to_owned(),
                ))?;
            }
        }
        Ok(())
    }

    pub fn generate_tydef(
        &self,
        f: &mut Fivemat,
        state: &mut TestState,
        ty: TyIdx,
    ) -> Result<(), GenerateError> {
        // Make sure our own name is interned
        self.intern_tyname(state, ty)?;

        match state.types.realize_ty(ty) {
            // Nominal types we need to emit a decl for
            Ty::Struct(struct_ty) => {
                // Emit an actual struct decl
                let inner_attrs = self.generate_repr_attr(f, state, &struct_ty.attrs, "struct")?;
                writeln!(f, "typedef struct {}{} {{", inner_attrs, struct_ty.name)?;
                f.add_indent(1);
                for field in &struct_ty.fields {
                    let field_name = &field.ident;
                    let (pre, post) = &state.tynames[&field.ty];
                    writeln!(f, "{pre}{field_name}{post};")?;
                }
                f.sub_indent(1);
                writeln!(f, "}} {};\n", struct_ty.name)?;
            }
            Ty::Union(union_ty) => {
                // Emit an actual union decl
                let inner_attrs = self.generate_repr_attr(f, state, &union_ty.attrs, "union")?;
                writeln!(f, "typedef union {}{} {{", inner_attrs, union_ty.name)?;
                f.add_indent(1);
                for field in &union_ty.fields {
                    let field_name = &field.ident;
                    let (pre, post) = &state.tynames[&field.ty];
                    writeln!(f, "{pre}{field_name}{post};")?;
                }
                f.sub_indent(1);
                writeln!(f, "}} {};\n", union_ty.name)?;
            }
            Ty::Enum(enum_ty) => {
                // Emit an actual enum decl
                let inner_attrs = self.generate_repr_attr(f, state, &enum_ty.attrs, "enum")?;
                writeln!(f, "typedef enum {}{} {{", inner_attrs, enum_ty.name)?;
                f.add_indent(1);
                for variant in &enum_ty.variants {
                    let variant_name = &variant.name;
                    writeln!(f, "{}_{variant_name},", enum_ty.name)?;
                }
                f.sub_indent(1);
                writeln!(f, "}} {};\n", enum_ty.name)?;
            }
            Ty::Tagged(_tagged_ty) => {
                return Err(UnsupportedError::Other(
                    "c doesn't have tagged unions impled yet".to_owned(),
                ))?;
                /*
                // Emit an actual enum decl
                self.generate_repr_attr(f, &tagged_ty.attrs, "tagged")?;
                writeln!(f, "typedef struct {} {{", tagged_ty.name)?;
                f.add_indent(1);
                for variant in &tagged_ty.variants {
                    let variant_name = &variant.name;
                    if let Some(fields) = &variant.fields {
                        writeln!(f, "{variant_name} {{")?;
                        f.add_indent(1);
                        for field in fields {
                            let field_name = &field.ident;
                            let field_tyname = state
                                .borrowed_tynames
                                .get(&field.ty)
                                .unwrap_or(&state.tynames[&field.ty]);
                            writeln!(f, "{field_name}: {field_tyname},")?;
                        }
                        f.sub_indent(1);
                        writeln!(f, "}},")?;
                    } else {
                        writeln!(f, "{variant_name},")?;
                    }
                }
                f.sub_indent(1);
                writeln!(f, "}} {};\n", tagged_ty.name)?;
                 */
            }
            Ty::Alias(_) => {
                // Just reuse the other impl
                self.generate_forward_decl(f, state, ty)?;
            }
            Ty::Pun(..) => {
                // Puns should be evaporated by the type name interner
            }
            Ty::Primitive(prim) => {
                match prim {
                    PrimitiveTy::RustArithmeticTy(
                        RustArithmeticTy::I8
                        | RustArithmeticTy::I16
                        | RustArithmeticTy::I32
                        | RustArithmeticTy::I64
                        | RustArithmeticTy::I128
                        | RustArithmeticTy::I256
                        | RustArithmeticTy::U8
                        | RustArithmeticTy::U16
                        | RustArithmeticTy::U32
                        | RustArithmeticTy::U64
                        | RustArithmeticTy::U128
                        | RustArithmeticTy::U256
                        | RustArithmeticTy::F16
                        | RustArithmeticTy::F16b
                        | RustArithmeticTy::F32
                        | RustArithmeticTy::F64
                        | RustArithmeticTy::F128,
                    )
                    | PrimitiveTy::Bool
                    | PrimitiveTy::Ptr => {
                        // Builtin
                    }

                    PrimitiveTy::CArithmeticTy(
                        CArithmeticTy::Char
                        | CArithmeticTy::SignedChar
                        | CArithmeticTy::UnsignedChar
                        | CArithmeticTy::Short
                        | CArithmeticTy::UnsignedShort
                        | CArithmeticTy::Int
                        | CArithmeticTy::UnsignedInt
                        | CArithmeticTy::Long
                        | CArithmeticTy::UnsignedLong
                        | CArithmeticTy::LongLong
                        | CArithmeticTy::UnsignedLongLong
                        | CArithmeticTy::Float
                        | CArithmeticTy::Double
                        | CArithmeticTy::LongDouble,
                    ) => {
                        // Builtin
                    }
                };
            }
            Ty::Array(ArrayTy { .. }) => {
                // Builtin
            }
            Ty::Ref(RefTy { .. }) => {
                // Builtin
            }
            Ty::Empty => {
                return Err(UnsupportedError::Other(
                    "c doesn't have empty tuples".to_owned(),
                ))?;
            }
        }
        Ok(())
    }

    pub fn generate_repr_attr(
        &self,
        f: &mut Fivemat,
        state: &TestState,
        attrs: &[Attr],
        _ty_style: &str,
    ) -> Result<String, GenerateError> {
        use kdl_script::parse::{AttrAligned, AttrPacked, AttrPassthrough, AttrRepr, Repr};

        let mut default_lang_repr = true;
        let mut lang_repr = None;
        let mut repr_attrs = vec![];
        let mut inner_attrs = vec![];
        let mut other_attrs = vec![];
        for attr in attrs {
            match attr {
                Attr::Align(AttrAligned { align }) => {
                    // This is an "inner" attribute that is applied to the type, not the typedef.
                    //
                    // Applying the attribute only to the typedef raises the alignment but not the
                    // size. Applying it to the type raises both, matching `#[repr(align(N))]`.
                    inner_attrs.push(format!("__attribute__((aligned({})))", align.val));
                }
                Attr::Packed(AttrPacked {}) => {
                    // Ignored on a typedef, must be on the struct.
                    inner_attrs.push("__attribute__((packed))".to_owned());
                }
                Attr::Passthrough(AttrPassthrough(attr)) => {
                    other_attrs.push(attr);
                }
                Attr::Repr(AttrRepr { reprs }) => {
                    default_lang_repr = false;
                    // Any explicit repr attributes disables default C
                    for repr in reprs {
                        match repr {
                            Repr::Transparent => {
                                return Err(UnsupportedError::Other(
                                    "unsupport repr transparent".to_owned(),
                                ))?;
                            }
                            Repr::Primitive(prim) => {
                                return Err(UnsupportedError::Other(format!(
                                    "unsupport repr {prim:?}"
                                )))?;
                            }
                            Repr::Lang(repr) => {
                                if let Some(old_repr) = lang_repr {
                                    return Err(UnsupportedError::Other(format!(
                                        "multiple lang reprs on one type ({old_repr}, {repr})"
                                    )))?;
                                }
                                lang_repr = Some(*repr);
                                continue;
                            }
                        };
                    }
                }
            }
        }
        if default_lang_repr && lang_repr.is_none() {
            lang_repr = Some(state.options.repr);
        }
        if let Some(lang_repr) = lang_repr {
            if let Some(attr) = self.lang_repr_decl(lang_repr)? {
                repr_attrs.push(attr.to_owned());
            }
        }
        if !repr_attrs.is_empty() {
            return Err(UnsupportedError::Other(
                "c doesn't implement non-trivial reprs attributes yet".to_owned(),
            ))?;
        }
        for attr in other_attrs {
            writeln!(f, "{}", attr)?;
        }
        let inner = inner_attrs
            .iter()
            .map(|attr| format!("{attr} "))
            .collect::<String>();
        Ok(inner)
    }

    pub fn generate_signature(
        &self,
        f: &mut Fivemat,
        state: &TestState,
        func: FuncIdx,
    ) -> Result<(), GenerateError> {
        let function = state.types.realize_func(func);

        let (pre, post) = if let Some(output) = function.outputs.first() {
            let (pre, post) = &state.tynames[&output.ty];
            (&**pre, &**post)
        } else {
            ("void ", "")
        };
        let convention_decl = self.convention_decl(state.options.convention)?;
        write!(f, "{pre}{}{}{post}(", convention_decl, function.name)?;
        let mut multiarg = false;
        // Add inputs
        for arg in &function.inputs {
            if multiarg {
                write!(f, ", ")?;
            }
            multiarg = true;
            let arg_name = &arg.name;
            let (pre, post) = &state.tynames[&arg.ty];
            write!(f, "{pre}{}{post}", arg_name)?;
        }
        write!(f, ")")?;
        Ok(())
    }

    pub fn convention_decl(
        &self,
        convention: CallingConvention,
    ) -> Result<&'static str, GenerateError> {
        use CCFlavor::*;
        use CallingConvention::*;
        // GCC (as __attribute__'s)
        //
        //  * x86: cdecl, fastcall, thiscall, stdcall,
        //         sysv_abi, ms_abi (64-bit: -maccumulate-outgoing-args?),
        //         naked, interrupt, sseregparm
        //  * ARM: pcs="aapcs", pcs="aapcs-vfp",
        //         long_call, short_call, naked,
        //         interrupt("IRQ", "FIQ", "SWI", "ABORT", "UNDEF"),
        //
        // MSVC (as ~keywords)
        //
        //  * __cdecl, __clrcall, __stdcall, __fastcall, __thiscall, __vectorcall

        let val = match convention {
            System | Win64 | Sysv64 | Aapcs => {
                // Don't want to think about these yet, I think they're
                // all properly convered by other ABIs
                return Err(self.unsupported_convention(&convention))?;
            }
            // C knows no Rust
            Rust => {
                return Err(self.unsupported_convention(&convention))?;
            }
            C => "",
            Cdecl => {
                if self.platform.target_os == Os::Windows {
                    match self.cc_flavor {
                        Msvc => "__cdecl ",
                        Gcc | Clang | Zigcc => "__attribute__((cdecl)) ",
                    }
                } else {
                    return Err(self.unsupported_convention(&convention))?;
                }
            }
            Stdcall => {
                if self.platform.target_os == Os::Windows {
                    match self.cc_flavor {
                        Msvc => "__stdcall ",
                        Gcc | Clang | Zigcc => "__attribute__((stdcall)) ",
                    }
                } else {
                    return Err(self.unsupported_convention(&convention))?;
                }
            }
            Fastcall => {
                if self.platform.target_os == Os::Windows {
                    match self.cc_flavor {
                        Msvc => "__fastcall ",
                        Gcc | Clang | Zigcc => "__attribute__((fastcall)) ",
                    }
                } else {
                    return Err(self.unsupported_convention(&convention))?;
                }
            }
            Vectorcall => {
                if self.platform.target_os == Os::Windows {
                    match self.cc_flavor {
                        Msvc => "__vectorcall ",
                        Gcc | Clang | Zigcc => "__attribute__((vectorcall)) ",
                    }
                } else {
                    return Err(self.unsupported_convention(&convention))?;
                }
            }
        };

        Ok(val)
    }

    fn lang_repr_decl(&self, repr: LangRepr) -> Result<Option<&'static str>, GenerateError> {
        match repr {
            LangRepr::Rust => Err(UnsupportedError::Other(
                "c doesn't support repr rust".to_owned(),
            ))?,
            LangRepr::C => Ok(None),
        }
    }

    fn unsupported_convention(&self, convention: &CallingConvention) -> UnsupportedError {
        UnsupportedError::Other(format!("unsupported convention {convention}"))
    }
}
