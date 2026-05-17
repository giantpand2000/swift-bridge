use crate::bridged_type::{
    fn_arg_name, pat_type_pat_is_self, BridgeableType, BridgedType, OpaqueForeignType, StdLibType,
    TypePosition,
};
use crate::parse::{HostLang, TypeDeclaration};
use crate::parsed_extern_fn::FailableInitializerType;
use crate::{ParsedExternFn, TypeDeclarations, SWIFT_BRIDGE_PREFIX};
use quote::{format_ident, ToTokens};
use std::ops::Deref;
use syn::{FnArg, Path, ReturnType, Type};

#[derive(Copy, Clone, PartialEq)]
pub(super) enum OpaqueRustRefSwiftRepr {
    Class,
    Struct,
}

pub(super) fn gen_func_swift_calls_rust(
    function: &ParsedExternFn,
    types: &TypeDeclarations,
    swift_bridge_path: &Path,
) -> String {
    gen_func_swift_calls_rust_with_opaque_ref_repr(
        function,
        types,
        swift_bridge_path,
        OpaqueRustRefSwiftRepr::Class,
    )
}

pub(super) fn gen_func_swift_calls_rust_with_struct_refs(
    function: &ParsedExternFn,
    types: &TypeDeclarations,
    swift_bridge_path: &Path,
) -> String {
    gen_func_swift_calls_rust_with_opaque_ref_repr(
        function,
        types,
        swift_bridge_path,
        OpaqueRustRefSwiftRepr::Struct,
    )
}

pub(super) fn has_immutable_opaque_rust_ref_arg(
    function: &ParsedExternFn,
    types: &TypeDeclarations,
) -> bool {
    function.func.sig.inputs.iter().any(|arg| match arg {
        FnArg::Receiver(_) => false,
        FnArg::Typed(pat_ty) => opaque_rust_struct_ref(&pat_ty.ty, types).is_some(),
    })
}

