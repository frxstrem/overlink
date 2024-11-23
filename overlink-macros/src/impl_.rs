use std::ffi::CString;

use proc_macro2::Span;
use quote::{quote, ToTokens};
use syn::{
    parse::{Parse, ParseStream},
    parse_quote,
    punctuated::Punctuated,
    spanned::Spanned,
    visit_mut::VisitMut,
};

pub fn overlink(args: MockFfiArgs, mut input: syn::ItemFn) -> syn::Result<impl ToTokens> {
    // Validate input
    if input.sig.abi.is_none() {
        return Err(syn::Error::new_spanned(
            input.sig.fn_token,
            "#[overlink] functions must have an explicit ABI",
        ));
    }

    // Named paths
    let std_ = quote! { ::overlink::__reexport::std };
    let internals_ = quote! { ::overlink::__internals };

    // Extract symbol name
    let (raw_symbol_name, raw_symbol_span) = match &args.name {
        Some(name) => (name.value(), name.span()),
        None => (input.sig.ident.to_string(), input.sig.ident.span()),
    };

    let symbol_name = syn::LitStr::new(&raw_symbol_name, raw_symbol_span);
    let symbol_name_c = syn::LitCStr::new(
        &CString::new(raw_symbol_name)
            .map_err(|_| syn::Error::new(raw_symbol_span, "symbol name contains a null byte"))?,
        raw_symbol_span,
    );

    // Generate code for calling the overridden function
    let mut prelude_attrs = Vec::new();
    let mut prelude = Vec::new();

    prelude_attrs.push(parse_quote!( #[export_name = #symbol_name]));

    let super_static = syn::Ident::new("__OVERLINK_SUPER__", Span::call_site());
    let super_fn = syn::Ident::new("__overlink_super__", Span::call_site());
    let super_fn_ty = bare_fn_type(&input.sig)?;

    prelude.extend([
        parse_quote! {
            static #super_static: #std_::sync::LazyLock<#super_fn_ty> = #std_::sync::LazyLock::new(|| unsafe {
                // perform the same check as `next_symbol` already does, but at compile-time
                const _: () = #internals_::next_symbol_check_types::<#super_fn_ty>();

                #internals_::next_symbol::<#super_fn_ty>(#symbol_name_c).unwrap()
            });
        },
        parse_quote! {
            let #super_fn: #super_fn_ty = *#super_static;
        },
    ]);

    // Generate code that checks for recursive calls
    if !args.allow_reentry {
        let recursion_state = syn::Ident::new("__OVERLINK_RECURSION__", Span::call_site());
        let recursion_guard = syn::Ident::new("__overlink_recursion_guard__", Span::call_site());

        let super_forward_args = extract_arg_names(&input.sig.inputs)?;

        prelude.extend([
            parse_quote! {
                #std_::thread_local! {
                    static #recursion_state: #std_::cell::Cell<#std_::primitive::bool> = const { #std_::cell::Cell::new(false) };
                }
            },
            parse_quote! {
                let Some(#recursion_guard) = #internals_::guard_recursion(&#recursion_state) else {
                    // recursed, so fall back to `super()`
                    return #super_fn(#super_forward_args);
                };
            },
        ]);
    }

    // Replace plain "super" with the super function
    struct ReplaceSuper(syn::Path);

    impl VisitMut for ReplaceSuper {
        fn visit_expr_mut(&mut self, i: &mut syn::Expr) {
            match i {
                syn::Expr::Macro(syn::ExprMacro {
                    mac: syn::Macro { path, tokens, .. },
                    ..
                }) if path.is_ident("super") => {
                    let super_fn = &self.0;
                    *i = parse_quote! {
                        #super_fn(#tokens)
                    };
                }

                _ => syn::visit_mut::visit_expr_mut(self, i),
            }
        }

        fn visit_expr_path_mut(&mut self, i: &mut syn::ExprPath) {
            if i.path.is_ident("super") {
                i.path = self.0.clone();
            } else {
                syn::visit_mut::visit_expr_path_mut(self, i);
            }
        }
    }

    ReplaceSuper(super_fn.into()).visit_item_fn_mut(&mut input);

    // Generate the final output and return
    input.attrs = prelude_attrs.into_iter().chain(input.attrs).collect();
    input.block.stmts = prelude.into_iter().chain(input.block.stmts).collect();
    Ok(input)
}

