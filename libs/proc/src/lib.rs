use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, punctuated::Punctuated, token::Comma, ItemFn, Meta, parse::Parser};

/// Parse configuration attributes and build a MonorailConfiguration struct literal
fn build_config(attr: TokenStream) -> Result<proc_macro2::TokenStream, TokenStream> {
    let attrs = if attr.is_empty() {
        Punctuated::<Meta, Comma>::new()
    } else {
        match Punctuated::<Meta, Comma>::parse_terminated.parse(attr) {
            Ok(attrs) => attrs,
            Err(e) => return Err(e.to_compile_error().into()),
        }
    };
    
    let mut limit_value = quote! { None };
    
    for meta in attrs {
        match meta {
            Meta::NameValue(nv) => {
                if let Some(ident) = nv.path.get_ident() {
                    let key = ident.to_string();
                    match key.as_str() {
                        "cores" | "core_override" | "limit" => {
                            let value = &nv.value;
                            limit_value = quote! { Some(#value) };
                        }
                        _ => {
                            return Err(syn::Error::new_spanned(
                                &nv.path,
                                format!("unknown configuration key: {}", key)
                            )
                            .to_compile_error()
                            .into());
                        }
                    }
                }
            }
            _ => {
                return Err(syn::Error::new_spanned(&meta, "expected key=value pairs")
                    .to_compile_error()
                    .into());
            }
        }
    }
    
    Ok(quote! {
        MonorailConfiguration {
            limit: #limit_value
        }
    })
}

/// Entrypoint to monorail.
/// Expands roughly to :
/// 
/// MonorailTopology::normal(|| async {
///     // user code here
/// })
/// 
/// Requires an async function.
/// attrs can be:
/// #[main( key=value )]
/// we will make a topology as follows:
/// 
/// let config = MonorailConfiguration { key: Some ( value ), ..Default::default() }; 
/// let topology = MonorailTopology::setup(config, || async { body });
/// 
/// Function arguments are passed through to the closure.
#[proc_macro_attribute]
pub fn main(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    
    if input.sig.asyncness.is_none() {
        return syn::Error::new_spanned(&input.sig, "function must be async")
            .to_compile_error()
            .into();
    }
    
    // Build configuration based on attributes
    let config = match build_config(attr) {
        Ok(config) => config,
        Err(e) => return e,
    };
    
    let body = &input.block;
    let fn_name = &input.sig.ident;
    let fn_vis = &input.vis;
    let fn_inputs = &input.sig.inputs;
    let fn_output = &input.sig.output;
    
    let expanded = quote! {
        #fn_vis fn #fn_name(#fn_inputs) #fn_output {
            MonorailTopology::setup(
                #config,
                || async move {
                    #body
                    signal_monorail(Ok(()));
                }
            ).unwrap();
        }
    };
    
    expanded.into()
}


/// Test entrypoint for monorail.
/// Similar to #[main], but wraps the function for use with #[test].
/// Creates a test function that sets up a MonorailTopology and runs the async code.
/// 
/// attrs can be:
/// #[test(cores=4)]
/// 
/// Expands to:
/// #[test]
/// fn test_name() {
///     MonorailTopology::setup(config, || async {
///         // original async function body
///     }).unwrap();
/// }
#[proc_macro_attribute]
pub fn test(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    
    if input.sig.asyncness.is_none() {
        return syn::Error::new_spanned(&input.sig, "function must be async")
            .to_compile_error()
            .into();
    }
    
    // Build configuration based on attributes
    let config = match build_config(attr) {
        Ok(config) => config,
        Err(e) => return e,
    };
    
    let body = &input.block;
    let fn_name = &input.sig.ident;
    let fn_vis = &input.vis;
    let fn_inputs = &input.sig.inputs;
    let attrs = &input.attrs;
    
    // Remove async from signature since the outer function won't be async
    let expanded = quote! {
        #[test]
        #(#attrs)*
        #fn_vis fn #fn_name(#fn_inputs) {
            MonorailTopology::setup(
                #config,
                || async move {
                    #body
                    signal_monorail(Ok(()));
                }
            ).unwrap();
        }
    };
    
    expanded.into()
}