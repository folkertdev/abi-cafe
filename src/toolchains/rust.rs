//! Rust(c) codegen backend backend

mod declare;
mod init;
mod write;

use camino::Utf8Path;
use camino::Utf8PathBuf;
use kdl_script::types::*;
use kdl_script::PunEnv;
use std::collections::HashMap;
use std::fmt::Write;
use std::str::FromStr;
use std::sync::Arc;

use super::super::*;
use super::*;
use crate::fivemat::Fivemat;
use crate::vals::ArgValuesIter;

const CALLER_VALS: &str = "CALLER_VALS";
const CALLEE_VALS: &str = "CALLEE_VALS";
const INDENT: &str = "    ";
const VARARGS: &str = "varargs";

pub struct TestState {
    pub inner: TestImpl,
    // interning state
    pub desired_funcs: Vec<FuncIdx>,
    pub tynames: HashMap<TyIdx, String>,
    pub borrowed_tynames: HashMap<TyIdx, String>,
}
impl std::ops::Deref for TestState {
    type Target = TestImpl;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl TestState {
    fn new(inner: TestImpl) -> Self {
        let desired_funcs = inner.options.functions.active_funcs(&inner.types);
        Self {
            inner,
            desired_funcs,
            tynames: Default::default(),
            borrowed_tynames: Default::default(),
        }
    }
}

#[allow(dead_code)]
pub struct RustcToolchain {
    /// What command should we invoke rustc from?
    command: Utf8PathBuf,
    /// The rustc version
    version: String,
    /// Is this a nightly rustc?
    is_nightly: bool,
    /// Info about the host platform
    pub platform_info: PlatformInfo,
    /// Windowsy or Unixy?
    platform: Platform,
    /// What codegen backend are we using?
    codegen_backend: Option<String>,
    /// What linker should rustc use, if not its default?
    linker: Option<Utf8PathBuf>,
    /// Enable debuginfo
    debug: bool,
}

#[derive(PartialEq)]
enum Platform {
    Windows,
    Unixy,
}

impl Toolchain for RustcToolchain {
    fn lang(&self) -> &'static str {
        "rust"
    }
    fn src_ext(&self) -> &'static str {
        "rs"
    }
    fn pun_env(&self) -> Arc<PunEnv> {
        Arc::new(kdl_script::PunEnv {
            lang: "rust".to_string(),
        })
    }
    fn compile_callee(
        &self,
        src_path: &Utf8Path,
        out_dir: &Utf8Path,
        lib_name: &str,
    ) -> Result<String, BuildError> {
        let mut cmd = Command::new(&self.command);
        cmd.arg("--crate-type")
            .arg("staticlib")
            .arg("--out-dir")
            .arg(out_dir)
            .arg("--target")
            .arg(self.platform_info.target.target_triple)
            .arg(format!("-Cmetadata={lib_name}"))
            .arg(src_path);
        if self.debug {
            cmd.arg("-g");
        }
        if let Some(codegen_backend) = &self.codegen_backend {
            cmd.arg(format!("-Zcodegen-backend={codegen_backend}"));
        }
        debug!("running: {:?}", cmd);
        let out = cmd.output()?;

        if !out.status.success() {
            Err(BuildError::RustCompile(out))
        } else {
            Ok(String::from(lib_name))
        }
    }

    fn compile_caller(
        &self,
        src_path: &Utf8Path,
        out_dir: &Utf8Path,
        lib_name: &str,
    ) -> Result<String, BuildError> {
        // Currently no need to be different
        self.compile_callee(src_path, out_dir, lib_name)
    }

    fn link_bin(
        &self,
        main_src: &Utf8Path,
        out_dir: &Utf8Path,
        build: &BuildOutput,
        bin_name: &str,
    ) -> Result<LinkOutput, LinkError> {
        let output = out_dir.join(bin_name);
        let mut cmd = Command::new(&self.command);
        cmd.arg("-v")
            .arg("-L")
            .arg(out_dir)
            .arg("-l")
            .arg(&build.caller_lib)
            .arg("-l")
            .arg(&build.callee_lib)
            .arg("--crate-type")
            .arg("bin")
            .arg("--target")
            .arg(self.platform_info.target.target_triple)
            // .arg("-Csave-temps=y")
            // .arg("--out-dir")
            // .arg("target/temp/")
            .arg("-o")
            .arg(&output)
            .arg(main_src);
        if let Some(linker) = &self.linker {
            cmd.arg(format!("-Clinker={linker}"));
        }
        if self.debug {
            cmd.arg("-g");
        }

        debug!("running: {:?}", cmd);
        let out = cmd.output()?;

        if !out.status.success() {
            Err(LinkError::RustLink(out))
        } else {
            Ok(LinkOutput { test_bin: output })
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
}

impl RustcToolchain {
    pub fn generate_caller_impl(
        &self,
        f: &mut Fivemat,
        state: &mut TestState,
    ) -> Result<(), GenerateError> {
        // Generate type decls and gather up functions
        self.generate_definitions(f, state)?;
        // Generate decls of the functions we want to call
        self.generate_caller_externs(f, state)?;

        // Generate the test function the harness will call
        writeln!(f, "#[no_mangle]\npub extern \"C\" fn do_test() {{")?;
        for &func in &state.desired_funcs {
            // Generate the individual function calls
            self.generate_caller_body(f, state, func)?;
        }
        writeln!(f, "}}")?;

        Ok(())
    }

    fn generate_caller_body(
        &self,
        f: &mut Fivemat,
        state: &TestState,
        func: FuncIdx,
    ) -> Result<(), GenerateError> {
        writeln!(f, "unsafe {{")?;
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
        if let Some(output) = function.outputs.first() {
            write!(f, "let {} = ", output.name)?;
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

impl RustcToolchain {
    pub fn generate_callee_impl(
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
        let convention_decl = self.convention_decl(state.options.convention)?;
        writeln!(f, "#[no_mangle]")?;
        write!(f, "pub unsafe extern \"{convention_decl}\" ")?;
        self.generate_signature(f, state, func, CallSide::Callee)?;
        writeln!(f, " {{")?;
        f.add_indent(1);
        writeln!(f, "unsafe {{")?;
        f.add_indent(1);

        // Report we're starting a function
        self.write_set_function(f, state, CALLEE_VALS, func)?;

        // Report the inputs
        let mut func_vals = state.vals.at_func(func);
        for arg in function.fixed_inputs() {
            let arg_vals = func_vals.next_arg();
            let arg_name = &arg.name;
            self.write_var(f, state, arg_name, arg.ty, arg_vals, CALLEE_VALS)?;
        }

        // Pull the varargs out of the va_list and report them too
        for arg in function.variadic_inputs() {
            let arg_vals = func_vals.next_arg();
            let arg_name = &arg.name;
            let arg_ty = &state.tynames[&arg.ty];
            writeln!(f, "let {arg_name} = {VARARGS}.next_arg::<{arg_ty}>();")?;
            self.write_var(f, state, arg_name, arg.ty, arg_vals, CALLEE_VALS)?;
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
            writeln!(f, "{}", arg.name)?;
        }
        f.sub_indent(1);
        writeln!(f, "}}")?;
        f.sub_indent(1);
        writeln!(f, "}}")?;
        Ok(())
    }
}

impl RustcToolchain {
    pub fn new(system_info: &Config, command: &Utf8Path, codegen_backend: Option<String>) -> Self {
        // Get rustc's version and host
        let rustc_info = Command::new(command)
            .arg("-Vv")
            .output()
            .expect("rustc -vV failed to run");
        let rustc_info_stdout = String::from_utf8(rustc_info.stdout).unwrap();
        let mut version = None;
        let mut host = None;
        for line in rustc_info_stdout.lines() {
            if let Some(val) = line.strip_prefix("host: ") {
                host = Some(val.to_owned());
            }
            if let Some(val) = line.strip_prefix("release: ") {
                version = Some(val.to_owned());
            }
        }
        let version = version.expect("failed to get rustc version");
        let is_nightly = version.contains("nightly") || version.contains("dev");

        let host = host.expect("failed to get rustc host triple");
        let host = platforms::Platform::find(&host).expect("invalid target triple");

        let target = system_info.target.unwrap_or(host);

        // Get rustc's cfgs for target platform.
        let rustc_cfgs = Command::new(command)
            .arg("--print=cfg")
            .arg(format!("--target={}", target.target_triple))
            .output()
            .expect("rustc failed to run");

        if !rustc_cfgs.status.success() {
            let stderr = String::from_utf8_lossy(&rustc_cfgs.stderr);
            unreachable!(
                "error looking up --target={}:\n{stderr}",
                target.target_triple
            );
        }
        let rustc_cfgs_stdout = String::from_utf8(rustc_cfgs.stdout).unwrap();
        let cfgs = rustc_cfgs_stdout
            .lines()
            .map(|line| cargo_platform::Cfg::from_str(line).expect("failed to parse rustc cfg"))
            .collect::<Vec<_>>();
        let is_windowsy = cfgs.contains(
            &cargo_platform::Cfg::from_str("windows").expect("failed to parse windows cfg"),
        );

        let platform = if is_windowsy {
            Platform::Windows
        } else {
            Platform::Unixy
        };

        Self {
            command: command.to_owned(),
            version,
            is_nightly,
            platform_info: PlatformInfo { target, host, cfgs },
            platform,
            codegen_backend,
            linker: system_info.linker.clone(),
            debug: system_info.debug,
        }
    }

    /// The c-variadic restrictions everyone shares, plus rust's own
    fn check_variadic(&self, state: &TestState, function: &Func) -> Result<(), GenerateError> {
        crate::toolchains::check_variadic(
            &state.types,
            &state.env,
            state.options.convention,
            function,
        )?;
        if self.has_variadic_int128(state, function) && !self.variadic_int128_supported() {
            Err(UnsupportedError::Other(
                "rust doesn't implement 128-bit c-varargs on this target".to_owned(),
            ))?;
        }
        Ok(())
    }

    /// Does this function pass a 128-bit integer as a c-vararg?
    ///
    /// Those need `#![feature(c_variadic_int128)]`, and only exist at all on
    /// the targets where clang provides `__int128`.
    pub fn has_variadic_int128(&self, state: &TestState, function: &Func) -> bool {
        function.variadic_inputs().iter().any(|arg| {
            matches!(
                variadic_arg_prim(&state.types, &state.env, arg.ty),
                Some(PrimitiveTy::I128 | PrimitiveTy::U128)
            )
        })
    }

    /// Are 128-bit c-varargs implemented for this target?
    ///
    /// This mirrors the `VaArgSafe` impls in core, which follow the targets
    /// where clang defines `__int128` (x86_64 is either 64-bit or x32, and
    /// gets `__int128` for both).
    fn variadic_int128_supported(&self) -> bool {
        use platforms::{Arch, PointerWidth};

        let target = self.platform_info.target;
        let is_64bit = target.target_pointer_width == PointerWidth::U64;
        match target.target_arch {
            Arch::X86_64 | Arch::Wasm32 => true,
            Arch::AArch64
            | Arch::Amdgpu
            | Arch::Arm64ec
            | Arch::Bpf
            | Arch::Loongarch64
            | Arch::Mips64
            | Arch::Mips64r6
            | Arch::Nvptx64
            | Arch::PowerPc64
            | Arch::Riscv64
            | Arch::S390X
            | Arch::Sparc64
            | Arch::Wasm64 => is_64bit,
            _ => false,
        }
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
