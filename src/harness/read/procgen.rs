use crate::toolchains::is_va_arg_safe;
use kdl_script::types::PRIMITIVES;

pub fn procgen_test_for_ty_string(ty_name: &str, ty_def: Option<&str>) -> String {
    let mut test_body = String::new();
    procgen_test_for_ty_impl(&mut test_body, ty_name, ty_def).expect("failed to format procgen!?");
    test_body
}

fn procgen_test_for_ty_impl(
    out: &mut dyn std::fmt::Write,
    ty_name: &str,
    ty_def: Option<&str>,
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

    // Not every type can be read from a variable argument list.
    if can_type_be_vararg(ty) {
        add_variadics(out, ty)?;
    }

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

/// Can this type be read from a variable argument list?
fn can_type_be_vararg(ty_name: &str) -> bool {
    match PRIMITIVES.iter().find(|(name, _)| *name == ty_name) {
        Some((_, prim)) => is_va_arg_safe(*prim),
        None => false,
    }
}

fn add_variadics(out: &mut dyn std::fmt::Write, ty: &str) -> std::fmt::Result {
    // Just the function being variadic can change its ABI.
    add_func_with_varargs(out, "variadic_abi", &[ty], &[], &[])?;

    // Some calling conventions have an overflow_arg_area: pass sufficient arguments that
    // some actually end up in there.
    for len in 1..=16 {
        add_func_with_varargs(
            out,
            &format!("variadic_in_{len}"),
            &["c_int"],
            &vec![ty; len],
            &[],
        )?;
    }

    // One of these is given to va_start to initialize the va_list,
    // and must itself not be subject to the default argument promotions.
    let padders = ["c_int", "c_double"];

    // Push c-variadic arguments out of the argument registers.
    for padder in padders {
        for len in 1..=8 {
            add_func_with_varargs(
                out,
                &format!("variadic_in_{len}_fixed_{padder}"),
                &vec![padder; len],
                &[ty],
                &[],
            )?;
        }
    }

    // Check that values are padded correctly to respect their alignment.
    let mut aligners = vec![ty];
    aligners.extend(["c_int", ty]);
    aligners.extend(["c_int", "c_int", ty]);
    aligners.extend(["c_long", ty]);
    aligners.extend(["c_longlong", ty]);
    aligners.extend(["c_double", ty]);
    aligners.extend(["c_double", "c_double", ty]);
    // FIXME: enable when rust has c_longdouble
    // aligners.extend(["c_longdouble", ty]);

    add_func_with_varargs(out, "variadic_in_aligners", &["c_int"], &aligners, &[])?;

    Ok(())
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
    add_func_with_varargs_help(out, func_name, inputs, None, outputs)
}

fn add_func_with_varargs(
    out: &mut dyn std::fmt::Write,
    func_name: &str,
    fixed_inputs: &[&str],
    variadic_inputs: &[&str],
    outputs: &[&str],
) -> std::fmt::Result {
    add_func_with_varargs_help(out, func_name, fixed_inputs, Some(variadic_inputs), outputs)
}

fn add_func_with_varargs_help(
    out: &mut dyn std::fmt::Write,
    func_name: &str,
    fixed_inputs: &[&str],
    variadic_inputs: Option<&[&str]>,
    outputs: &[&str],
) -> std::fmt::Result {
    writeln!(out, r#"fn "{func_name}" {{"#)?;
    writeln!(out, r#"    inputs {{"#)?;
    for arg_ty in fixed_inputs {
        writeln!(out, r#"        _ "{arg_ty}""#)?;
    }

    if let Some(variadic_inputs) = variadic_inputs {
        writeln!(out, r#"        _ "...""#)?;

        // This list may be empty, but having a `...` in the signature can change the ABI.
        for arg_ty in variadic_inputs {
            writeln!(out, r#"        _ "{arg_ty}""#)?;
        }
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
