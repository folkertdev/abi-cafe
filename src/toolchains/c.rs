//! C codegen backend backend

mod declare;
mod init;
mod write;

use camino::Utf8Path;
use kdl_script::types::*;
use kdl_script::PunEnv;
use platforms::Arch;
use platforms::Endian;
use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;

use super::super::*;
use super::*;
use crate::fivemat::Fivemat;
use crate::harness::vals::ArgValuesIter;

const CALLER_VALS: &str = "CALLER_VALS";
const CALLEE_VALS: &str = "CALLEE_VALS";
const INDENT: &str = "    ";
const VARARGS: &str = "varargs";

pub struct CcToolchain {
    cc_flavor: CCFlavor,
    platform: &'static platforms::Platform,
    mode: CCMode,
    compiler: Option<Utf8PathBuf>,
    linker: Option<Utf8PathBuf>,
    debug: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CCFlavor {
    Clang,
    Gcc,
    Msvc,
    Zigcc,
}

impl CCFlavor {
    pub fn from_name(name: &str) -> Option<Self> {
        let flavor = match name {
            TOOLCHAIN_GCC => CCFlavor::Gcc,
            TOOLCHAIN_CLANG => CCFlavor::Clang,
            TOOLCHAIN_MSVC => CCFlavor::Msvc,
            TOOLCHAIN_ZIGCC => CCFlavor::Zigcc,
            _ => return None,
        };
        Some(flavor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CCMode {
    CC,
    Clang,
    Gcc,
    Msvc,
    Zigcc,
}

impl CCMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::CC => TOOLCHAIN_CC,
            Self::Clang => TOOLCHAIN_CLANG,
            Self::Gcc => TOOLCHAIN_GCC,
            Self::Msvc => TOOLCHAIN_MSVC,
            Self::Zigcc => TOOLCHAIN_ZIGCC,
        }
    }
}

impl From<CCFlavor> for CCMode {
    fn from(value: CCFlavor) -> Self {
        match value {
            CCFlavor::Gcc => CCMode::Gcc,
            CCFlavor::Clang => CCMode::Clang,
            CCFlavor::Msvc => CCMode::Msvc,
            CCFlavor::Zigcc => CCMode::Zigcc,
        }
    }
}

impl From<CCMode> for ToolchainId {
    fn from(value: CCMode) -> Self {
        value.as_str().to_owned()
    }
}

fn is_mips(arch: Arch) -> bool {
    matches!(
        arch,
        Arch::Mips | Arch::Mips32r6 | Arch::Mips64 | Arch::Mips64r6
    )
}

pub struct TestState {
    pub inner: TestImpl,
    // interning state
    pub desired_funcs: Vec<FuncIdx>,
    pub tynames: HashMap<TyIdx, (String, String)>,
}
impl std::ops::Deref for TestState {
    type Target = TestImpl;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl TestState {
    fn new(inner: TestImpl) -> Self {
        let desired_funcs = inner.options.active_funcs(&inner.types);
        Self {
            inner,
            desired_funcs,
            tynames: Default::default(),
        }
    }
}

impl Toolchain for CcToolchain {
    fn lang(&self) -> &'static str {
        "c"
    }
    fn src_ext(&self) -> &'static str {
        "c"
    }

    fn pun_env(&self) -> Arc<PunEnv> {
        Arc::new(kdl_script::PunEnv {
            lang: "c".to_string(),
        })
    }

    fn compile_callee(
        &self,
        src_path: &Utf8Path,
        out_dir: &Utf8Path,
        lib_name: &str,
    ) -> Result<String, BuildError> {
        match self.mode {
            CCMode::CC => self.compile_cc(src_path, out_dir, lib_name),
            CCMode::Gcc => self.compile_gcc(src_path, out_dir, lib_name),
            CCMode::Clang => self.compile_clang(src_path, out_dir, lib_name),
            CCMode::Msvc => self.compile_msvc(src_path, out_dir, lib_name),
            CCMode::Zigcc => self.compile_zigcc(src_path, out_dir, lib_name),
        }
    }

