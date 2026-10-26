//! `#[ilium_plugin(side = "...", exports = [...])]` (rust-sdk.md "The ilium_plugin macro").
//!
//! Server and client native entry points forward to SDK lifecycle and handler traits.
//! Scene native callbacks and wasm32 glue remain separate, unfinished contracts.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::ext::IdentExt;
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
            // `crate = "..."` is a supported argument even though crate is a Rust keyword.
            let name = input.call(Ident::parse_any)?;
            input.parse::<Token![=]>()?;
            match name.to_string().as_str() {
                "side" => {
                    let value: LitStr = input.parse()?;
                    if !SIDES.contains(&value.value().as_str()) {
                        return Err(syn::Error::new(
                            value.span(),
                            "side must be \"server\", \"client\" or \"scene\"",
                        ));
                    }
                    side = Some(value.value());
                }
                "exports" => {
                    let array: ExprArray = input.parse()?;
                    for element in array.elems {
                        match element {
                            Expr::Lit(ExprLit {
                                lit: Lit::Str(text),
                                ..
                            }) => exports.push(text.value()),
                            other => {
                                return Err(syn::Error::new_spanned(
                                    other,
                                    "exports must be string literals",
                                ))
                            }
                        }
                    }
                }
                "crate" => {
                    let value: LitStr = input.parse()?;
                    krate = value.parse()?;
                }
                other => {
                    return Err(syn::Error::new(
                        name.span(),
                        format!("unknown ilium_plugin argument `{other}`"),
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        let side = side.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "ilium_plugin requires side = \"...\"")
        })?;
        Ok(PluginArgs {
            side,
            exports,
            krate,
        })
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
    // Scene worlds have a different lifecycle; never impose server/client traits on them.
    let (activate, deactivate, events) = if side == "scene" {
        (
            quote!(unimplemented!("rust-sdk.scene-native-lifecycle")),
            quote!(unimplemented!("rust-sdk.scene-native-lifecycle")),
            quote!(unimplemented!("rust-sdk.scene-native-handler")),
        )
    } else {
        (
            quote!(#krate::Lifecycle::activate(self, _context)),
            quote!(#krate::Lifecycle::deactivate(self, _reason)),
            quote!(#krate::Handler::on_events(self, _batch)),
        )
    };
    quote! {
        #item_struct

        impl #krate::NativeEntry for #name {
            const SIDE: &'static str = #side;
            const EXPORTS: &'static [&'static str] = &[#(#exports),*];

            fn native_activate(
                &mut self,
                _context: #krate::types::Activation,
            ) -> ::core::result::Result<(), #krate::types::PluginError> {
                #activate
            }

            fn native_deactivate(
                &mut self,
                _reason: #krate::types::DeactivationReason,
            ) -> ::core::option::Option<::std::vec::Vec<u8>> {
                #deactivate
            }

            fn native_on_events(
                &mut self,
                _batch: ::std::vec::Vec<#krate::types::Event>,
            ) -> ::std::vec::Vec<#krate::types::Submission> {
                #events
            }
        }
    }
    .into()
}
