use std::fmt::Write;
use std::sync::Arc;

use crate::harness::report::{BuildOutput, LinkOutput};
use crate::harness::test::*;
use crate::{error::*, SortedMap};

use camino::{Utf8Path, Utf8PathBuf};
use kdl_script::types::{Func, PrimitiveTy, Ty, TyIdx, TypedProgram};
use kdl_script::PunEnv;

pub mod c;
pub mod rust;

pub use c::{CCFlavor, CcToolchain};
pub use rust::RustcToolchain;

pub const TOOLCHAIN_RUSTC: &str = "rustc";
pub const TOOLCHAIN_CC: &str = "cc";
pub const TOOLCHAIN_GCC: &str = "gcc";
pub const TOOLCHAIN_CLANG: &str = "clang";
pub const TOOLCHAIN_MSVC: &str = "msvc";
pub const TOOLCHAIN_ZIGCC: &str = "zigcc";

const C_TOOLCHAINS: &[&str] = &[
    TOOLCHAIN_CC,
    TOOLCHAIN_GCC,
    TOOLCHAIN_CLANG,
    TOOLCHAIN_MSVC,
    TOOLCHAIN_ZIGCC,
];

/// The c-variadic restrictions that every language agrees on.
///
/// Backends should call this before generating a c-variadic function, and
/// may of course have restrictions of their own on top of these.
pub fn check_variadic(
    types: &TypedProgram,
    env: &PunEnv,
    convention: CallingConvention,
    function: &Func,
) -> Result<(), GenerateError> {
    if !convention.supports_c_variadic() {
        Err(UnsupportedError::Other(format!(
            "the {convention} convention can't be c-variadic"
        )))?;
    }
    // C only allows this in C23, and it's not worth the trouble of detecting
    // that; every backend also needs a fixed arg to anchor `va_start` on.
    if function.fixed_inputs().is_empty() {
        Err(UnsupportedError::Other(
            "a c-variadic function needs at least one fixed argument".to_owned(),
        ))?;
    }
    for arg in function.variadic_inputs() {
        if variadic_arg_prim(types, env, arg.ty).is_none() {
            Err(UnsupportedError::Other(format!(
                "{} can't be passed as a c-vararg",
                types.format_ty(arg.ty)
            )))?;
        }
    }
    Ok(())
}

/// The primitive a c-vararg of this type is passed as, if it can be one at all.
///
/// C's default argument promotions rewrite anything narrower than an `int`
/// or a `double` on the way in, so those types can never be read back out of
/// a `va_list` as themselves. Everything wider is fair game, but so far only
/// scalars are implemented (rust's `VaArgSafe` has the same restriction).
pub fn variadic_arg_prim(types: &TypedProgram, env: &PunEnv, ty: TyIdx) -> Option<PrimitiveTy> {
    match types.realize_ty(ty) {
        Ty::Primitive(prim) => match prim {
            PrimitiveTy::I32
            | PrimitiveTy::I64
            | PrimitiveTy::I128
            | PrimitiveTy::U32
            | PrimitiveTy::U64
            | PrimitiveTy::U128
            | PrimitiveTy::F64
            | PrimitiveTy::Ptr => Some(*prim),
            // Promoted to int/uint/double, so they arrive as another type
            PrimitiveTy::I8
            | PrimitiveTy::I16
            | PrimitiveTy::U8
            | PrimitiveTy::U16
            | PrimitiveTy::F16
            | PrimitiveTy::F32
            | PrimitiveTy::Bool => None,
            // Not scalars either language can read back out
            PrimitiveTy::I256 | PrimitiveTy::U256 | PrimitiveTy::F128 | PrimitiveTy::Complex(_) => {
                None
            }
        },
        Ty::Alias(alias_ty) => variadic_arg_prim(types, env, alias_ty.real),
        Ty::Pun(pun) => {
            let real_ty = types.resolve_pun(pun, env).unwrap();
            variadic_arg_prim(types, env, real_ty)
        }
        Ty::Struct(_)
        | Ty::Union(_)
        | Ty::Enum(_)
        | Ty::Tagged(_)
        | Ty::Array(_)
        | Ty::Ref(_)
        | Ty::Empty => None,
    }
}

