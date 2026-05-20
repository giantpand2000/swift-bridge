use crate::bridged_type::{BridgeableType, BridgedType, StdLibType, TypePosition};
use crate::parse::HostLang;
use crate::parsed_extern_fn::SwiftFuncGenerics;
use crate::TypeDeclarations;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{quote, ToTokens};
use std::collections::HashSet;
use std::str::FromStr;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Path, Type};

/// Box<dyn FnOnce(A, B, C) -> ()>, Box<dyn Fn(A, B, C) -> ()>,
/// or Arc<dyn Fn(A, B, C) -> ()>.
#[derive(Debug)]
pub(crate) struct BridgeableBoxedFnOnce {
    pub owner: BridgeableFnOwner,
    pub trait_kind: BridgeableFnTrait,
    pub trait_bounds: Vec<BridgeableFnTraitBound>,
    /// The functions parameters.
    pub params: Vec<BridgedType>,
    /// The functions return type.
    pub ret: Box<BridgedType>,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum BridgeableFnOwner {
    Box,
    Arc,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum BridgeableFnTrait {
    FnOnce,
    Fn,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum BridgeableFnTraitBound {
    Send,
    Sync,
    Static,
}

impl BridgeableFnTraitBound {
    fn to_token_stream(&self) -> TokenStream {
        match self {
            BridgeableFnTraitBound::Send => quote! { Send },
            BridgeableFnTraitBound::Sync => quote! { Sync },
            BridgeableFnTraitBound::Static => quote! { 'static },
        }
    }
}

/// example: Vec<SomeType, AnotherType, u32>
pub(crate) struct FunctionArguments(pub Vec<Type>);
impl Parse for FunctionArguments {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let args: Punctuated<Type, syn::Token![,]> = Punctuated::parse_terminated(input)?;
        Ok(Self(args.into_iter().collect()))
    }
}

impl BridgeableBoxedFnOnce {
    pub fn does_not_have_params_or_return(&self) -> bool {
        self.params.is_empty() && self.ret.is_null()
    }

    pub fn uses_core_no_args_no_return_support(&self) -> bool {
        self.owner == BridgeableFnOwner::Box
            && self.trait_kind == BridgeableFnTrait::FnOnce
            && self.does_not_have_params_or_return()
    }

    pub fn is_fn_once(&self) -> bool {
        self.trait_kind == BridgeableFnTrait::FnOnce
    }

    fn trait_ident(&self) -> Ident {
        let trait_name = match self.trait_kind {
            BridgeableFnTrait::FnOnce => "FnOnce",
            BridgeableFnTrait::Fn => "Fn",
        };

        Ident::new(trait_name, Span::call_site())
    }

    fn rust_callback_pointer_type(&self, types: &TypeDeclarations) -> TokenStream {
        let params: Vec<TokenStream> = self
            .params
            .iter()
            .map(|a| a.to_rust_type_path(types))
            .collect();
        let ret = &self.ret.to_rust_type_path(types);
        let trait_ident = self.trait_ident();
        let trait_bounds = self.trait_bound_tokens();

        match self.owner {
            BridgeableFnOwner::Box => {
                quote! {
                    *mut Box<dyn #trait_ident(#(#params),*) -> #ret #( + #trait_bounds )*>
                }
            }
            BridgeableFnOwner::Arc => {
                quote! {
                    *mut std::sync::Arc<dyn #trait_ident(#(#params),*) -> #ret #( + #trait_bounds )*>
                }
            }
        }
    }

    /// Box<dyn FnOnce(A, B) -> C>
    pub fn to_rust_type_path(&self, types: &TypeDeclarations) -> TokenStream {
        let args: Vec<TokenStream> = self
            .params
            .iter()
            .map(|a| a.to_rust_type_path(types))
            .collect();
        let ret = &self.ret.to_rust_type_path(types);
        let trait_ident = self.trait_ident();
        let trait_bounds = self.trait_bound_tokens();

        match self.owner {
            BridgeableFnOwner::Box => {
                quote! {
                    Box<dyn #trait_ident(#(#args),*) -> #ret #( + #trait_bounds )*>
                }
            }
            BridgeableFnOwner::Arc => {
                quote! {
                    std::sync::Arc<dyn #trait_ident(#(#args),*) -> #ret #( + #trait_bounds )*>
                }
            }
        }
    }

    pub fn convert_rust_value_to_ffi_compatible_value(
        &self,
        expression: &TokenStream,
        types: &TypeDeclarations,
    ) -> TokenStream {
        let args: Vec<TokenStream> = self
            .params
            .iter()
            .map(|a| a.to_rust_type_path(types))
            .collect();
        let ret = &self.ret.to_rust_type_path(types);

        let trait_ident = self.trait_ident();
        let trait_bounds = self.trait_bound_tokens();

        match self.owner {
            BridgeableFnOwner::Box => {
                quote! {
                    Box::into_raw(Box::new(#expression)) as *mut Box<dyn #trait_ident(#(#args),*) -> #ret #( + #trait_bounds )*>
                }
            }
            BridgeableFnOwner::Arc => {
                quote! {
                    Box::into_raw(Box::new(#expression)) as *mut std::sync::Arc<dyn #trait_ident(#(#args),*) -> #ret #( + #trait_bounds )*>
                }
            }
        }
    }

    fn trait_bound_tokens(&self) -> Vec<TokenStream> {
        self.trait_bounds
            .iter()
            .map(|bound| bound.to_token_stream())
            .collect()
    }

    pub fn to_ffi_compatible_rust_type(&self, types: &TypeDeclarations) -> TokenStream {
        self.rust_callback_pointer_type(types)
    }

    pub fn to_swift_to_rust_ffi_compatible_rust_type(&self) -> TokenStream {
        quote! {
            *mut std::ffi::c_void
        }
    }

    /// Returns each of the parameters as an FFI friendly type.
    ///
    /// For example, `Box<dyn FnOnce(u8, SomeType)>` would give us:
    /// arg0: u8, arg1: *mut super::SomeType
    pub fn params_to_ffi_compatible_rust_types(
        &self,
        swift_bridge_path: &Path,
        types: &TypeDeclarations,
    ) -> Vec<TokenStream> {
        self.params
            .iter()
            .enumerate()
            .map(|(idx, ty)| {
                let param_name = Ident::new(&format!("arg{}", idx), Span::call_site());
                let param_ty = ty.to_ffi_compatible_rust_type(swift_bridge_path, types);

                quote! {
                    #param_name: #param_ty
                }
            })
            .collect()
    }

    /// arg0: UInt8, arg1: SomeType, ...
    pub fn params_to_swift_types(
        &self,
        types: &TypeDeclarations,
        swift_bridge_path: &Path,
    ) -> String {
        self.params
            .iter()
            .enumerate()
            .map(|(idx, ty)| {
                let ty = ty.to_swift_type(
                    TypePosition::FnArg(HostLang::Rust, idx),
                    types,
                    swift_bridge_path,
                );

                format!("_ arg{idx}: {ty}")
            })
            .collect::<Vec<String>>()
            .join(", ")
    }

    /// Box<dyn FnOnce(u8, SomeRustType)> becomes:
    /// uint8_t arg0, *void arg1
    pub fn params_to_c_types(&self, types: &TypeDeclarations) -> String {
        self.params
            .iter()
            .enumerate()
            .map(|(idx, ty)| {
                let ty = ty.to_c(types);

                format!("{ty} arg{idx}")
            })
            .collect::<Vec<String>>()
            .join(", ")
    }

    /// Returns each `arg0, arg1, ... argN`.
    ///
    /// For example, `Box<dyn FnOnce(u8, SomeType)>` would give us:
    /// arg0, unsafe { *Box::from_raw(arg1) }
    pub fn to_rust_call_args(
        &self,
        swift_bridge_path: &Path,
        types: &TypeDeclarations,
    ) -> Vec<TokenStream> {
        self.params
            .iter()
            .enumerate()
            .map(|(idx, ty)| {
                let arg_name = Ident::new(&format!("arg{}", idx), Span::call_site());
                ty.convert_ffi_expression_to_rust_type(
                    &arg_name.to_token_stream(),
                    arg_name.span(),
                    swift_bridge_path,
                    types,
                )
            })
            .collect()
    }

    /// Returns each `arg0, arg1, ... argN`.
    ///
    /// For example, `Box<dyn FnOnce(u8, SomeType)>` would give us:
    /// "arg0, arg1"
    pub fn to_swift_call_args(&self) -> String {
        self.params
            .iter()
            .enumerate()
            .map(|(idx, _ty)| format!("arg{}", idx))
            .collect::<Vec<String>>()
            .join(", ")
    }

    /// Box<dyn FnOnce(u8, SomeType)> would become:
    /// ", arg0, { arg1.isOwned = false; arg1 }()"
    pub fn to_from_swift_to_rust_ffi_call_args(&self, types: &TypeDeclarations) -> String {
        let mut args = "".to_string();

        if self.params.is_empty() {
            return args;
        }

        for (idx, ty) in self.params.iter().enumerate() {
            let arg_name = format!("arg{}", idx);
            args += &format!(
                ", {}",
                ty.convert_swift_expression_to_ffi_type(
                    &arg_name,
                    types,
                    TypePosition::FnArg(HostLang::Rust, idx)
                )
            );
        }

        args
    }

    pub fn to_swift_type(
        &self,
        type_pos: TypePosition,
        types: &TypeDeclarations,
        swift_bridge_path: &Path,
    ) -> String {
        match type_pos {
            TypePosition::FnArg(host_lang, _) => {
                if host_lang.is_rust() {
                    self.to_swift_closure_type(true, types, swift_bridge_path)
                } else {
                    "UnsafeMutableRawPointer".to_string()
                }
            }
            TypePosition::FnReturn(host_lang) => {
                if host_lang.is_rust() {
                    self.to_swift_closure_type(false, types, swift_bridge_path)
                } else {
                    "UnsafeMutableRawPointer".to_string()
                }
            }
            _ => "UnsafeMutableRawPointer".to_string(),
        }
    }

    pub fn to_swift_closure_type(
        &self,
        escaping: bool,
        types: &TypeDeclarations,
        swift_bridge_path: &Path,
    ) -> String {
        let params = self
            .params
            .iter()
            .enumerate()
            .map(|(idx, ty)| {
                ty.to_swift_type(
                    TypePosition::FnArg(HostLang::Rust, idx),
                    types,
                    swift_bridge_path,
                )
            })
            .collect::<Vec<String>>()
            .join(", ");
        let ret = self.ret.to_swift_type(
            TypePosition::FnReturn(HostLang::Rust),
            types,
            swift_bridge_path,
        );
        let escaping = if escaping { "@escaping " } else { "" };

        format!("{escaping}({params}) -> {ret}")
    }

    pub fn convert_ffi_value_to_swift_value(&self, type_pos: TypePosition) -> String {
        match type_pos {
            TypePosition::FnArg(_, param_idx) => {
                if self.does_not_have_params_or_return() {
                    format!("{{ cb{param_idx}.call() }}")
                } else if self.params.len() > 0 {
                    let args = self.to_swift_call_args();
                    format!("{{ {args} in cb{param_idx}.call({args}) }}")
                } else {
                    format!("{{ cb{param_idx}.call() }}")
                }
            }
            _ => todo!("Not yet supported"),
        }
    }

    pub fn rust_to_swift_callback_class_name(
        &self,
        maybe_associated_ty: &str,
        fn_name: &str,
        idx: usize,
    ) -> String {
        if self.trait_kind == BridgeableFnTrait::FnOnce {
            format!("__private__RustFnOnceCallback{maybe_associated_ty}${fn_name}$param{idx}")
        } else {
            format!("__private__RustFnCallback{maybe_associated_ty}${fn_name}$param{idx}")
        }
    }

    pub fn swift_to_rust_callback_class_name(
        &self,
        maybe_associated_ty: &str,
        fn_name: &str,
        idx: usize,
    ) -> String {
        format!("__private__SwiftFnCallback{maybe_associated_ty}${fn_name}$param{idx}")
    }

    pub fn params_to_swift_ffi_types(
        &self,
        types: &TypeDeclarations,
        swift_bridge_path: &Path,
    ) -> String {
        self.params
            .iter()
            .enumerate()
            .map(|(idx, ty)| {
                let ty = ty.to_swift_type(
                    TypePosition::FnArg(HostLang::Swift, idx),
                    types,
                    swift_bridge_path,
                );

                format!("_ arg{idx}: {ty}")
            })
            .collect::<Vec<String>>()
            .join(", ")
    }

    pub fn params_to_ffi_compatible_rust_arg_names_and_types(
        &self,
        swift_bridge_path: &Path,
        types: &TypeDeclarations,
    ) -> Vec<TokenStream> {
        self.params
            .iter()
            .enumerate()
            .map(|(idx, ty)| {
                let arg_name = Ident::new(&format!("arg{}", idx), Span::call_site());
                let arg_ty = ty.to_ffi_compatible_rust_type(swift_bridge_path, types);

                quote! {
                    #arg_name: #arg_ty
                }
            })
            .collect()
    }

    pub fn swift_callback_params_from_ffi_conversions(
        &self,
        types: &TypeDeclarations,
        swift_bridge_path: &Path,
    ) -> String {
        self.params
            .iter()
            .enumerate()
            .map(|(idx, ty)| {
                let arg_name = format!("arg{}", idx);
                let arg = ty.convert_ffi_expression_to_swift_type(
                    &arg_name,
                    TypePosition::FnArg(HostLang::Rust, idx),
                    types,
                    swift_bridge_path,
                );

                format!("let {arg_name} = {arg}")
            })
            .collect::<Vec<String>>()
            .join("\n    ")
    }

    pub fn swift_callback_call_args(&self) -> String {
        self.params
            .iter()
            .enumerate()
            .map(|(idx, _)| format!("arg{idx}"))
            .collect::<Vec<String>>()
            .join(", ")
    }

    pub fn swift_callback_return(&self, expression: &str, types: &TypeDeclarations) -> String {
        if self.ret.is_null() {
            expression.to_string()
        } else {
            self.ret.convert_swift_expression_to_ffi_type(
                expression,
                types,
                TypePosition::FnReturn(HostLang::Swift),
            )
        }
    }

    pub fn to_rust_closure_expression_from_swift_ffi_value(
        &self,
        expression: &TokenStream,
        call_link_name: &str,
        free_link_name: &str,
        call_fn_ident: &Ident,
        free_fn_ident: &Ident,
        idx: usize,
        swift_bridge_path: &Path,
        types: &TypeDeclarations,
    ) -> TokenStream {
        let guard_ident = Ident::new(
            &format!("__SwiftBridgeCallbackGuard{idx}"),
            Span::call_site(),
        );
        let callback_ident =
            Ident::new(&format!("__swift_bridge_callback{idx}"), Span::call_site());

        let params =
            self.params_to_ffi_compatible_rust_arg_names_and_types(swift_bridge_path, types);
        let arg_idents: Vec<Ident> = self
            .params
            .iter()
            .enumerate()
            .map(|(idx, _)| Ident::new(&format!("arg{}", idx), Span::call_site()))
            .collect();
        let ffi_args: Vec<TokenStream> = self
            .params
            .iter()
            .enumerate()
            .map(|(idx, ty)| {
                let arg = Ident::new(&format!("arg{}", idx), Span::call_site()).to_token_stream();
                ty.convert_rust_expression_to_ffi_type(
                    &arg,
                    swift_bridge_path,
                    types,
                    Span::call_site(),
                )
            })
            .collect();
        let maybe_params = if params.is_empty() {
            quote! {}
        } else {
            quote! { , #(#params),* }
        };
        let maybe_args = if ffi_args.is_empty() {
            quote! {}
        } else {
            quote! { , #(#ffi_args),* }
        };
        let maybe_ret = if self.ret.is_null() {
            quote! {}
        } else {
            let ret = self
                .ret
                .to_ffi_compatible_rust_type(swift_bridge_path, types);
            quote! { -> #ret }
        };
        let call_swift = if self.ret.is_null() {
            quote! {
                unsafe { #call_fn_ident(#callback_ident.ptr() #maybe_args) };
            }
        } else {
            let ret_value = self.ret.convert_ffi_expression_to_rust_type(
                &quote! { __swift_bridge_ret },
                Span::call_site(),
                swift_bridge_path,
                types,
            );
            quote! {
                let __swift_bridge_ret = unsafe { #call_fn_ident(#callback_ident.ptr() #maybe_args) };
                #ret_value
            }
        };
        let closure_ty = self.to_rust_type_path(types);
        let closure = quote! {
            move |#(#arg_idents),*| {
                #call_swift
            }
        };
        let closure = match self.owner {
            BridgeableFnOwner::Box => quote! { Box::new(#closure) as #closure_ty },
            BridgeableFnOwner::Arc => quote! { std::sync::Arc::new(#closure) as #closure_ty },
        };
        let maybe_send_impl = if self.trait_bounds.contains(&BridgeableFnTraitBound::Send) {
            quote! { unsafe impl Send for #guard_ident {} }
        } else {
            quote! {}
        };
        let maybe_sync_impl = if self.trait_bounds.contains(&BridgeableFnTraitBound::Sync) {
            quote! { unsafe impl Sync for #guard_ident {} }
        } else {
            quote! {}
        };

        quote! {
            {
                extern "C" {
                    #[link_name = #call_link_name]
                    fn #call_fn_ident(
                        callback: *mut std::ffi::c_void
                        #maybe_params
                    ) #maybe_ret;

                    #[link_name = #free_link_name]
                    fn #free_fn_ident(callback: *mut std::ffi::c_void);
                }

                struct #guard_ident {
                    ptr: *mut std::ffi::c_void,
                }

                #maybe_send_impl
                #maybe_sync_impl

                impl #guard_ident {
                    fn ptr(&self) -> *mut std::ffi::c_void {
                        self.ptr
                    }
                }

                impl Drop for #guard_ident {
                    fn drop(&mut self) {
                        unsafe { #free_fn_ident(self.ptr) }
                    }
                }

                let #callback_ident = #guard_ident { ptr: #expression };
                #closure
            }
        }
    }

    /// Generate the generate bounds for the Swift side.
    /// For example:
    /// "<GenericRustString: IntoRustString>"
    pub fn maybe_swift_generics(&self, types: &TypeDeclarations) -> String {
        let mut maybe_generics = HashSet::new();

        for bridged_arg in &self.params {
            if bridged_arg.contains_owned_string_recursive(types) {
                maybe_generics.insert(SwiftFuncGenerics::String);
            } else if bridged_arg.contains_ref_string_recursive() {
                maybe_generics.insert(SwiftFuncGenerics::Str);
            }
        }

        let maybe_generics = if maybe_generics.is_empty() {
            "".to_string()
        } else {
            let mut m = vec![];

            let generics: Vec<SwiftFuncGenerics> = maybe_generics.into_iter().collect();

            for generic in generics {
                m.push(generic.as_bound())
            }

            format!("<{}>", m.join(", "))
        };

        maybe_generics
    }
}

impl BridgeableBoxedFnOnce {
    pub fn from_str_tokens(string: &str, types: &TypeDeclarations) -> Option<Self> {
        let (owner, trait_kind, signature) = parse_callback_prefix(string)?;

        // ( A , B , C ) -> D >
        //   OR
        // ( A , B , C ) >
        let open_parens = signature.find("(").unwrap();
        let closing_parens = signature.find(")").unwrap();
        // A, B, C
        let args = &signature[open_parens + 1..closing_parens];

        let signature_end = trim_callback_signature_end(&signature[closing_parens + 1..]);
        let (ret, trait_bounds) = parse_return_and_bounds(signature_end)?;

        let trait_bounds = parse_callback_trait_bounds(trait_bounds)?;

        let args = TokenStream::from_str(args).unwrap();
        let args: FunctionArguments = syn::parse2(args).unwrap();

        let ret = if let Some(ret) = ret {
            // Parse out the comma in:
            //   Box<dyn FnOnce() -> (),>
            let ret = ret.trim_end_matches(",");

            let ret = syn::parse2::<Type>(TokenStream::from_str(ret).unwrap()).unwrap();
            BridgedType::new_with_type(&ret, types)?
        } else {
            BridgedType::StdLib(StdLibType::Null)
        };

        let mut args_bridged_tys = Vec::with_capacity(args.0.len());
        for arg in args.0 {
            args_bridged_tys.push(BridgedType::new_with_type(&arg, types)?);
        }

        return Some(BridgeableBoxedFnOnce {
            owner,
            trait_kind,
            trait_bounds,
            params: args_bridged_tys,
            ret: Box::new(ret),
        });
    }
}

fn trim_callback_signature_end(mut string: &str) -> &str {
    loop {
        string = string.trim();

        if let Some(trimmed) = string.strip_suffix(">") {
            string = trimmed;
            continue;
        }

        if let Some(trimmed) = string.strip_suffix(",") {
            string = trimmed;
            continue;
        }

        return string.trim();
    }
}

fn parse_return_and_bounds(signature_end: &str) -> Option<(Option<&str>, &str)> {
    let signature_end = signature_end.trim();

    if signature_end.is_empty() {
        return Some((None, ""));
    }

    if let Some(bounds) = signature_end.strip_prefix("+") {
        return Some((None, bounds));
    }

    let ret = signature_end.strip_prefix("->")?.trim();
    if let Some((ret, bounds)) = ret.split_once("+") {
        Some((Some(ret.trim()), bounds))
    } else {
        Some((Some(ret), ""))
    }
}

fn parse_callback_trait_bounds(bounds: &str) -> Option<Vec<BridgeableFnTraitBound>> {
    let bounds = bounds.trim();
    if bounds.is_empty() {
        return Some(vec![]);
    }

    let mut parsed = vec![];
    for bound in bounds.split("+") {
        let bound = bound.trim();
        if bound.is_empty() {
            continue;
        }

        match bound {
            "Send" => parsed.push(BridgeableFnTraitBound::Send),
            "Sync" => parsed.push(BridgeableFnTraitBound::Sync),
            "'static" => parsed.push(BridgeableFnTraitBound::Static),
            _ => return None,
        }
    }

    Some(parsed)
}

fn parse_callback_prefix(string: &str) -> Option<(BridgeableFnOwner, BridgeableFnTrait, &str)> {
    for (prefix, owner, trait_kind) in [
        (
            "Box < dyn FnOnce",
            BridgeableFnOwner::Box,
            BridgeableFnTrait::FnOnce,
        ),
        (
            "Box < dyn Fn",
            BridgeableFnOwner::Box,
            BridgeableFnTrait::Fn,
        ),
        (
            "Arc < dyn Fn",
            BridgeableFnOwner::Arc,
            BridgeableFnTrait::Fn,
        ),
        (
            "std :: sync :: Arc < dyn Fn",
            BridgeableFnOwner::Arc,
            BridgeableFnTrait::Fn,
        ),
    ] {
        if callback_prefix_matches(string, prefix) {
            return Some((owner, trait_kind, &string[prefix.len()..]));
        }
    }

    None
}

fn callback_prefix_matches(string: &str, prefix: &str) -> bool {
    if !string.starts_with(prefix) {
        return false;
    }

    match string[prefix.len()..].chars().next() {
        Some('(') | Some(' ') => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that we can parse a boxed fn once that does not have an `->` token
    #[test]
    fn boxed_fn_once_from_string_no_arrow() {
        let tokens = quote! {Box<dyn FnOnce()>}.to_token_stream().to_string();

        assert!(
            BridgeableBoxedFnOnce::from_str_tokens(&tokens, &TypeDeclarations::default())
                .unwrap()
                .ret
                .is_null()
        );
    }

    /// Verify that we can parse a boxed fn once that has an `->` token
    #[test]
    fn boxed_fn_once_from_string_with_arrow() {
        let tokens = quote! {Box<dyn FnOnce() -> u8>}
            .to_token_stream()
            .to_string();

        assert!(matches!(
            *BridgeableBoxedFnOnce::from_str_tokens(&tokens, &TypeDeclarations::default())
                .unwrap()
                .ret,
            BridgedType::StdLib(StdLibType::U8)
        ));
    }

    #[test]
    fn boxed_fn_from_string_with_arrow() {
        let tokens = quote! {Box<dyn Fn(u8) -> u8>}.to_token_stream().to_string();
        let parsed =
            BridgeableBoxedFnOnce::from_str_tokens(&tokens, &TypeDeclarations::default()).unwrap();

        assert_eq!(parsed.owner, BridgeableFnOwner::Box);
        assert_eq!(parsed.trait_kind, BridgeableFnTrait::Fn);
        assert_eq!(parsed.params.len(), 1);
    }

    #[test]
    fn boxed_fnonce_from_string_with_send_sync_static_bounds() {
        let tokens = quote! {Box<dyn FnOnce(u8) -> u8 + Send + Sync + 'static>}
            .to_token_stream()
            .to_string();
        let parsed =
            BridgeableBoxedFnOnce::from_str_tokens(&tokens, &TypeDeclarations::default()).unwrap();

        assert_eq!(parsed.owner, BridgeableFnOwner::Box);
        assert_eq!(parsed.trait_kind, BridgeableFnTrait::FnOnce);
        assert_eq!(
            parsed.trait_bounds,
            vec![
                BridgeableFnTraitBound::Send,
                BridgeableFnTraitBound::Sync,
                BridgeableFnTraitBound::Static,
            ]
        );
    }

    #[test]
    fn arc_fn_from_string_with_arrow() {
        let tokens = quote! {Arc<dyn Fn(u8) -> u8>}.to_token_stream().to_string();
        let parsed =
            BridgeableBoxedFnOnce::from_str_tokens(&tokens, &TypeDeclarations::default()).unwrap();

        assert_eq!(parsed.owner, BridgeableFnOwner::Arc);
        assert_eq!(parsed.trait_kind, BridgeableFnTrait::Fn);
        assert_eq!(parsed.params.len(), 1);
    }

    #[test]
    fn arc_fn_from_string_with_send_sync_static_bounds() {
        let tokens = quote! {Arc<dyn Fn(u8) -> u8 + Send + Sync + 'static>}
            .to_token_stream()
            .to_string();
        let parsed =
            BridgeableBoxedFnOnce::from_str_tokens(&tokens, &TypeDeclarations::default()).unwrap();

        assert_eq!(parsed.owner, BridgeableFnOwner::Arc);
        assert_eq!(parsed.trait_kind, BridgeableFnTrait::Fn);
        assert_eq!(
            parsed.trait_bounds,
            vec![
                BridgeableFnTraitBound::Send,
                BridgeableFnTraitBound::Sync,
                BridgeableFnTraitBound::Static,
            ]
        );
    }

    #[test]
    fn arc_fnonce_from_string_is_not_supported() {
        let tokens = quote! {Arc<dyn FnOnce(u8) -> u8>}
            .to_token_stream()
            .to_string();

        assert!(
            BridgeableBoxedFnOnce::from_str_tokens(&tokens, &TypeDeclarations::default()).is_none()
        );
    }

    /// Verify that we can parse a boxed fn once that explicitly returns the null type.
    #[test]
    fn boxed_fn_once_from_string_returns_null() {
        let tokens = quote! {Box<dyn FnOnce() -> ()>}
            .to_token_stream()
            .to_string();

        assert!(
            BridgeableBoxedFnOnce::from_str_tokens(&tokens, &TypeDeclarations::default())
                .unwrap()
                .ret
                .is_null(),
        );
    }

    /// Verify that we can parse a boxed fn that does not have a space before the argument
    /// parentheses.
    /// Not sure what leads to this case.. but if we don't handle it the test suite will fail so
    /// we can always figure out what leads to not having the space before the parens in the future.
    #[test]
    fn no_space_before_arg_parens() {
        let tokens = "Box < dyn FnOnce() -> () >";

        assert!(
            BridgeableBoxedFnOnce::from_str_tokens(tokens, &TypeDeclarations::default())
                .unwrap()
                .ret
                .is_null(),
        );
    }

    /// Verify that we can parse a boxed fn that has a comma after the FnOnce.
    /// rustfmt adds a trailing comma when it puts a long function signature on its own line.
    #[test]
    fn comma_after_fn_once() {
        let tests = vec![
            quote! {Box<dyn FnOnce(),>},
            quote! {Box<dyn FnOnce() -> (),>},
            quote! {
                Box<
                    dyn FnOnce(Result<String, String>),
                >
            },
        ];

        for test in tests {
            let tokens = test.to_token_stream().to_string();

            assert!(
                BridgeableBoxedFnOnce::from_str_tokens(&tokens, &TypeDeclarations::default())
                    .unwrap()
                    .ret
                    .is_null(),
            );
        }
    }
}