fn gen_func_swift_calls_rust_with_opaque_ref_repr(
    function: &ParsedExternFn,
    types: &TypeDeclarations,
    swift_bridge_path: &Path,
    opaque_ref_repr: OpaqueRustRefSwiftRepr,
) -> String {
    let fn_name = function.sig.ident.to_string();
    let params =
        to_swift_param_names_and_types(function, false, types, swift_bridge_path, opaque_ref_repr);
    let call_args = to_swift_call_args(
        function,
        true,
        false,
        types,
        swift_bridge_path,
        opaque_ref_repr,
    );
    let call_fn = if function.sig.asyncness.is_some() {
        let maybe_args = if function.sig.inputs.is_empty() {
            "".to_string()
        } else {
            format!(", {}", call_args)
        };

        format!("{}(wrapperPtr, onComplete{})", fn_name, maybe_args)
    } else {
        format!("{}({})", fn_name, call_args)
    };

    let maybe_type_name_segment = if let Some(ty) = function.associated_type.as_ref() {
        match ty {
            TypeDeclaration::Shared(_) => {
                //
                todo!()
            }
            TypeDeclaration::Opaque(ty) => {
                format!("${}", ty.to_string())
            }
        }
    } else {
        "".to_string()
    };

    let maybe_static_class_func = if function.associated_type.is_some()
        && (!function.is_method() && !function.is_swift_initializer)
    {
        if function.is_copy_method_on_opaque_type() {
            "static "
        } else {
            "class "
        }
    } else {
        ""
    };

    let public_func_fn_name = if function.is_swift_initializer {
        if function.is_copy_method_on_opaque_type() {
            "public init".to_string()
        } else {
            if let Some(crate::parsed_extern_fn::FailableInitializerType::Throwing) =
                function.swift_failable_initializer
            {
                "public convenience init".to_string()
            } else if let Some(crate::parsed_extern_fn::FailableInitializerType::Option) =
                function.swift_failable_initializer
            {
                "public convenience init?".to_string()
            } else {
                "public convenience init".to_string()
            }
        }
    } else {
        if let Some(swift_name) = &function.swift_name_override {
            format!("public func {}", swift_name.value())
        } else {
            format!("public func {}", fn_name.as_str())
        }
    };

    let maybe_throws =
        if let Some(FailableInitializerType::Throwing) = function.swift_failable_initializer {
            " throws"
        } else {
            ""
        };
    let indentation = if function.associated_type.is_some() {
        "    "
    } else {
        ""
    };

    let call_rust = format!(
        "{prefix}{type_name_segment}${call_fn}",
        prefix = SWIFT_BRIDGE_PREFIX,
        type_name_segment = maybe_type_name_segment,
        call_fn = call_fn
    );
    let mut call_rust = if function.sig.asyncness.is_some() {
        call_rust
    } else if function.is_swift_initializer {
        if let Some(FailableInitializerType::Throwing) = function.swift_failable_initializer {
            let built_in = function.return_ty_built_in(types).unwrap();
            built_in.convert_ffi_value_to_swift_value(
                &call_rust,
                TypePosition::ThrowingInit(function.host_lang),
                types,
                swift_bridge_path,
            )
        } else {
            call_rust
        }
    } else if let Some(built_in) = function.return_ty_built_in(types) {
        convert_ffi_return_value_to_swift_value(
            function,
            &built_in,
            &call_rust,
            TypePosition::FnReturn(function.host_lang),
            types,
            swift_bridge_path,
            opaque_ref_repr,
        )
    } else {
        if function.host_lang.is_swift() {
            call_rust
        } else {
            match &function.sig.output {
                ReturnType::Default => {
                    // () is a built in type so this would have been handled in the previous block.
                    unreachable!()
                }
                ReturnType::Type(_, ty) => {
                    let ty_name = match ty.deref() {
                        Type::Reference(reference) => reference.elem.to_token_stream().to_string(),
                        Type::Path(path) => path.path.segments.to_token_stream().to_string(),
                        _ => todo!(),
                    };

                    match types.get(&ty_name).unwrap() {
                        TypeDeclaration::Shared(_) => call_rust,
                        TypeDeclaration::Opaque(opaque) => {
                            if opaque.host_lang.is_rust() {
                                let (is_owned, ty) = match ty.deref() {
                                    Type::Reference(reference) => ("false", &reference.elem),
                                    _ => ("true", ty),
                                };

                                let ty = ty.to_token_stream().to_string();
                                format!("{}(ptr: {}, isOwned: {})", ty, call_rust, is_owned)
                            } else {
                                let ty = ty.to_token_stream().to_string();
                                format!(
                                    "Unmanaged<{}>.fromOpaque({}).takeRetainedValue()",
                                    ty, call_rust
                                )
                            }
                        }
                    }
                }
            }
        }
    };
    let returns_null = BridgedType::new_with_return_type(&function.func.sig.output, types)
        .map(|b| b.is_null())
        .unwrap_or(false);

    let maybe_return = if returns_null || function.is_swift_initializer {
        ""
    } else {
        "return "
    };

    for arg in function.func.sig.inputs.iter() {
        let bridged_arg = BridgedType::new_with_fn_arg(arg, types);
        if bridged_arg.is_none() {
            continue;
        }
        let bridged_arg = bridged_arg.unwrap();

        let arg_name = fn_arg_name(arg).unwrap().to_string();

        // TODO: Refactor to make less duplicative
        match bridged_arg {
            BridgedType::StdLib(StdLibType::Str) => {
                call_rust = format!(
                    r#"{maybe_return}{arg}.toRustStr({{ {arg}AsRustStr in
{indentation}        {call_rust}
{indentation}    }})"#,
                    maybe_return = maybe_return,
                    indentation = indentation,
                    arg = arg_name,
                    call_rust = call_rust
                );
            }
            BridgedType::StdLib(StdLibType::Option(briged_opt)) if briged_opt.ty.is_str() => {
                call_rust = format!(
                    r#"{maybe_return}optionalRustStrToRustStr({arg}, {{ {arg}AsRustStr in
{indentation}        {call_rust}
{indentation}    }})"#,
                    maybe_return = maybe_return,
                    indentation = indentation,
                    arg = arg_name,
                    call_rust = call_rust
                );
            }
            _ => {}
        }
    }

    if function.is_swift_initializer {
        if function.is_copy_method_on_opaque_type() {
            call_rust = format!("self.bytes = {}", call_rust)
        } else {
            if let Some(FailableInitializerType::Option) = function.swift_failable_initializer {
                call_rust = format!(
                    "guard let val = {} else {{ return nil }}; self.init(ptr: val)",
                    call_rust
                )
            } else if function.swift_failable_initializer.is_none() {
                call_rust = format!("self.init(ptr: {})", call_rust)
            }
        }
    }

    let maybe_return = if function.is_swift_initializer {
        "".to_string()
    } else {
        to_swift_return_type(function, types, swift_bridge_path, opaque_ref_repr)
    };

    let maybe_generics = function.maybe_swift_generics(types);

    let func_definition = if function.sig.asyncness.is_some() {
        let func_ret_ty = function.return_ty_built_in(types).unwrap();
        let rust_fn_ret_ty = swift_return_type_for_bridged_type(
            function,
            &func_ret_ty,
            TypePosition::FnReturn(HostLang::Rust),
            types,
            swift_bridge_path,
            opaque_ref_repr,
        );
        let maybe_on_complete_sig_ret_val = if func_ret_ty.is_null() {
            "".to_string()
        } else {
            format!(
                ", rustFnRetVal: {}",
                func_ret_ty.to_swift_type(
                    TypePosition::ResultFfiReturnType,
                    types,
                    swift_bridge_path
                )
            )
        };
        let callback_wrapper_ty = format!("CbWrapper{}${}", maybe_type_name_segment, fn_name);
        let (run_wrapper_cb, error, maybe_try, with_checked_continuation_function_name) =
            if let Some(result) = func_ret_ty.as_result() {
                let run_wrapper_cb = result.generate_swift_calls_async_rust_callback(
                    "rustFnRetVal",
                    TypePosition::FnReturn(HostLang::Rust),
                    types,
                    swift_bridge_path,
                );
                (
                    run_wrapper_cb,
                    "Error".to_string(),
                    " try ".to_string(),
                    "withCheckedThrowingContinuation".to_string(),
                )
            } else {
                let on_complete_ret_val = if func_ret_ty.is_null() {
                    "()".to_string()
                } else {
                    convert_ffi_return_value_to_swift_value(
                        function,
                        &func_ret_ty,
                        "rustFnRetVal",
                        TypePosition::ResultFfiReturnType,
                        types,
                        swift_bridge_path,
                        opaque_ref_repr,
                    )
                };
                (
                    format!(r#"wrapper.cb(.success({on_complete_ret_val}))"#),
                    "Never".to_string(),
                    " ".to_string(),
                    "withCheckedContinuation".to_string(),
                )
            };
        let callback_wrapper = format!(
            r#"{indentation}class {cb_wrapper_ty} {{
{indentation}    var cb: (Result<{rust_fn_ret_ty}, {error}>) -> ()
{indentation}
{indentation}    public init(cb: @escaping (Result<{rust_fn_ret_ty}, {error}>) -> ()) {{
{indentation}        self.cb = cb
{indentation}    }}
{indentation}}}"#,
            indentation = indentation,
            cb_wrapper_ty = callback_wrapper_ty
        );

        let fn_body = format!(
            r#"func onComplete(cbWrapperPtr: UnsafeMutableRawPointer?{maybe_on_complete_sig_ret_val}) {{
    let wrapper = Unmanaged<{cb_wrapper_ty}>.fromOpaque(cbWrapperPtr!).takeRetainedValue()
    {run_wrapper_cb}
}}