/// A compiler/language toolchain!
pub trait Toolchain {
    fn lang(&self) -> &'static str;
    fn src_ext(&self) -> &'static str;
    fn pun_env(&self) -> Arc<PunEnv>;
    fn generate_callee(&self, f: &mut dyn Write, test: TestImpl) -> Result<(), GenerateError>;
    fn generate_caller(&self, f: &mut dyn Write, test: TestImpl) -> Result<(), GenerateError>;

    fn compile_callee(
        &self,
        src_path: &Utf8Path,
        out_dir: &Utf8Path,
        lib_name: &str,
    ) -> Result<String, BuildError>;
    fn compile_caller(
        &self,
        src_path: &Utf8Path,
        out_dir: &Utf8Path,
        lib_name: &str,
    ) -> Result<String, BuildError>;

    /// Link the caller and callee static libs together with `main_src` (a main
    /// written in this toolchain's language) into a runnable test binary.
    fn link_bin(
        &self,
        main_src: &Utf8Path,
        out_dir: &Utf8Path,
        build: &BuildOutput,
        bin_name: &str,
    ) -> Result<LinkOutput, LinkError>;
}

/// All the toolchains
pub struct Toolchains {
    pub platform_info: PlatformInfo,
    pub rustc_command: Utf8PathBuf,
    pub linker: Option<Utf8PathBuf>,
    pub toolchains: ToolchainMap,
    pub debug: bool,
}
pub type ToolchainMap = SortedMap<String, Arc<dyn Toolchain + Send + Sync>>;

#[derive(Debug, Clone)]
pub struct PlatformInfo {
    /// Platform we're targetting
    pub target: &'static platforms::Platform,
    /// Platform we're running on
    pub host: &'static platforms::Platform,
    /// Enabled rustc cfgs of the target, used for our own test harness cfgs
    pub cfgs: Vec<cargo_platform::Cfg>,
}

/// Create all the toolchains
pub(crate) fn create_toolchains(cfg: &crate::Config) -> Toolchains {
    let mut toolchains = ToolchainMap::default();

    let rustc_command: Utf8PathBuf = "rustc".into();
    let base_rustc = RustcToolchain::new(cfg, &rustc_command, None);
    let platform_info = base_rustc.platform_info.clone();

    // Set up env vars for CC
    std::env::set_var("OUT_DIR", &cfg.paths.out_dir);
    std::env::set_var("HOST", platform_info.host.target_triple);
    std::env::set_var("TARGET", platform_info.target.target_triple);
    std::env::set_var("OPT_LEVEL", "0");

    // Add rust toolchains
    add_toolchain(&mut toolchains, TOOLCHAIN_RUSTC, base_rustc);
    for (name, path) in &cfg.rustc_codegen_backends {
        add_toolchain(
            &mut toolchains,
            name,
            RustcToolchain::new(cfg, &rustc_command, Some(path.to_owned())),
        );
    }

    // Add c toolchains
    for &name in C_TOOLCHAINS {
        add_toolchain(
            &mut toolchains,
            name,
            CcToolchain::new(cfg, platform_info.target, name),
        );
    }
    for (flavor, name, path) in &cfg.cc_toolchains {
        add_toolchain(
            &mut toolchains,
            name,
            CcToolchain::new_custom(cfg, platform_info.target, *flavor, path),
        );
    }

    Toolchains {
        platform_info,
        rustc_command,
        linker: cfg.linker.clone(),
        toolchains,
        debug: cfg.debug,
    }
}

/// Register a toolchain
fn add_toolchain<A: Toolchain + Send + Sync + 'static>(
    toolchains: &mut ToolchainMap,
    id: impl Into<ToolchainId>,
    toolchain: A,
) {
    let id = id.into();
    let old = toolchains.insert(id.clone(), Arc::new(toolchain));
    assert!(old.is_none(), "duplicate toolchain id: {}", id);
}