    fn compile_caller(
        &self,
        src_path: &Utf8Path,
        out_dir: &Utf8Path,
        lib_name: &str,
    ) -> Result<String, BuildError> {
        match self.mode {
            CCMode::CC => self.compile_cc(src_path, out_dir, lib_name),
            CCMode::Gcc => self.compile_gcc(src_path, out_dir, lib_name),
            CCMode::Clang => self.compile_clang(src_path, out_dir, lib_name),
            CCMode::Msvc => self.compile_msvc(src_path, out_dir, lib_name),
            CCMode::Zigcc => self.compile_zigcc(src_path, out_dir, lib_name),
        }
    }

    fn generate_callee(&self, f: &mut dyn Write, test: TestImpl) -> Result<(), GenerateError> {
        let mut f = Fivemat::new(f, INDENT);
        let mut state = TestState::new(test);
        self.generate_callee_impl(&mut f, &mut state)
    }

    fn generate_caller(&self, f: &mut dyn Write, test: TestImpl) -> Result<(), GenerateError> {
        let mut f = Fivemat::new(f, INDENT);
        let mut state = TestState::new(test);
        self.generate_caller_impl(&mut f, &mut state)
    }

    fn link_bin(
        &self,
        main_src: &Utf8Path,
        out_dir: &Utf8Path,
        build: &BuildOutput,
        bin_name: &str,
    ) -> Result<LinkOutput, LinkError> {
        let output = out_dir.join(bin_name);
        let mut cmd = self.link_command();
        if self.debug {
            cmd.arg("-g");
        }
        // The caller references the callee, so it has to come first
        cmd.arg("-o")
            .arg(&output)
            .arg(main_src)
            .arg("-L")
            .arg(out_dir)
            .arg(format!("-l{}", build.caller_lib))
            .arg(format!("-l{}", build.callee_lib));

        debug!("running: {:?}", cmd);
        let out = cmd.output()?;

        if !out.status.success() {
            Err(LinkError::CLink(out))
        } else {
            Ok(LinkOutput { test_bin: output })
        }
    }
}

impl CcToolchain {
    fn generate_caller_impl(
        &self,
        f: &mut Fivemat,
        state: &mut TestState,
    ) -> Result<(), GenerateError> {
        // Generate type decls and gather up functions
        self.generate_definitions(f, state)?;
        // Generate decls of the functions we want to call
        self.generate_caller_externs(f, state)?;

        // Generate the test function the harness will call
        writeln!(f, "void do_test(void) {{")?;
        f.add_indent(1);
        for &func in &state.desired_funcs {
            // Generate the individual function calls
            self.generate_caller_body(f, state, func)?;
        }
        f.sub_indent(1);
        writeln!(f, "}}")?;

        Ok(())
    }

    fn generate_caller_body(
        &self,
        f: &mut Fivemat,
        state: &TestState,
        func: FuncIdx,
    ) -> Result<(), GenerateError> {
        writeln!(f, "{{")?;
        f.add_indent(1);
        let function = state.types.realize_func(func);

        // Report we're starting a function
        self.write_set_function(f, state, CALLER_VALS, func)?;

        // Create vars for all the inputs
        let mut func_vals = state.vals.at_func(func);
        for arg in &function.inputs {
            let arg_vals: ArgValuesIter = func_vals.next_arg();
            // Create and report the input
            self.init_var(f, state, &arg.name, arg.ty, arg_vals.clone())?;
            self.write_var(f, state, &arg.name, arg.ty, arg_vals, CALLER_VALS)?;
        }

        // Call the function
        self.call_function(f, state, function)?;

        // Report all the outputs
        for arg in &function.outputs {
            let arg_vals: ArgValuesIter = func_vals.next_arg();

            self.write_var(f, state, &arg.name, arg.ty, arg_vals, CALLER_VALS)?;
        }

        f.sub_indent(1);
        writeln!(f, "}}")?;
        Ok(())
    }