return{maybe_try}await {with_checked_continuation_function_name}({{ (continuation: CheckedContinuation<{rust_fn_ret_ty}, {error}>) in
    let callback = {{ rustFnRetVal in
        continuation.resume(with: rustFnRetVal)
    }}

    let wrapper = {cb_wrapper_ty}(cb: callback)
    let wrapperPtr = Unmanaged.passRetained(wrapper).toOpaque()

    {call_rust}
}})"#,
            rust_fn_ret_ty = rust_fn_ret_ty,
            error = error,
            maybe_on_complete_sig_ret_val = maybe_on_complete_sig_ret_val,
            cb_wrapper_ty = callback_wrapper_ty,
            call_rust = call_rust,
        );

        let mut fn_body_indented = "".to_string();
        for line in fn_body.lines() {
            if line.len() > 0 {
                fn_body_indented += &format!("{}    {}\n", indentation, line);
            } else {
                fn_body_indented += "\n"
            }
        }
        let fn_body_indented = fn_body_indented.trim_end();

        format!(
            r#"{indentation}{maybe_static_class_func}{swift_class_func_name}{maybe_generics}({params}) async{maybe_ret} {{
{fn_body_indented}
{indentation}}}
{callback_wrapper}"#,
            indentation = indentation,
            maybe_static_class_func = maybe_static_class_func,
            swift_class_func_name = public_func_fn_name,
            maybe_generics = maybe_generics,
            params = params,
            maybe_ret = maybe_return,
            fn_body_indented = fn_body_indented,
            callback_wrapper = callback_wrapper
        )
    } else {
        format!(
            r#"{indentation}{maybe_static_class_func}{swift_class_func_name}{maybe_generics}({params}){maybe_throws}{maybe_ret} {{
{indentation}    {call_rust}
{indentation}}}"#,
            indentation = indentation,
            maybe_static_class_func = maybe_static_class_func,
            swift_class_func_name = public_func_fn_name,
            maybe_generics = maybe_generics,
            params = params,
            maybe_ret = maybe_return,
            call_rust = call_rust,
            maybe_throws = maybe_throws,
        )
    };

    func_definition
}

