use quote::ToTokens;

mod impl_;

#[proc_macro_attribute]
pub fn overlink(
    args: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let args = syn::parse_macro_input!(args);
    let input = syn::parse_macro_input!(input);

    impl_::overlink(args, input)
        .map(|output| output.into_token_stream())
        .unwrap_or_else(|err| err.into_compile_error())
        .into()
}