    fn call_function(
        &self,
        f: &mut Fivemat,
        state: &TestState,
        function: &Func,
    ) -> Result<(), GenerateError> {
        let func_name = &function.name;

        // make sure the outputs aren't weird
        self.check_returns(state, function)?;
        if let Some(arg) = function.outputs.first() {
            let (pre, post) = &state.tynames[&arg.ty];
            write!(f, "{pre}{}{post} = ", arg.name)?;
        }

        // Call the function
        write!(f, "{func_name}(")?;
        let inputs = function.inputs.iter();

        for (arg_idx, arg) in inputs.enumerate() {
            if arg_idx > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", arg.name)?;
        }
        writeln!(f, ");")?;
        writeln!(f)?;
        Ok(())
    }
}

impl CcToolchain {
    fn generate_callee_impl(
        &self,
        f: &mut Fivemat,
        state: &mut TestState,
    ) -> Result<(), GenerateError> {
        // Generate type decls and gather up functions
        self.generate_definitions(f, state)?;

        for &func in &state.desired_funcs {
            // Generate the individual function definitions
            self.generate_callee_body(f, state, func)?;
        }
        Ok(())
    }

    fn generate_callee_body(
        &self,
        f: &mut Fivemat,
        state: &TestState,
        func: FuncIdx,
    ) -> Result<(), GenerateError> {
        let function = state.types.realize_func(func);
        self.generate_signature(f, state, func)?;
        writeln!(f, " {{")?;
        f.add_indent(1);

        // Report we're starting a function
        self.write_set_function(f, state, CALLEE_VALS, func)?;

        // Report the fixed inputs
        let mut func_vals = state.vals.at_func(func);
        for arg in function.fixed_inputs() {
            let arg_vals = func_vals.next_arg();
            let arg_name = &arg.name;
            self.write_var(f, state, arg_name, arg.ty, arg_vals, CALLEE_VALS)?;
        }

        // Report the c-variadic arguments.
        if function.is_variadic() {
            let [.., last_fixed] = function.fixed_inputs() else {
                panic!("we require at least one fixed argument in c-variadic functions");
            };

            // Initialize the va_list.
            writeln!(f, "va_list {VARARGS};")?;
            writeln!(f, "va_start({VARARGS}, {});", last_fixed.name)?;

            for arg in function.variadic_inputs() {
                let arg_vals = func_vals.next_arg();
                let arg_name = &arg.name;
                let (pre, post) = &state.tynames[&arg.ty];
                let arg_ty = format!("{pre}{post}");
                writeln!(
                    f,
                    "{pre}{arg_name}{post} = va_arg({VARARGS}, {});",
                    arg_ty.trim()
                )?;
                self.write_var(f, state, arg_name, arg.ty, arg_vals, CALLEE_VALS)?;
            }
            writeln!(f, "va_end({VARARGS});")?;
        }

        // Create outputs and report them
        for arg in &function.outputs {
            let arg_vals = func_vals.next_arg();
            self.init_var(f, state, &arg.name, arg.ty, arg_vals.clone())?;
            self.write_var(f, state, &arg.name, arg.ty, arg_vals, CALLEE_VALS)?;
        }

        // Return the outputs
        self.check_returns(state, function)?;
        if let Some(arg) = function.outputs.first() {
            writeln!(f, "return {};", arg.name)?;
        }
        f.sub_indent(1);
        writeln!(f, "}}")?;
        Ok(())
    }
}

