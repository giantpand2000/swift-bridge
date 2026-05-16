use crate::parse::parse_extern_mod::function_attributes::FunctionAttributes;
use proc_macro2::{Delimiter, Group, Ident, Literal, TokenStream, TokenTree};
use quote::quote;
use std::collections::HashMap;
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::{
    AngleBracketedGenericArguments, Attribute, GenericArgument, LitStr, PathArguments, ReturnType,
    Token, Type, TypePath, Visibility,
};

pub(super) fn normalize_swift_function_syntax(tokens: TokenStream) -> syn::Result<TokenStream> {
    let (tokens, _) = normalize_token_stream(tokens)?;
    Ok(tokens)
}

fn normalize_token_stream(tokens: TokenStream) -> syn::Result<(TokenStream, bool)> {
    let mut normalized = TokenStream::new();
    let mut changed = false;
    let mut iter = tokens.into_iter().peekable();

    while let Some(token) = iter.next() {
        if token_is_ident(&token, "extern") {
            if let Some(TokenTree::Literal(abi_name)) = iter.peek() {
                if literal_is_swift(abi_name) {
                    let abi_name = iter.next().unwrap();
                    if let Some(TokenTree::Group(group)) = iter.peek() {
                        if group.delimiter() == Delimiter::Brace {
                            let group = match iter.next().unwrap() {
                                TokenTree::Group(group) => group,
                                _ => unreachable!(),
                            };
                            normalized.extend([token, abi_name]);
                            let (group, group_changed) = normalize_group_as_extern_swift(group)?;
                            changed |= group_changed;
                            normalized.extend([group]);
                            continue;
                        }
                    }

                    normalized.extend([token, abi_name]);
                    continue;
                }
            }
        }

        let (token, token_changed) = normalize_token_tree(token)?;
        changed |= token_changed;
        normalized.extend([token]);
    }

    Ok((normalized, changed))
}

fn normalize_token_tree(token: TokenTree) -> syn::Result<(TokenTree, bool)> {
    match token {
        TokenTree::Group(group) => {
            let delimiter = group.delimiter();
            let span = group.span();
            let (stream, changed) = normalize_token_stream(group.stream())?;
            if !changed {
                return Ok((TokenTree::Group(group), false));
            }
            let mut group = Group::new(delimiter, stream);
            group.set_span(span);
            Ok((TokenTree::Group(group), true))
        }
        token => Ok((token, false)),
    }
}

fn normalize_group_as_extern_swift(group: Group) -> syn::Result<(TokenTree, bool)> {
    let span = group.span();
    let (stream, changed) = normalize_extern_swift_items(group.stream())?;
    if !changed {
        return Ok((TokenTree::Group(group), false));
    }
    let mut group = Group::new(Delimiter::Brace, stream);
    group.set_span(span);
    Ok((TokenTree::Group(group), true))
}

fn normalize_extern_swift_items(tokens: TokenStream) -> syn::Result<(TokenStream, bool)> {
    let mut normalized = TokenStream::new();
    let mut current_item = TokenStream::new();
    let mut changed = false;
    let mut generated_rust_names = HashMap::new();
    let context = ExternSwiftContext::from_tokens(tokens.clone());

    for token in tokens {
        let is_semi = token_is_punct(&token, ';');
        current_item.extend([token]);

        if is_semi {
            let (item, item_changed, rust_name) =
                normalize_extern_swift_item(current_item, &context)?;
            if let Some(rust_name) = rust_name {
                track_generated_rust_name(&mut generated_rust_names, &rust_name)?;
            }
            changed |= item_changed;
            normalized.extend(item);
            current_item = TokenStream::new();
        }
    }

    if !current_item.is_empty() {
        let (item, item_changed, rust_name) = normalize_extern_swift_item(current_item, &context)?;
        if let Some(rust_name) = rust_name {
            track_generated_rust_name(&mut generated_rust_names, &rust_name)?;
        }
        changed |= item_changed;
        normalized.extend(item);
    }

    Ok((normalized, changed))
}

fn normalize_extern_swift_item(
    tokens: TokenStream,
    context: &ExternSwiftContext,
) -> syn::Result<(TokenStream, bool, Option<Ident>)> {
    if tokens.is_empty() {
        return Ok((tokens, false, None));
    }

    match parse_swift_func_item(tokens.clone()) {
        Ok(swift_func) => {
            let rust_name = swift_func.rust_fn_name()?;
            Ok((
                swift_func.to_rust_foreign_fn(&rust_name, context)?,
                true,
                Some(rust_name),
            ))
        }
        Err(err) => {
            if is_probably_swift_func(&tokens) {
                Err(err)
            } else {
                Ok((tokens, false, None))
            }
        }
    }
}