fn to_swift_param_names_and_types(
    function: &ParsedExternFn,
    include_receiver_if_present: bool,
    types: &TypeDeclarations,
    swift_bridge_path: &Path,
    opaque_ref_repr: OpaqueRustRefSwiftRepr,
) -> String {
    if opaque_ref_repr == OpaqueRustRefSwiftRepr::Class {
        return function.to_swift_param_names_and_types(
            include_receiver_if_present,
            types,
            swift_bridge_path,
        );
    }

    let mut params: Vec<String> = vec![];

    for (arg_idx, arg) in function.func.sig.inputs.iter().enumerate() {
        let param = match arg {
            FnArg::Receiver(_receiver) => {
                if include_receiver_if_present {
                    params.push(format!("_ this: UnsafeMutableRawPointer"));
                }

                continue;
            }
            FnArg::Typed(pat_ty) => {
                if pat_type_pat_is_self(pat_ty) {
                    if include_receiver_if_present {
                        params.push(format!("_ this: UnsafeMutableRawPointer"));
                    }

                    continue;
                }

                let arg_name = pat_ty.pat.to_token_stream().to_string();

                let ty = swift_type_for_type(
                    &pat_ty.ty,
                    TypePosition::FnArg(function.host_lang, arg_idx),
                    types,
                    swift_bridge_path,
                    opaque_ref_repr,
                );

                if let Some(argument_label) =
                    function.argument_labels.get(&format_ident!("{}", arg_name))
                {
                    format!("{} {}: {}", argument_label.value().as_str(), arg_name, ty)
                } else {
                    format!("_ {}: {}", arg_name, ty)
                }
            }
        };
        params.push(param)
    }

    params.join(", ")
}

fn to_swift_call_args(
    function: &ParsedExternFn,
    include_receiver_if_present: bool,
    include_var_name: bool,
    types: &TypeDeclarations,
    swift_bridge_path: &Path,
    opaque_ref_repr: OpaqueRustRefSwiftRepr,
) -> String {
    if opaque_ref_repr == OpaqueRustRefSwiftRepr::Class {
        return function.to_swift_call_args(
            include_receiver_if_present,
            include_var_name,
            types,
            swift_bridge_path,
        );
    }

    let mut args = vec![];
    let inputs = &function.func.sig.inputs;
    for (arg_idx, arg) in inputs.iter().enumerate() {
        match arg {
            FnArg::Receiver(receiver) => {
                if include_receiver_if_present {
                    push_receiver_as_arg(function, &mut args, receiver.reference.is_some());
                }
            }
            FnArg::Typed(pat_ty) => {
                let is_reference = match pat_ty.ty.deref() {
                    Type::Reference(_) => true,
                    _ => false,
                };

                if pat_type_pat_is_self(pat_ty) {
                    if include_receiver_if_present {
                        push_receiver_as_arg(function, &mut args, is_reference);
                    }

                    continue;
                }

                let pat = &pat_ty.pat;
                let arg = pat.to_token_stream().to_string();
                let arg_name = arg.clone();

                let arg = convert_swift_expression_to_ffi_type(
                    &pat_ty.ty,
                    &arg,
                    TypePosition::FnArg(function.host_lang, arg_idx),
                    types,
                    swift_bridge_path,
                    opaque_ref_repr,
                );
                let arg = if include_var_name {
                    if let Some(label) =
                        function.argument_labels.get(&format_ident!("{}", arg_name))
                    {
                        let label_str = label.value();
                        if label_str == "_" {
                            arg
                        } else {
                            format!("{}: {}", label_str, arg)
                        }
                    } else {
                        format!("{}: {}", arg_name, arg)
                    }
                } else {
                    arg
                };

                args.push(arg);
            }
        };
    }
    args.join(", ")
}

fn push_receiver_as_arg(function: &ParsedExternFn, args: &mut Vec<String>, is_reference: bool) {
    let arg = if function.is_copy_method_on_opaque_type() {
        "self.bytes"
    } else if is_reference {
        "ptr"
    } else {
        "{isOwned = false; return ptr;}()"
    };
    args.push(arg.to_string());
}