impl CcToolchain {
    pub fn new(system_info: &Config, platform: &'static platforms::Platform, mode: CCMode) -> Self {
        let cc_flavor = match mode {
            CCMode::Gcc => CCFlavor::Gcc,
            CCMode::Clang => CCFlavor::Clang,
            CCMode::Msvc => CCFlavor::Msvc,
            CCMode::Zigcc => CCFlavor::Zigcc,
            CCMode::CC => {
                let compiler = cc::Build::new()
                    .cargo_metadata(false)
                    .cargo_debug(false)
                    .cargo_warnings(false)
                    .cargo_output(false)
                    .get_compiler();
                if compiler.is_like_msvc() {
                    CCFlavor::Msvc
                } else if compiler.is_like_gnu() {
                    CCFlavor::Gcc
                } else if compiler.is_like_clang() {
                    CCFlavor::Clang
                } else {
                    panic!("Unknown compiler flavour for CC");
                }
            }
        };

        Self {
            cc_flavor,
            platform,
            mode,
            compiler: None,
            linker: system_info.linker.clone(),
            debug: system_info.debug,
        }
    }

    /// A user-supplied compiler (`--add-cc-toolchain flavor:name:path`)
    pub fn new_custom(
        system_info: &Config,
        platform: &'static platforms::Platform,
        cc_flavor: CCFlavor,
        compiler: &Utf8Path,
    ) -> Self {
        Self {
            mode: CCMode::from(cc_flavor),
            cc_flavor,
            platform,
            compiler: Some(compiler.to_owned()),
            linker: system_info.linker.clone(),
            debug: system_info.debug,
        }
    }

    /// The binary to invoke, which the user can override per-toolchain
    fn compiler(&self, default: &'static str) -> Utf8PathBuf {
        self.compiler.clone().unwrap_or_else(|| default.into())
    }

    /// Tell clang (and zig cc, which is clang in disguise) what we're building for.
    /// gcc gets that from its own triple, so it has no equivalent.
    fn clang_target_flag(&self) -> String {
        let rust_triple = self.platform.target_triple;
        let clang_triple = match self.platform.target_arch {
            arch @ (Arch::Riscv32 | Arch::Riscv64) => {
                // Turn `riscv64gc-*` into `riscv64-*` etc.
                let (_, rest) = rust_triple.split_once('-').unwrap();
                format!("{arch}-{rest}")
            }
            _ => rust_triple.to_owned(),
        };
        format!("--target={clang_triple}")
    }

    /// The driver that links the final test binary.
    fn link_command(&self) -> Command {
        // If the user provided an explicit linker, use that.
        if let Some(linker) = &self.linker {
            return Command::new(linker);
        }

        // Otherwise use whatever cc flavor we have.
        match self.mode {
            // Let the cc crate figure out what the host's `cc` even is
            CCMode::CC => cc::Build::new().get_compiler().to_command(),
            CCMode::Gcc => Command::new(self.compiler(TOOLCHAIN_GCC)),
            CCMode::Clang => {
                let mut cmd = Command::new(self.compiler(TOOLCHAIN_CLANG));
                cmd.arg(self.clang_target_flag());
                cmd
            }
            CCMode::Zigcc => {
                let mut cmd = Command::new(self.compiler("zig"));
                cmd.arg("cc").arg(self.clang_target_flag());
                cmd
            }
            CCMode::Msvc => unimplemented!("cannot yet link with msvc"),
        }
    }

    fn extra_flags(&self) -> &[&str] {
        let is_le = self.platform.target_endian == Endian::Little;
        match self.cc_flavor {
            CCFlavor::Gcc if self.platform.target_arch == Arch::Arm => &["-mfp16-format=ieee"],
            CCFlavor::Clang if self.platform.target_arch == Arch::PowerPc64 && is_le => {
                &["-mfloat128"]
            }
            // Without this `__fp16` is storage-only, and can't be passed or returned.
            CCFlavor::Clang | CCFlavor::Zigcc if is_mips(self.platform.target_arch) => {
                &["-Xclang", "-fnative-half-arguments-and-returns"]
            }
            _ => &[],
        }
    }

