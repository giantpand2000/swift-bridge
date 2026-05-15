use crate::parse::parse_extern_mod::function_attributes::FunctionAttributes;
use proc_macro2::{Delimiter, Group, Ident, Literal, TokenStream, TokenTree};
use quote::quote;
use std::collections::HashMap;
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, LitStr, ReturnType, Token, Type, Visibility};

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

    for token in tokens {
        let is_semi = token_is_punct(&token, ';');
        current_item.extend([token]);

        if is_semi {
            let (item, item_changed, rust_name) = normalize_extern_swift_item(current_item)?;
            if let Some(rust_name) = rust_name {
                track_generated_rust_name(&mut generated_rust_names, &rust_name)?;
            }
            changed |= item_changed;
            normalized.extend(item);
            current_item = TokenStream::new();
        }
    }

    if !current_item.is_empty() {
        let (item, item_changed, rust_name) = normalize_extern_swift_item(current_item)?;
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
) -> syn::Result<(TokenStream, bool, Option<Ident>)> {
    if tokens.is_empty() {
        return Ok((tokens, false, None));
    }

    match parse_swift_func_item(tokens.clone()) {
        Ok(swift_func) => {
            let rust_name = swift_func.rust_fn_name()?;
            Ok((
                swift_func.to_rust_foreign_fn(&rust_name),
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
                "multiple Swift `func` declarations generate the Rust function name `{}`; add `#[swift_bridge(rust_name = \"...\")]` to one of them",
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
        TokenTree::Ident(ident) => ident == "func",
        _ => false,
    })
}

fn is_probably_swift_func_macro(tokens: &TokenStream) -> bool {
    let mut iter = tokens.clone().into_iter().peekable();

    while let Some(token) = iter.next() {
        if token_is_ident(&token, "func") {
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

struct SwiftFunc {
    attrs: Vec<Attribute>,
    vis: Visibility,
    leading_asyncness: Option<Token![async]>,
    swift_name: Ident,
    params: Vec<SwiftFuncParam>,
    trailing_asyncness: Option<Token![async]>,
    output: ReturnType,
}

impl SwiftFunc {
    fn rust_fn_name(&self) -> syn::Result<Ident> {
        if let Some(rust_name) = self.rust_name_override()? {
            return lit_str_to_ident(rust_name);
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

    fn to_rust_foreign_fn(&self, rust_name: &Ident) -> TokenStream {
        let attrs = &self.attrs;
        let vis = &self.vis;
        let swift_name = LitStr::new(&self.swift_name.to_string(), self.swift_name.span());
        let asyncness = self.leading_asyncness.or(self.trailing_asyncness);
        let params = self.params.iter().map(SwiftFuncParam::to_rust_param);
        let output = &self.output;

        quote! {
            #[swift_bridge(swift_name = #swift_name)]
            #(#attrs)*
            #vis #asyncness fn #rust_name(#(#params),*) #output;
        }
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

        Self::parse_signature(input, attrs, vis, leading_asyncness, true)
    }
}

impl SwiftFunc {
    fn parse_signature(
        input: ParseStream,
        attrs: Vec<Attribute>,
        vis: Visibility,
        leading_asyncness: Option<Token![async]>,
        parse_semi: bool,
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
        if func_token != "func" {
            return Err(syn::Error::new_spanned(
                func_token,
                r#"expected Swift function macro declaration starting with `func!`"#,
            ));
        }

        input.parse::<Token![!]>()?;

        let content;
        syn::parenthesized!(content in input);
        let leading_asyncness: Option<Token![async]> = content.parse()?;
        let func = SwiftFunc::parse_signature(&content, attrs, vis, leading_asyncness, false)?;
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
                ty: input.parse()?,
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
            ty: input.parse()?,
        })
    }
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
                func!(callCustom(_ value: i32, forKey key: u32) -> u32);
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
            "multiple Swift `func` declarations generate the Rust function name `load_url`"
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
