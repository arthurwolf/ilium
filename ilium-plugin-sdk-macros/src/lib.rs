//! `#[ilium_plugin(side = "...", exports = [...])]` (rust-sdk.md "The ilium_plugin macro").
//!
//! Skeleton: arguments are parsed and unknown ones are a compile error. The expansion is an
//! `impl NativeEntry` whose methods are `unimplemented!`. The wasm32 glue (wit-bindgen exports)
//! comes with the implementation phase.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Expr, ExprArray, ExprLit, Ident, ItemStruct, Lit, LitStr, Token};

const SIDES: &[&str] = &["server", "client", "scene"];

struct PluginArgs {
    side: String,
    exports: Vec<String>,
    krate: syn::Path,
}

impl Parse for PluginArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut side = None;
        let mut exports = Vec::new();
        let mut krate: syn::Path = syn::parse_quote!(::ilium_plugin_sdk);
        while !input.is_empty() {
            let name: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            match name.to_string().as_str() {
                "side" => {
                    let value: LitStr = input.parse()?;
                    if !SIDES.contains(&value.value().as_str()) {
                        return Err(syn::Error::new(value.span(), "side must be \"server\", \"client\" or \"scene\""));
                    }
                    side = Some(value.value());
                }
                "exports" => {
                    let array: ExprArray = input.parse()?;
                    for element in array.elems {
                        match element {
                            Expr::Lit(ExprLit { lit: Lit::Str(text), .. }) => exports.push(text.value()),
                            other => return Err(syn::Error::new_spanned(other, "exports must be string literals")),
                        }
                    }
                }
                "crate" => {
                    let value: LitStr = input.parse()?;
                    krate = value.parse()?;
                }
                other => {
                    return Err(syn::Error::new(name.span(), format!("unknown ilium_plugin argument `{other}`")));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        let side = side.ok_or_else(|| syn::Error::new(Span::call_site(), "ilium_plugin requires side = \"...\""))?;
        Ok(PluginArgs { side, exports, krate })
    }
}

/// Marks the plugin struct and generates its entry points (rust-sdk.md "The ilium_plugin macro").
#[proc_macro_attribute]
pub fn ilium_plugin(args: TokenStream, item: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(args as PluginArgs);
    let item_struct = syn::parse_macro_input!(item as ItemStruct);
    let name = &item_struct.ident;
    let side = &args.side;
    let exports = &args.exports;
    let krate = &args.krate;
    quote! {
        #item_struct

        impl #krate::NativeEntry for #name {
            const SIDE: &'static str = #side;
            const EXPORTS: &'static [&'static str] = &[#(#exports),*];

            fn native_activate(
                &mut self,
                _context: #krate::types::Activation,
            ) -> ::core::result::Result<(), #krate::types::PluginError> {
                unimplemented!("rust-sdk.macro-dispatch")
            }

            fn native_deactivate(
                &mut self,
                _reason: #krate::types::DeactivationReason,
            ) -> ::core::option::Option<::std::vec::Vec<u8>> {
                unimplemented!("rust-sdk.macro-dispatch")
            }

            fn native_on_events(
                &mut self,
                _batch: ::std::vec::Vec<#krate::types::Event>,
            ) -> ::std::vec::Vec<#krate::types::Submission> {
                unimplemented!("rust-sdk.macro-dispatch")
            }
        }
    }
    .into()
}