fn parse_swift_func_item(tokens: TokenStream) -> syn::Result<SwiftFunc> {
    match syn::parse2::<SwiftFunc>(tokens.clone()) {
        Ok(swift_func) => Ok(swift_func),
        Err(func_err) => match syn::parse2::<SwiftFuncMacro>(tokens.clone()) {
            Ok(swift_func_macro) => Ok(swift_func_macro.func),
            Err(macro_err) => {
                if is_probably_swift_func_macro(&tokens) {
                    Err(macro_err)
                } else {
                    Err(func_err)
                }
            }
        },
    }
}

fn track_generated_rust_name(
    generated_rust_names: &mut HashMap<String, Ident>,
    rust_name: &Ident,
) -> syn::Result<()> {
    let rust_name_string = rust_name.to_string();

    if let Some(previous) = generated_rust_names.get(&rust_name_string) {
        let mut err = syn::Error::new_spanned(
            rust_name,
            format!(
                "multiple Swift `func` or `static_func` declarations generate the Rust function name `{}`; add `#[swift_bridge(rust_name = \"...\")]` to one of them",
                rust_name_string
            ),
        );
        err.combine(syn::Error::new_spanned(
            previous,
            "previous Swift `func` declaration generated this Rust function name",
        ));
        return Err(err);
    }

    generated_rust_names.insert(rust_name_string, rust_name.clone());

    Ok(())
}

fn is_probably_swift_func(tokens: &TokenStream) -> bool {
    tokens.clone().into_iter().any(|token| match token {
        TokenTree::Ident(ident) => ident == "func" || ident == "static_func",
        _ => false,
    })
}

fn is_probably_swift_func_macro(tokens: &TokenStream) -> bool {
    let mut iter = tokens.clone().into_iter().peekable();

    while let Some(token) = iter.next() {
        if token_is_ident(&token, "func") || token_is_ident(&token, "static_func") {
            return iter
                .peek()
                .map(|token| token_is_punct(token, '!'))
                .unwrap_or(false);
        }
    }

    false
}

fn token_is_ident(token: &TokenTree, ident: &str) -> bool {
    match token {
        TokenTree::Ident(token_ident) => token_ident == ident,
        _ => false,
    }
}

fn token_is_punct(token: &TokenTree, ch: char) -> bool {
    match token {
        TokenTree::Punct(punct) => punct.as_char() == ch,
        _ => false,
    }
}