fn to_swift_return_type(
    function: &ParsedExternFn,
    types: &TypeDeclarations,
    swift_bridge_path: &Path,
    opaque_ref_repr: OpaqueRustRefSwiftRepr,
) -> String {
    if opaque_ref_repr == OpaqueRustRefSwiftRepr::Class {
        return function.to_swift_return_type(types, swift_bridge_path);
    }

    match &function.func.sig.output {
        ReturnType::Default => "".to_string(),
        ReturnType::Type(_, ty) => {
            if let Some(built_in) = BridgedType::new_with_type(ty, types) {
                if function.host_lang.is_swift() {
                    if built_in.can_be_encoded_with_zero_bytes() {
                        return "".to_string();
                    }
                }

                let maybe_throws = if built_in.is_result() { "throws " } else { "" };

                format!(
                    " {}-> {}",
                    maybe_throws,
                    swift_type_for_type(
                        ty,
                        TypePosition::FnReturn(function.host_lang),
                        types,
                        swift_bridge_path,
                        opaque_ref_repr,
                    )
                )
            } else {
                todo!("Push ParsedErrors")
            }
        }
    }
}

fn swift_type_for_type(
    ty: &Type,
    type_pos: TypePosition,
    types: &TypeDeclarations,
    swift_bridge_path: &Path,
    opaque_ref_repr: OpaqueRustRefSwiftRepr,
) -> String {
    if opaque_ref_repr == OpaqueRustRefSwiftRepr::Struct {
        if let Some(opaque) = opaque_rust_struct_ref(ty, types) {
            return opaque.swift_struct_ref_name(types, swift_bridge_path);
        }
    }

    BridgedType::new_with_type(ty, types)
        .unwrap()
        .to_swift_type(type_pos, types, swift_bridge_path)
}

fn swift_return_type_for_bridged_type(
    function: &ParsedExternFn,
    bridged_type: &BridgedType,
    type_pos: TypePosition,
    types: &TypeDeclarations,
    swift_bridge_path: &Path,
    opaque_ref_repr: OpaqueRustRefSwiftRepr,
) -> String {
    if opaque_ref_repr == OpaqueRustRefSwiftRepr::Struct {
        if let Some(opaque) = return_opaque_rust_struct_ref(function, types) {
            return opaque.swift_struct_ref_name(types, swift_bridge_path);
        }
    }

    bridged_type.to_swift_type(type_pos, types, swift_bridge_path)
}

fn convert_swift_expression_to_ffi_type(
    ty: &Type,
    expression: &str,
    type_pos: TypePosition,
    types: &TypeDeclarations,
    _swift_bridge_path: &Path,
    opaque_ref_repr: OpaqueRustRefSwiftRepr,
) -> String {
    if opaque_ref_repr == OpaqueRustRefSwiftRepr::Struct {
        if opaque_rust_struct_ref(ty, types).is_some() {
            return format!("{}.ptr", expression);
        }
    }

    BridgedType::new_with_type(ty, types)
        .unwrap()
        .convert_swift_expression_to_ffi_type(expression, types, type_pos)
}

fn convert_ffi_return_value_to_swift_value(
    function: &ParsedExternFn,
    bridged_type: &BridgedType,
    expression: &str,
    type_pos: TypePosition,
    types: &TypeDeclarations,
    swift_bridge_path: &Path,
    opaque_ref_repr: OpaqueRustRefSwiftRepr,
) -> String {
    if opaque_ref_repr == OpaqueRustRefSwiftRepr::Struct {
        if let Some(opaque) = return_opaque_rust_struct_ref(function, types) {
            return format!(
                "{}(ptr: {})",
                opaque.swift_struct_ref_name(types, swift_bridge_path),
                expression
            );
        }
    }

    bridged_type.convert_ffi_value_to_swift_value(expression, type_pos, types, swift_bridge_path)
}

fn return_opaque_rust_struct_ref(
    function: &ParsedExternFn,
    types: &TypeDeclarations,
) -> Option<OpaqueForeignType> {
    match &function.func.sig.output {
        ReturnType::Default => None,
        ReturnType::Type(_, ty) => opaque_rust_struct_ref(ty, types),
    }
}

fn opaque_rust_struct_ref(ty: &Type, types: &TypeDeclarations) -> Option<OpaqueForeignType> {
    let opaque = OpaqueForeignType::from_type(ty, types)?;

    if opaque.host_lang.is_rust()
        && opaque.reference
        && !opaque.mutable
        && !opaque.has_swift_bridge_copy_annotation
    {
        Some(opaque)
    } else {
        None
    }
}