    fn compile_cc(
        &self,
        src_path: &Utf8Path,
        out_dir: &Utf8Path,
        lib_name: &str,
    ) -> Result<String, BuildError> {
        let mut build = cc::Build::new();
        for flag in self.extra_flags() {
            build.flag(flag);
        }
        build
            .file(src_path)
            .opt_level(0)
            .debug(self.debug)
            .cargo_metadata(false)
            .cargo_debug(false)
            .cargo_warnings(false)
            .cargo_output(false)
            .target(self.platform.target_triple)
            .out_dir(out_dir)
            // .warnings_into_errors(true)
            .try_compile(lib_name)?;
        Ok(String::from(lib_name))
    }

    /// Run a command, and complain if it fails (rather than silently carrying on
    /// to produce a confusing link error later)
    fn run(&self, mut cmd: Command) -> Result<(), BuildError> {
        debug!("running: {:?}", cmd);
        let out = cmd.output()?;
        if out.status.success() {
            Ok(())
        } else {
            Err(BuildError::CCompileFailed(out))
        }
    }

    /// Add the flags every c compiler we drive wants, run it, and archive the result
    fn compile_and_archive(
        &self,
        mut cmd: Command,
        src_path: &Utf8Path,
        out_dir: &Utf8Path,
        lib_name: &str,
    ) -> Result<String, BuildError> {
        let obj_path = out_dir.join(format!("{lib_name}.o"));
        let lib_path = out_dir.join(format!("lib{lib_name}.a"));
        for flag in self.extra_flags() {
            cmd.arg(flag);
        }
        if self.debug {
            cmd.arg("-g");
        }
        cmd.arg("-ffunction-sections")
            .arg("-fdata-sections")
            .arg("-fPIC")
            .arg("-o")
            .arg(&obj_path)
            .arg("-c")
            .arg(src_path);
        self.run(cmd)?;

        let mut ar = Command::new("ar");
        ar.arg("cq").arg(&lib_path).arg(&obj_path);
        self.run(ar)?;

        let mut ar = Command::new("ar");
        ar.arg("s").arg(&lib_path);
        self.run(ar)?;

        Ok(String::from(lib_name))
    }

    fn compile_clang(
        &self,
        src_path: &Utf8Path,
        out_dir: &Utf8Path,
        lib_name: &str,
    ) -> Result<String, BuildError> {
        let mut cmd = Command::new(self.compiler(TOOLCHAIN_CLANG));
        cmd.arg(self.clang_target_flag());
        self.compile_and_archive(cmd, src_path, out_dir, lib_name)
    }

    fn compile_zigcc(
        &self,
        src_path: &Utf8Path,
        out_dir: &Utf8Path,
        lib_name: &str,
    ) -> Result<String, BuildError> {
        let mut cmd = Command::new(self.compiler("zig"));
        cmd.arg("cc").arg(self.clang_target_flag());
        self.compile_and_archive(cmd, src_path, out_dir, lib_name)
    }

    fn compile_gcc(
        &self,
        src_path: &Utf8Path,
        out_dir: &Utf8Path,
        lib_name: &str,
    ) -> Result<String, BuildError> {
        let cmd = Command::new(self.compiler(TOOLCHAIN_GCC));
        self.compile_and_archive(cmd, src_path, out_dir, lib_name)
    }

    fn compile_msvc(
        &self,
        _src_path: &Utf8Path,
        _out_dir: &Utf8Path,
        _lib_name: &str,
    ) -> Result<String, BuildError> {
        unimplemented!()
    }

    fn check_returns(&self, state: &TestState, function: &Func) -> Result<(), GenerateError> {
        let has_outparams = function
            .outputs
            .iter()
            .any(|arg| state.types.ty_contains_ref(arg.ty));
        if has_outparams {
            return Err(UnsupportedError::Other(
                "outparams (outputs containing references) aren't supported".to_owned(),
            ))?;
        }
        if function.outputs.len() > 1 {
            return Err(UnsupportedError::Other(
                "multiple returns (should this be a struct?)".to_owned(),
            ))?;
        }
        Ok(())
    }
}