pub struct MockFfiArgs {
    allow_reentry: bool,
    name: Option<syn::LitStr>,
}

impl Parse for MockFfiArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        mod kw {
            syn::custom_keyword!(allow_reentry);
            syn::custom_keyword!(name);
        }

        let mut allow_reentry = false;
        let mut name = None;

        while !input.is_empty() {
            let la = input.lookahead1();

            if la.peek(kw::allow_reentry) {
                let key = input.parse::<kw::allow_reentry>()?;

                if allow_reentry {
                    return Err(syn::Error::new_spanned(
                        key,
                        "`allow_reentry` argument may not appear more than once",
                    ));
                }

                allow_reentry = true;
            } else if la.peek(kw::name) {
                let key = input.parse::<kw::name>()?;

                if name.is_some() {
                    return Err(syn::Error::new_spanned(
                        key,
                        "`name` argument may not appear more than once",
                    ));
                }

                input.parse::<syn::Token![=]>()?;
                let value = input.parse()?;

                name = Some(value);
            } else {
                return Err(la.error());
            }

            if input.is_empty() {
                break;
            }

            input.parse::<syn::Token![,]>()?;
        }

        Ok(Self {
            allow_reentry,
            name,
        })
    }
}

/// Helper to convert a function signature into a function pointer type.
fn bare_fn_type(sig: &syn::Signature) -> syn::Result<syn::TypeBareFn> {
    let span = sig.span();

    let lifetimes = sig
        .generics
        .params
        .iter()
        .map(|param| match param {
            syn::GenericParam::Lifetime(_) => Ok(param.clone()),
            _ => Err(syn::Error::new_spanned(
                param,
                "#[overlink] does not support non-lifetime generic parameters",
            )),
        })
        .collect::<syn::Result<_>>()?;

    let inputs = sig
        .inputs
        .iter()
        .map(|input| match input {
            syn::FnArg::Receiver(_) => Err(syn::Error::new_spanned(
                input,
                "#[overlink] does not support self-receiver arguments",
            )),
            syn::FnArg::Typed(syn::PatType { attrs, ty, .. }) => Ok(syn::BareFnArg {
                attrs: attrs.clone(),
                name: None,
                ty: (**ty).clone(),
            }),
        })
        .collect::<syn::Result<_>>()?;

    let variadic = sig.variadic.as_ref().map(|variadic| syn::BareVariadic {
        attrs: variadic.attrs.clone(),
        name: None,
        dots: variadic.dots,
        comma: variadic.comma,
    });

    Ok(syn::TypeBareFn {
        lifetimes: Some(syn::BoundLifetimes {
            for_token: syn::Token![for](span),
            lt_token: syn::Token![<](span),
            lifetimes,
            gt_token: syn::Token![>](span),
        }),
        unsafety: sig.unsafety,
        abi: sig.abi.clone(),
        fn_token: sig.fn_token,
        paren_token: sig.paren_token,
        inputs,
        variadic,
        output: sig.output.clone(),
    })
}

/// Helper to turn a list of function signature arguments into identifiers.
fn extract_arg_names<'a>(
    inputs: impl IntoIterator<Item = &'a syn::FnArg>,
) -> syn::Result<Punctuated<syn::Expr, syn::Token![,]>> {
    inputs
        .into_iter()
        .map(|input| match &input {
            syn::FnArg::Receiver(syn::Receiver { self_token, .. }) => {
                Ok::<syn::Expr, _>(syn::Expr::Path(syn::ExprPath {
                    attrs: Vec::new(),
                    qself: None,
                    path: (*self_token).into(),
                }))
            }
            syn::FnArg::Typed(syn::PatType { pat, .. }) => match &**pat {
                syn::Pat::Ident(syn::PatIdent { ident, .. }) => {
                    Ok(syn::Expr::Path(syn::ExprPath {
                        attrs: Vec::new(),
                        qself: None,
                        path: ident.clone().into(),
                    }))
                }
                _ => Err(syn::Error::new_spanned(
                    pat,
                    "only simply named arguments are supported by #[overlink]",
                )),
            },
        })
        .collect()
}
