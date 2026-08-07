/// Which battery of functions a procgen test should generate for its type.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ProcgenMode {
    /// Everything: values, references, structs, ...
    Full,
    /// Only c-variadic calls (which only scalars can participate in)
    Variadic,
}

pub fn procgen_test_for_ty_string(
    ty_name: &str,
    ty_def: Option<&str>,
    mode: ProcgenMode,
) -> String {
    let mut test_body = String::new();
    procgen_test_for_ty_impl(&mut test_body, ty_name, ty_def, mode)
        .expect("failed to format procgen!?");
    test_body
}

fn procgen_test_for_ty_impl(
    out: &mut dyn std::fmt::Write,
    ty_name: &str,
    ty_def: Option<&str>,
    mode: ProcgenMode,
) -> std::fmt::Result {
    let ty = ty_name;
    let ty_ref = format!("&{ty_name}");

    // Apply the type's definitions first
    let has_refs = if let Some(ty_def) = ty_def {
        writeln!(out, "{}", ty_def)?;
        // To avoid outparam nonsense, avoid testing outputs of the type
        // if any part of its definition involves a reference.
        // (Yes this is a blunt check but it's fine enough.)
        ty_def.contains('&')
    } else {
        false
    };

    if let ProcgenMode::Variadic = mode {
        return add_variadics(out, ty);
    }

    // Start gentle with basic one value in/out tests
    add_func(out, "val_in", &[ty], &[])?;
    add_func(out, "ref_in", &[&ty_ref], &[])?;
    if !has_refs {
        add_func(out, "val_out", &[], &[ty])?;
        add_func(out, "val_in_out", &[ty], &[ty])?;
    }

    // Stress out the calling convention and try lots of different
    // input counts. For many types this will result in register
    // exhaustion and get some things passed on the stack.
    for len in 2..=16 {
        add_func(out, &format!("val_in_{len}"), &vec![ty; len], &[])?;
    }

    // Stress out the calling convention with a struct full of values.
    // Some conventions will just shove this in a pointer/stack,
    // others will try to scalarize this into registers anyway.
    add_structs(out, ty)?;

    // Now perturb the arguments by including a byte and a float in
    // the argument list. This will mess with alignment and also mix
    // up the "type classes" (float vs int) and trigger more corner
    // cases in the ABIs as things get distributed to different classes
    // of register.

    // We do small and big versions to check the cases where everything
    // should fit in registers vs not.
    let small_count = 4;
    let big_count = 16;

    add_perturbs(out, ty, small_count, "small")?;
    add_perturbs(out, ty, big_count, "big")?;
    add_perturbs_struct(out, ty, small_count, "small")?;
    add_perturbs_struct(out, ty, big_count, "big")?;
    Ok(())
}