fn literal_is_swift(literal: &Literal) -> bool {
    syn::parse2::<LitStr>(quote! { #literal })
        .map(|lit| lit.value() == "Swift")
        .unwrap_or(false)
}

#[derive(Default)]
struct ExternSwiftContext {
    type_name: Option<Ident>,
    type_count: usize,
}

impl ExternSwiftContext {
    fn from_tokens(tokens: TokenStream) -> Self {
        let mut context = Self::default();
        let mut current_item = TokenStream::new();

        for token in tokens {
            let is_semi = token_is_punct(&token, ';');
            current_item.extend([token]);

            if is_semi {
                context.record_type_item(current_item);
                current_item = TokenStream::new();
            }
        }

        if !current_item.is_empty() {
            context.record_type_item(current_item);
        }

        context
    }

    fn record_type_item(&mut self, item: TokenStream) {
        if let Ok(type_item) = syn::parse2::<syn::ForeignItemType>(item) {
            self.type_count += 1;
            if self.type_count == 1 {
                self.type_name = Some(type_item.ident);
            } else {
                self.type_name = None;
            }
        }
    }

    fn single_type_name(&self) -> Option<&Ident> {
        if self.type_count == 1 {
            self.type_name.as_ref()
        } else {
            None
        }
    }
}

struct SwiftFunc {
    attrs: Vec<Attribute>,
    vis: Visibility,
    leading_asyncness: Option<Token![async]>,
    swift_name: Ident,
    params: Vec<SwiftFuncParam>,
    trailing_asyncness: Option<Token![async]>,
    output: ReturnType,
    kind: SwiftFuncKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SwiftFuncKind {
    InstanceOrFreestanding,
    Static,
}

impl SwiftFunc {
    fn rust_fn_name(&self) -> syn::Result<Ident> {
        if let Some(rust_name) = self.rust_name_override()? {
            return lit_str_to_ident(rust_name);
        }

        if self.swift_name == "init" {
            return Ok(Ident::new("new", self.swift_name.span()));
        }

        Ok(Ident::new(
            &to_snake_case(&self.swift_name.to_string()),
            self.swift_name.span(),
        ))
    }

    fn rust_name_override(&self) -> syn::Result<Option<LitStr>> {
        let mut rust_name = None;

        for attr in self
            .attrs
            .iter()
            .filter(|attr| attr.path.is_ident("swift_bridge"))
        {
            let attrs: FunctionAttributes = attr.parse_args()?;
            if attrs.rust_name.is_some() {
                rust_name = attrs.rust_name;
            }
        }

        Ok(rust_name)
    }

    fn to_rust_foreign_fn(
        &self,
        rust_name: &Ident,
        context: &ExternSwiftContext,
    ) -> syn::Result<TokenStream> {
        let attrs = &self.attrs;
        let vis = &self.vis;
        let swift_name = LitStr::new(&self.swift_name.to_string(), self.swift_name.span());
        let asyncness = self.leading_asyncness.or(self.trailing_asyncness);
        let mut params = Vec::new();
        if self.should_infer_instance_receiver(context)? {
            params.push(quote! { &self });
        }
        params.extend(self.params.iter().map(SwiftFuncParam::to_rust_param));
        let output = self.output_tokens(context)?;
        let associated_to = self.inferred_associated_to(context)?;
        let init_attr = self.inferred_init_attr()?;

        Ok(quote! {
            #[swift_bridge(swift_name = #swift_name)]
            #associated_to
            #init_attr
            #(#attrs)*
            #vis #asyncness fn #rust_name(#(#params),*) #output;
        })
    }

    fn should_infer_instance_receiver(&self, context: &ExternSwiftContext) -> syn::Result<bool> {
        Ok(self.kind == SwiftFuncKind::InstanceOrFreestanding
            && context.single_type_name().is_some()
            && !self.is_swift_initializer()?
            && !self.has_associated_to_attr()?)
    }

    fn inferred_associated_to(
        &self,
        context: &ExternSwiftContext,
    ) -> syn::Result<Option<TokenStream>> {
        let is_initializer = self.is_swift_initializer()?;
        if (self.kind != SwiftFuncKind::Static && !is_initializer)
            || self.has_associated_to_attr()?
        {
            return Ok(None);
        }

        let Some(type_name) = context.single_type_name() else {
            if is_initializer {
                return Ok(None);
            }
            return Err(syn::Error::new_spanned(
                &self.swift_name,
                "`static_func!` requires exactly one `type` declaration in the extern \"Swift\" block, or an explicit `#[swift_bridge(associated_to = Type)]` attribute",
            ));
        };

        Ok(Some(quote! {
            #[swift_bridge(associated_to = #type_name)]
        }))
    }

    fn output_tokens(&self, context: &ExternSwiftContext) -> syn::Result<TokenStream> {
        if self.is_swift_initializer()? {
            if let ReturnType::Default = self.output {
                if let Some(type_name) = context.single_type_name() {
                    return Ok(quote! { -> #type_name });
                }
            }
        }

        let output = &self.output;
        Ok(quote! { #output })
    }

    fn inferred_init_attr(&self) -> syn::Result<Option<TokenStream>> {
        if self.swift_name == "init" && !self.has_init_attr()? {
            Ok(Some(quote! {
                #[swift_bridge(init)]
            }))
        } else {
            Ok(None)
        }
    }

    fn is_swift_initializer(&self) -> syn::Result<bool> {
        Ok(self.swift_name == "init" || self.has_init_attr()?)
    }

    fn has_init_attr(&self) -> syn::Result<bool> {
        for attr in self
            .attrs
            .iter()
            .filter(|attr| attr.path.is_ident("swift_bridge"))
        {
            let attrs: FunctionAttributes = attr.parse_args()?;
            if attrs.is_swift_initializer {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn has_associated_to_attr(&self) -> syn::Result<bool> {
        for attr in self
            .attrs
            .iter()
            .filter(|attr| attr.path.is_ident("swift_bridge"))
        {
            let attrs: FunctionAttributes = attr.parse_args()?;
            if attrs.associated_to.is_some() {
                return Ok(true);
            }
        }

        Ok(false)
    }
}

fn lit_str_to_ident(lit: LitStr) -> syn::Result<Ident> {
    let mut ident = syn::parse_str::<Ident>(&lit.value()).map_err(|_| {
        syn::Error::new_spanned(&lit, "`rust_name` must be a valid Rust identifier")
    })?;
    ident.set_span(lit.span());
    Ok(ident)
}

impl Parse for SwiftFunc {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        let vis: Visibility = input.parse()?;
        let leading_asyncness: Option<Token![async]> = input.parse()?;

        let func_token = Ident::parse_any(input)?;
        if func_token != "func" {
            return Err(syn::Error::new_spanned(
                func_token,
                r#"expected Swift function declaration starting with `func`"#,
            ));
        }

        Self::parse_signature(
            input,
            attrs,
            vis,
            leading_asyncness,
            true,
            SwiftFuncKind::InstanceOrFreestanding,
        )
    }
}

impl SwiftFunc {
    fn parse_signature(
        input: ParseStream,
        attrs: Vec<Attribute>,
        vis: Visibility,
        leading_asyncness: Option<Token![async]>,
        parse_semi: bool,
        kind: SwiftFuncKind,
    ) -> syn::Result<Self> {
        let swift_name = Ident::parse_any(input)?;

        let content;
        syn::parenthesized!(content in input);
        let params =
            syn::punctuated::Punctuated::<SwiftFuncParam, Token![,]>::parse_terminated(&content)?
                .into_iter()
                .collect();

        let trailing_asyncness: Option<Token![async]> = input.parse()?;
        if leading_asyncness.is_some() && trailing_asyncness.is_some() {
            return Err(syn::Error::new_spanned(
                trailing_asyncness,
                "`async` may only appear once in a Swift function declaration",
            ));
        }

        let output: ReturnType = input.parse()?;
        let output = normalize_swift_return_type(output)?;
        if parse_semi {
            input.parse::<Token![;]>()?;
        }

        Ok(Self {
            attrs,
            vis,
            leading_asyncness,
            swift_name,
            params,
            trailing_asyncness,
            output,
            kind,
        })
    }
}

struct SwiftFuncMacro {
    func: SwiftFunc,
}

impl Parse for SwiftFuncMacro {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        let vis: Visibility = input.parse()?;

        let func_token = Ident::parse_any(input)?;
        let kind = match func_token.to_string().as_str() {
            "func" => SwiftFuncKind::InstanceOrFreestanding,
            "static_func" => SwiftFuncKind::Static,
            _ => {
                return Err(syn::Error::new_spanned(
                    func_token,
                    r#"expected Swift function macro declaration starting with `func!` or `static_func!`"#,
                ))
            }
        };

        input.parse::<Token![!]>()?;

        let content;
        syn::parenthesized!(content in input);
        let leading_asyncness: Option<Token![async]> = content.parse()?;
        let func =
            SwiftFunc::parse_signature(&content, attrs, vis, leading_asyncness, false, kind)?;
        if !content.is_empty() {
            return Err(content.error("unexpected tokens in Swift function macro declaration"));
        }

        input.parse::<Token![;]>()?;

        Ok(Self { func })
    }
}

struct SwiftFuncParam {
    attrs: Vec<Attribute>,
    label: SwiftFuncParamLabel,
    local_name: Ident,
    ty: Type,
}

impl SwiftFuncParam {
    fn to_rust_param(&self) -> TokenStream {
        let attrs = &self.attrs;
        let local_name = Ident::new(
            &to_snake_case(&self.local_name.to_string()),
            self.local_name.span(),
        );
        let ty = &self.ty;

        match &self.label {
            SwiftFuncParamLabel::Default => {
                quote! {
                    #(#attrs)* #local_name: #ty
                }
            }
            SwiftFuncParamLabel::Label(label) => {
                let label = LitStr::new(&label.to_string(), label.span());
                quote! {
                    #(#attrs)* #[swift_bridge(label = #label)] #local_name: #ty
                }
            }
            SwiftFuncParamLabel::Unlabeled(underscore) => {
                let label = LitStr::new("_", underscore.span);
                quote! {
                    #(#attrs)* #[swift_bridge(label = #label)] #local_name: #ty
                }
            }
        }
    }
}

impl Parse for SwiftFuncParam {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;

        let first_name = if input.peek(Token![_]) {
            SwiftFuncParamName::Underscore(input.parse()?)
        } else {
            SwiftFuncParamName::Ident(Ident::parse_any(input)?)
        };

        if input.peek(Token![:]) {
            let local_name = first_name.into_ident()?;
            input.parse::<Token![:]>()?;
            return Ok(Self {
                attrs,
                label: SwiftFuncParamLabel::Default,
                local_name,
                ty: parse_swift_type(input)?,
            });
        }

        let local_name = Ident::parse_any(input)?;
        input.parse::<Token![:]>()?;

        let label = match first_name {
            SwiftFuncParamName::Ident(label) => SwiftFuncParamLabel::Label(label),
            SwiftFuncParamName::Underscore(underscore) => {
                SwiftFuncParamLabel::Unlabeled(underscore)
            }
        };

        Ok(Self {
            attrs,
            label,
            local_name,
            ty: parse_swift_type(input)?,
        })
    }
}

fn parse_swift_type(input: ParseStream) -> syn::Result<Type> {
    let ty: Type = input.parse()?;
    normalize_swift_type(&ty)
}

fn normalize_swift_return_type(output: ReturnType) -> syn::Result<ReturnType> {
    match output {
        ReturnType::Default => Ok(ReturnType::Default),
        ReturnType::Type(arrow, ty) => Ok(ReturnType::Type(
            arrow,
            Box::new(normalize_swift_type(&ty)?),
        )),
    }
}

fn normalize_swift_type(ty: &Type) -> syn::Result<Type> {
    match ty {
        Type::Path(type_path) => normalize_swift_type_path(type_path),
        Type::Tuple(tuple) => {
            let mut tuple = tuple.clone();
            for elem in tuple.elems.iter_mut() {
                *elem = normalize_swift_type(elem)?;
            }
            Ok(Type::Tuple(tuple))
        }
        Type::Reference(reference) => {
            let mut reference = reference.clone();
            reference.elem = Box::new(normalize_swift_type(&reference.elem)?);
            Ok(Type::Reference(reference))
        }
        Type::Ptr(ptr) => {
            let mut ptr = ptr.clone();
            ptr.elem = Box::new(normalize_swift_type(&ptr.elem)?);
            Ok(Type::Ptr(ptr))
        }
        Type::Paren(paren) => {
            let mut paren = paren.clone();
            paren.elem = Box::new(normalize_swift_type(&paren.elem)?);
            Ok(Type::Paren(paren))
        }
        Type::Group(group) => {
            let mut group = group.clone();
            group.elem = Box::new(normalize_swift_type(&group.elem)?);
            Ok(Type::Group(group))
        }
        _ => Ok(ty.clone()),
    }
}

fn normalize_swift_type_path(type_path: &TypePath) -> syn::Result<Type> {
    if type_path.qself.is_none()
        && type_path.path.leading_colon.is_none()
        && type_path.path.segments.len() == 1
    {
        let segment = type_path.path.segments.first().unwrap();
        if let Some(mapped) = normalize_single_segment_swift_type(segment)? {
            return Ok(mapped);
        }
    }

    let mut type_path = type_path.clone();
    for segment in type_path.path.segments.iter_mut() {
        segment.arguments = normalize_swift_path_arguments(&segment.arguments)?;
    }
    Ok(Type::Path(type_path))
}

fn normalize_single_segment_swift_type(segment: &syn::PathSegment) -> syn::Result<Option<Type>> {
    let ident = segment.ident.to_string();

    match &segment.arguments {
        PathArguments::None => {
            let tokens = match ident.as_str() {
                "UInt8" => quote! { u8 },
                "Int8" => quote! { i8 },
                "UInt16" => quote! { u16 },
                "Int16" => quote! { i16 },
                "UInt32" => quote! { u32 },
                "Int32" => quote! { i32 },
                "UInt64" => quote! { u64 },
                "Int64" => quote! { i64 },
                "UInt" => quote! { usize },
                "Int" => quote! { isize },
                "Float" => quote! { f32 },
                "Double" => quote! { f64 },
                "Bool" => quote! { bool },
                "Void" => quote! { () },
                "RustString" => quote! { String },
                "RustStringRef" => quote! { &String },
                "RustStringRefMut" => quote! { &mut String },
                "RustStr" => quote! { &str },
                "UnsafeRawPointer" => quote! { *const std::ffi::c_void },
                "UnsafeMutableRawPointer" => quote! { *mut std::ffi::c_void },
                _ => return Ok(None),
            };

            parse_normalized_type(tokens).map(Some)
        }
        PathArguments::AngleBracketed(args) => match ident.as_str() {
            "Optional" => map_swift_generic_type(segment, args, "Option").map(Some),
            "RustVec" => map_swift_generic_type(segment, args, "Vec").map(Some),
            "RustResult" => map_swift_generic_type(segment, args, "Result").map(Some),
            "UnsafePointer" => {
                let inner = single_generic_type(args, "UnsafePointer")?;
                let inner = normalize_swift_type(inner)?;
                parse_normalized_type(quote! { *const #inner }).map(Some)
            }
            "UnsafeMutablePointer" => {
                let inner = single_generic_type(args, "UnsafeMutablePointer")?;
                let inner = normalize_swift_type(inner)?;
                parse_normalized_type(quote! { *mut #inner }).map(Some)
            }
            _ => Ok(None),
        },
        _ => Ok(None),
    }
}

fn map_swift_generic_type(
    segment: &syn::PathSegment,
    args: &AngleBracketedGenericArguments,
    rust_ident: &str,
) -> syn::Result<Type> {
    let rust_ident = Ident::new(rust_ident, segment.ident.span());
    let args = normalize_swift_angle_bracketed_args(args)?;
    parse_normalized_type(quote! { #rust_ident #args })
}

fn normalize_swift_path_arguments(arguments: &PathArguments) -> syn::Result<PathArguments> {
    match arguments {
        PathArguments::None => Ok(PathArguments::None),
        PathArguments::AngleBracketed(args) => Ok(PathArguments::AngleBracketed(
            normalize_swift_angle_bracketed_args(args)?,
        )),
        PathArguments::Parenthesized(args) => {
            let mut args = args.clone();
            for input in args.inputs.iter_mut() {
                *input = normalize_swift_type(input)?;
            }
            args.output = normalize_swift_return_type(args.output)?;
            Ok(PathArguments::Parenthesized(args))
        }
    }
}

fn normalize_swift_angle_bracketed_args(
    args: &AngleBracketedGenericArguments,
) -> syn::Result<AngleBracketedGenericArguments> {
    let mut args = args.clone();

    for arg in args.args.iter_mut() {
        match arg {
            GenericArgument::Type(ty) => {
                *ty = normalize_swift_type(ty)?;
            }
            GenericArgument::Binding(binding) => {
                binding.ty = normalize_swift_type(&binding.ty)?;
            }
            _ => {}
        }
    }

    Ok(args)
}

fn single_generic_type<'a>(
    args: &'a AngleBracketedGenericArguments,
    swift_type_name: &str,
) -> syn::Result<&'a Type> {
    if args.args.len() != 1 {
        return Err(syn::Error::new_spanned(
            args,
            format!("`{swift_type_name}` must have exactly one generic type argument"),
        ));
    }

    match args.args.first().unwrap() {
        GenericArgument::Type(ty) => Ok(ty),
        arg => Err(syn::Error::new_spanned(
            arg,
            format!("`{swift_type_name}` must have a type argument"),
        )),
    }
}

fn parse_normalized_type(tokens: TokenStream) -> syn::Result<Type> {
    syn::parse2(tokens.clone()).map_err(|_| {
        syn::Error::new_spanned(
            tokens,
            "failed to convert Swift type spelling to a Rust type",
        )
    })
}

enum SwiftFuncParamName {
    Ident(Ident),
    Underscore(Token![_]),
}

impl SwiftFuncParamName {
    fn into_ident(self) -> syn::Result<Ident> {
        match self {
            SwiftFuncParamName::Ident(ident) => Ok(ident),
            SwiftFuncParamName::Underscore(underscore) => Err(syn::Error::new_spanned(
                underscore,
                "`_` argument labels must be followed by a local parameter name",
            )),
        }
    }
}

enum SwiftFuncParamLabel {
    Default,
    Label(Ident),
    Unlabeled(Token![_]),
}

fn to_snake_case(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut snake = String::new();

    for (idx, ch) in chars.iter().enumerate() {
        if *ch == '_' {
            snake.push('_');
            continue;
        }

        if ch.is_ascii_uppercase() {
            let prev = idx.checked_sub(1).and_then(|idx| chars.get(idx)).copied();
            let next = chars.get(idx + 1).copied();

            let should_insert_underscore = idx > 0
                && prev.map(|prev| prev != '_').unwrap_or(false)
                && (prev
                    .map(|prev| prev.is_ascii_lowercase() || prev.is_ascii_digit())
                    .unwrap_or(false)
                    || next.map(|next| next.is_ascii_lowercase()).unwrap_or(false));

            if should_insert_underscore {
                snake.push('_');
            }

            snake.push(ch.to_ascii_lowercase());
        } else {
            snake.push(*ch);
        }
    }

    snake
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_tokens_eq;
    use quote::quote;

    #[test]
    fn normalizes_swift_func_to_rust_foreign_fn() {
        let tokens = quote! {
            extern "Swift" {
                func fetchUser(_ id: UserId, includePosts include_posts: bool, limit: u32) -> User;
            }
        };

        let normalized = normalize_swift_function_syntax(tokens).unwrap();

        assert_tokens_eq(
            &normalized,
            &quote! {
                extern "Swift" {
                    #[swift_bridge(swift_name = "fetchUser")]
                    fn fetch_user(
                        #[swift_bridge(label = "_")] id: UserId,
                        #[swift_bridge(label = "includePosts")] include_posts: bool,
                        limit: u32
                    ) -> User;
                }
            },
        );
    }

    #[test]
    fn leaves_rust_foreign_fn_unchanged() {
        let tokens = quote! {
            extern "Swift" {
                fn some_function(arg: u32);
            }
        };

        let normalized = normalize_swift_function_syntax(tokens.clone()).unwrap();

        assert_tokens_eq(&normalized, &tokens);
    }

    #[test]
    fn converts_lower_camel_and_acronyms_to_snake_case() {
        assert_eq!(to_snake_case("fetchUser"), "fetch_user");
        assert_eq!(to_snake_case("URLRequest"), "url_request");
        assert_eq!(to_snake_case("userID"), "user_id");
    }

    #[test]
    fn normalizes_swift_func_with_rust_name_override() {
        let tokens = quote! {
            extern "Swift" {
                #[swift_bridge(rust_name = "call_custom")]
                func callCustom(_ value: i32);
            }
        };

        let normalized = normalize_swift_function_syntax(tokens).unwrap();

        assert_tokens_eq(
            &normalized,
            &quote! {
                extern "Swift" {
                    #[swift_bridge(swift_name = "callCustom")]
                    #[swift_bridge(rust_name = "call_custom")]
                    fn call_custom(#[swift_bridge(label = "_")] value: i32);
                }
            },
        );
    }

    #[test]
    fn normalizes_swift_func_macro_to_rust_foreign_fn() {
        let tokens = quote! {
            extern "Swift" {
                #[swift_bridge(rust_name = "call_custom")]
                func!(callCustom(_ value: Int32, forKey key: UInt32) -> UInt32);
            }
        };

        let normalized = normalize_swift_function_syntax(tokens).unwrap();

        assert_tokens_eq(
            &normalized,
            &quote! {
                extern "Swift" {
                    #[swift_bridge(swift_name = "callCustom")]
                    #[swift_bridge(rust_name = "call_custom")]
                    fn call_custom(
                        #[swift_bridge(label = "_")] value: i32,
                        #[swift_bridge(label = "forKey")] key: u32
                    ) -> u32;
                }
            },
        );
    }

    #[test]
    fn normalizes_func_macro_to_instance_method_when_extern_swift_has_one_type() {
        let tokens = quote! {
            extern "Swift" {
                type Foo;

                func!(bar(_ value: Int64));
            }
        };

        let normalized = normalize_swift_function_syntax(tokens).unwrap();

        assert_tokens_eq(
            &normalized,
            &quote! {
                extern "Swift" {
                    type Foo;

                    #[swift_bridge(swift_name = "bar")]
                    fn bar(&self, #[swift_bridge(label = "_")] value: i64);
                }
            },
        );
    }

    #[test]
    fn normalizes_static_func_macro_to_associated_function() {
        let tokens = quote! {
            extern "Swift" {
                type Foo;

                static_func!(bar(_ value: Int64));
            }
        };

        let normalized = normalize_swift_function_syntax(tokens).unwrap();

        assert_tokens_eq(
            &normalized,
            &quote! {
                extern "Swift" {
                    type Foo;

                    #[swift_bridge(swift_name = "bar")]
                    #[swift_bridge(associated_to = Foo)]
                    fn bar(#[swift_bridge(label = "_")] value: i64);
                }
            },
        );
    }

    #[test]
    fn normalizes_init_attribute_func_macro_as_associated_initializer() {
        let tokens = quote! {
            extern "Swift" {
                type Foo;

                #[swift_bridge(init)]
                func!(bar(_ value: Int64));
            }
        };

        let normalized = normalize_swift_function_syntax(tokens).unwrap();

        assert_tokens_eq(
            &normalized,
            &quote! {
                extern "Swift" {
                    type Foo;

                    #[swift_bridge(swift_name = "bar")]
                    #[swift_bridge(associated_to = Foo)]
                    #[swift_bridge(init)]
                    fn bar(#[swift_bridge(label = "_")] value: i64) -> Foo;
                }
            },
        );
    }

    #[test]
    fn normalizes_init_func_macro_as_initializer() {
        let tokens = quote! {
            extern "Swift" {
                type Foo;

                func!(init(_ value: Int64));
            }
        };

        let normalized = normalize_swift_function_syntax(tokens).unwrap();

        assert_tokens_eq(
            &normalized,
            &quote! {
                extern "Swift" {
                    type Foo;

                    #[swift_bridge(swift_name = "init")]
                    #[swift_bridge(associated_to = Foo)]
                    #[swift_bridge(init)]
                    fn new(#[swift_bridge(label = "_")] value: i64) -> Foo;
                }
            },
        );
    }

    #[test]
    fn normalizes_swift_builtin_types_to_rust_types() {
        let tokens = quote! {
            extern "Swift" {
                func!(
                    setValues(
                        _ u8Value: UInt8,
                        i8Value: Int8,
                        u16Value: UInt16,
                        i16Value: Int16,
                        u32Value: UInt32,
                        i32Value: Int32,
                        u64Value: UInt64,
                        i64Value: Int64,
                        uintValue: UInt,
                        intValue: Int,
                        floatValue: Float,
                        doubleValue: Double,
                        boolValue: Bool
                    ) -> Void
                );
            }
        };

        let normalized = normalize_swift_function_syntax(tokens).unwrap();

        assert_tokens_eq(
            &normalized,
            &quote! {
                extern "Swift" {
                    #[swift_bridge(swift_name = "setValues")]
                    fn set_values(
                        #[swift_bridge(label = "_")] u8_value: u8,
                        i8_value: i8,
                        u16_value: u16,
                        i16_value: i16,
                        u32_value: u32,
                        i32_value: i32,
                        u64_value: u64,
                        i64_value: i64,
                        uint_value: usize,
                        int_value: isize,
                        float_value: f32,
                        double_value: f64,
                        bool_value: bool
                    ) -> ();
                }
            },
        );
    }

    #[test]
    fn normalizes_swift_bridge_generic_types_to_rust_types() {
        let tokens = quote! {
            extern "Swift" {
                func!(
                    loadValues(
                        _ values: RustVec<UInt32>,
                        maybeEnabled: Optional<Bool>,
                        result: RustResult<String, SomeError>
                    ) -> Optional<RustVec<Int32>>
                );
            }
        };

        let normalized = normalize_swift_function_syntax(tokens).unwrap();

        assert_tokens_eq(
            &normalized,
            &quote! {
                extern "Swift" {
                    #[swift_bridge(swift_name = "loadValues")]
                    fn load_values(
                        #[swift_bridge(label = "_")] values: Vec<u32>,
                        maybe_enabled: Option<bool>,
                        result: Result<String, SomeError>
                    ) -> Option<Vec<i32> >;
                }
            },
        );
    }

    #[test]
    fn normalizes_swift_pointer_types_to_rust_types() {
        let tokens = quote! {
            extern "Swift" {
                func!(
                    readPointers(
                        _ bytes: UnsafePointer<UInt8>,
                        output: UnsafeMutablePointer<Int32>,
                        raw: UnsafeRawPointer,
                        mutableRaw: UnsafeMutableRawPointer
                    )
                );
            }
        };

        let normalized = normalize_swift_function_syntax(tokens).unwrap();

        assert_tokens_eq(
            &normalized,
            &quote! {
                extern "Swift" {
                    #[swift_bridge(swift_name = "readPointers")]
                    fn read_pointers(
                        #[swift_bridge(label = "_")] bytes: *const u8,
                        output: *mut i32,
                        raw: *const std::ffi::c_void,
                        mutable_raw: *mut std::ffi::c_void
                    );
                }
            },
        );
    }

    #[test]
    fn normalizes_async_swift_func_macro_to_rust_foreign_fn() {
        let tokens = quote! {
            extern "Swift" {
                func!(async fetchUser(_ id: UserId) -> User);
            }
        };

        let normalized = normalize_swift_function_syntax(tokens).unwrap();

        assert_tokens_eq(
            &normalized,
            &quote! {
                extern "Swift" {
                    #[swift_bridge(swift_name = "fetchUser")]
                    async fn fetch_user(#[swift_bridge(label = "_")] id: UserId) -> User;
                }
            },
        );
    }

    #[test]
    fn errors_when_swift_funcs_generate_duplicate_rust_name() {
        let tokens = quote! {
            extern "Swift" {
                func loadURL();
                func loadUrl();
            }
        };

        let err = normalize_swift_function_syntax(tokens).unwrap_err();
        let err = err.to_string();

        assert!(err.contains(
            "multiple Swift `func` or `static_func` declarations generate the Rust function name `load_url`"
        ));
    }

    #[test]
    fn rust_name_override_resolves_auto_name_conflict() {
        let tokens = quote! {
            extern "Swift" {
                func loadURL();
                #[swift_bridge(rust_name = "load_url_2")]
                func loadUrl();
            }
        };

        let normalized = normalize_swift_function_syntax(tokens).unwrap();

        assert_tokens_eq(
            &normalized,
            &quote! {
                extern "Swift" {
                    #[swift_bridge(swift_name = "loadURL")]
                    fn load_url();
                    #[swift_bridge(swift_name = "loadUrl")]
                    #[swift_bridge(rust_name = "load_url_2")]
                    fn load_url_2();
                }
            },
        );
    }
}