fn add_structs(out: &mut dyn std::fmt::Write, ty: &str) -> std::fmt::Result {
    for len in 1..=16 {
        // Establish type names
        let struct_ty = format!("Many{len}");
        let struct_ty_ref = format!("&{struct_ty}");

        // Emit struct defs
        writeln!(out, r#"struct "{struct_ty}" {{"#)?;
        for field_idx in 0..len {
            writeln!(out, r#"    f{field_idx} "{ty}""#)?;
        }
        writeln!(out, r#"}}"#)?;

        // Check that by-val works
        add_func(out, &format!("struct_in_{len}"), &[&struct_ty], &[])?;
        // Check that by-ref works, for good measure
        add_func(out, &format!("ref_struct_in_{len}"), &[&struct_ty_ref], &[])?;
    }
    Ok(())
}

fn add_perturbs(
    out: &mut dyn std::fmt::Write,
    ty: &str,
    count: usize,
    label: &str,
) -> std::fmt::Result {
    for idx in 0..count {
        let inputs = perturb_list(ty, count, idx);
        add_func(
            out,
            &format!("val_in_{idx}_perturbed_{label}"),
            &inputs,
            &[],
        )?;
    }
    Ok(())
}

fn add_perturbs_struct(
    out: &mut dyn std::fmt::Write,
    ty: &str,
    count: usize,
    label: &str,
) -> std::fmt::Result {
    for idx in 0..count {
        let inputs = perturb_list(ty, count, idx);

        // Establish type names
        let struct_ty = format!("Perturbed{label}{idx}");

        // Emit struct defs
        writeln!(out, r#"struct "{struct_ty}" {{"#)?;
        for (field_idx, field_ty) in inputs.iter().enumerate() {
            writeln!(out, r#"    f{field_idx} "{field_ty}""#)?;
        }
        writeln!(out, r#"}}"#)?;

        // Add the function
        add_func(
            out,
            &format!("struct_in_{idx}_perturbed_{label}"),
            &[&struct_ty],
            &[],
        )?;
    }
    Ok(())
}

/// The types we mix into a varargs list: a 4-byte int, an 8-byte int, and a
/// float.
///
/// C's default argument promotions mean the `u8`/`f32` used for the fixed
/// arguments can never appear in a varargs list, so these are the smallest
/// of each flavour that survives those promotions.
const VARIADIC_MIXERS: &[&str] = &["u32", "u64", "f64"];

/// Stress out the c-variadic parts of the calling convention.
///
/// Varargs are typically passed in a completely different way from the fixed
/// arguments (often a "register save area" the callee spills to), so this
/// wiggles both how many varargs there are and how many registers the fixed
/// arguments burned before them.
fn add_variadics(out: &mut dyn std::fmt::Write, ty: &str) -> std::fmt::Result {
    // Lots of vararg counts, to exhaust whatever the varargs get passed in.
    for len in 1..=16 {
        let inputs = variadic_list(&[ty], &vec![ty; len]);
        add_func(out, &format!("variadic_in_{len}"), &inputs, &[])?;
    }

    // Vary the number of *fixed* args, which is what decides where the
    // varargs start (leftover registers vs the stack).
    for len in 2..=8 {
        let inputs = variadic_list(&vec![ty; len], &mixed_varargs(ty));
        add_func(out, &format!("variadic_in_{len}_fixed"), &inputs, &[])?;
    }

    // Now perturb the argument lists to mess with alignment and the int/float
    // "type classes", which varargs are especially prone to getting wrong
    // (x64 even passes the number of float varargs in a register!).

    // We do small and big versions to check the cases where everything
    // should fit in registers vs not.
    let small_count = 4;
    let big_count = 16;

    add_variadic_alternating(out, ty, small_count, "small")?;
    add_variadic_alternating(out, ty, big_count, "big")?;
    add_variadic_perturbs(out, ty, small_count, "small")?;
    add_variadic_perturbs(out, ty, big_count, "big")?;
    add_variadic_fixed_perturbs(out, ty, small_count, "small")?;
    add_variadic_fixed_perturbs(out, ty, big_count, "big")?;
    Ok(())
}

/// Alternate the type with another one all the way down the varargs list.
///
/// A list that's all one type only ever lands on one alignment, so this is
/// where things like "an 8-byte vararg that follows a 4-byte one" actually
/// get tested. We do both phases so the type is tried at every offset.
fn add_variadic_alternating(
    out: &mut dyn std::fmt::Write,
    ty: &str,
    count: usize,
    label: &str,
) -> std::fmt::Result {
    for mixer in VARIADIC_MIXERS {
        // That would just be a list of one type again
        if *mixer == ty {
            continue;
        }
        for phase in 0..2 {
            let varargs = (0..count)
                .map(|idx| if (idx + phase) % 2 == 0 { ty } else { *mixer })
                .collect::<Vec<_>>();

            let inputs = variadic_list(&[ty], &varargs);
            add_func(
                out,
                &format!("variadic_in_{phase}_alternating_{mixer}_{label}"),
                &inputs,
                &[],
            )?;
        }
    }
    Ok(())
}

/// Slide a single mixer of each flavour through the varargs, leaving the
/// fixed args alone.
fn add_variadic_perturbs(
    out: &mut dyn std::fmt::Write,
    ty: &str,
    count: usize,
    label: &str,
) -> std::fmt::Result {
    for idx in 0..count {
        let mut varargs = vec![ty; count];
        for (mixer_idx, mixer) in VARIADIC_MIXERS.iter().enumerate() {
            // Spread the mixers out evenly, and rotate them all along by idx
            varargs[(idx + mixer_idx * count / VARIADIC_MIXERS.len()) % count] = *mixer;
        }

        let inputs = variadic_list(&[ty], &varargs);
        add_func(
            out,
            &format!("variadic_in_{idx}_perturbed_{label}"),
            &inputs,
            &[],
        )?;
    }
    Ok(())
}

/// Perturb the fixed args, which shifts where the varargs land.
fn add_variadic_fixed_perturbs(
    out: &mut dyn std::fmt::Write,
    ty: &str,
    count: usize,
    label: &str,
) -> std::fmt::Result {
    for idx in 0..count {
        // Note the extra trailing `ty`: C's `va_start` only has defined
        // behaviour if the last fixed arg is unaffected by the default
        // argument promotions, and the perturbs are `u8`/`f32`.
        let mut fixed = perturb_list(ty, count, idx);
        fixed.push(ty);

        let inputs = variadic_list(&fixed, &mixed_varargs(ty));
        add_func(
            out,
            &format!("variadic_in_{idx}_fixed_perturbed_{label}"),
            &inputs,
            &[],
        )?;
    }
    Ok(())
}

/// A varargs list with one of everything in it, for the tests that are really
/// about the fixed arguments but shouldn't pass a uniform list either.
fn mixed_varargs(ty: &str) -> Vec<&str> {
    let mut varargs = vec![ty];
    varargs.extend_from_slice(VARIADIC_MIXERS);
    varargs
}

/// Glue a list of fixed args and a list of varargs together with the
/// `...` marker that makes the function c-variadic.
fn variadic_list<'a>(fixed: &[&'a str], varargs: &[&'a str]) -> Vec<&'a str> {
    let mut inputs = fixed.to_vec();
    inputs.push("...");
    inputs.extend_from_slice(varargs);
    inputs
}

fn perturb_list(ty: &str, count: usize, idx: usize) -> Vec<&str> {
    let mut inputs = vec![ty; count];

    let byte_idx = idx;
    let float_idx = count - 1 - idx;
    inputs[byte_idx] = "u8";
    inputs[float_idx] = "f32";
    inputs
}

fn add_func(
    out: &mut dyn std::fmt::Write,
    func_name: &str,
    inputs: &[&str],
    outputs: &[&str],
) -> std::fmt::Result {
    writeln!(out, r#"fn "{func_name}" {{"#)?;
    writeln!(out, r#"    inputs {{"#)?;
    for arg_ty in inputs {
        writeln!(out, r#"        _ "{arg_ty}""#)?;
    }
    writeln!(out, r#"    }}"#)?;
    writeln!(out, r#"    outputs {{"#)?;
    for arg_ty in outputs {
        writeln!(out, r#"        _ "{arg_ty}""#)?;
    }
    writeln!(out, r#"    }}"#)?;
    writeln!(out, r#"}}"#)?;
    Ok(())
}
