//! `#[ilium_plugin(side = "...", exports = [...])]` (rust-sdk.md "The ilium_plugin macro").
//!
//! Server and client native entry points forward to SDK lifecycle and handler traits; unknown
//! arguments are a compile error. Scene native callbacks and wasm32 glue remain separate,
//! unfinished contracts.

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
    let is_scene = side == "scene";
    let activate = if is_scene {
        quote!(unimplemented!("rust-sdk.scene-native-lifecycle"))
    } else {
        quote!(<Self as #krate::Lifecycle>::activate(self, _context))
    };
    let deactivate = if is_scene {
        quote!(unimplemented!("rust-sdk.scene-native-lifecycle"))
    } else {
        quote!(<Self as #krate::Lifecycle>::deactivate(self, _reason))
    };
    // Only a plugin that declares the `handler` export implements `Handler`; others receive no events.
    let events = if is_scene {
        quote!(unimplemented!("rust-sdk.scene-native-handler"))
    } else if exports.iter().any(|export| export == "handler") {
        quote!(<Self as #krate::Handler>::on_events(self, _batch))
    } else {
        quote! {
            let _ = _batch;
            ::std::vec::Vec::new()
        }
    };
    let has = |name: &str| exports.iter().any(|export| export == name);
    let run_job_fn = if has("job-runner") {
        quote! {
            fn native_run_job(
                &mut self,
                kind: ::std::string::String,
                input: ::std::vec::Vec<u8>,
            ) -> ::core::option::Option<::core::result::Result<::std::vec::Vec<u8>, #krate::types::PluginError>> {
                ::core::option::Option::Some(<Self as #krate::JobRunner>::run(self, kind, input))
            }
        }
    } else {
        quote! {}
    };
    let handle_service_fn = if has("service-provider") {
        quote! {
            fn native_handle_service(
                &mut self,
                call: #krate::exports::ServiceCall,
            ) -> ::core::option::Option<#krate::exports::ServiceReply> {
                ::core::option::Option::Some(<Self as #krate::ServiceProvider>::handle(self, call))
            }
        }
    } else {
        quote! {}
    };
    let handle_hook_fn = if has("hook-handler") {
        quote! {
            fn native_handle_hook(
                &mut self,
                call: #krate::exports::HookCall,
            ) -> ::core::option::Option<::core::result::Result<#krate::exports::HookReply, #krate::types::PluginError>> {
                ::core::option::Option::Some(<Self as #krate::HookHandler>::handle_hook(self, call))
            }
        }
    } else {
        quote! {}
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

            #run_job_fn
            #handle_service_fn
            #handle_hook_fn
        }
    }
    .into()
}

#[cfg(test)]
mod spec_tests {
    use super::PluginArgs;

    fn parse_args(source: &str) -> syn::Result<PluginArgs> {
        syn::parse_str::<PluginArgs>(source)
    }

    fn parse_error_message(source: &str) -> String {
        match parse_args(source) {
            Ok(_) => panic!("expected `{source}` to be rejected"),
            Err(error) => error.to_string(),
        }
    }

    /// Manual: side and exports are read from the attribute
    #[test]
    fn side_and_exports_are_read_in_declared_order() {
        let args = parse_args(
            r#"side = "client", exports = ["views", "catalog-provider", "token-provider"]"#,
        )
        .expect("a client plugin with three exports must parse");

        assert_eq!(args.side, "client");
        assert_eq!(
            args.exports,
            vec!["views", "catalog-provider", "token-provider"]
        );
    }

    #[test]
    fn omitted_or_empty_exports_mean_no_optional_exports() {
        let omitted = parse_args(r#"side = "server""#).expect("side alone must parse");
        let empty = parse_args(r#"side = "server", exports = []"#)
            .expect("an empty exports list must parse");

        assert!(omitted.exports.is_empty());
        assert!(empty.exports.is_empty());
    }

    #[test]
    fn every_documented_side_is_accepted() {
        for side in ["server", "client", "scene"] {
            let args = parse_args(&format!(r#"side = "{side}""#))
                .expect("each documented side must parse");
            assert_eq!(args.side, side);
        }
    }

    /// Manual: the SDK crate is ilium_plugin_sdk
    #[test]
    fn generated_code_refers_to_the_sdk_crate_by_its_absolute_path() {
        let args = parse_args(r#"side = "server""#).expect("side alone must parse");

        assert!(args.krate.leading_colon.is_some());
        let segment_names: Vec<String> = args
            .krate
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        assert_eq!(segment_names, vec!["ilium_plugin_sdk"]);
    }

    /// Manual: side is required
    #[test]
    fn missing_side_is_a_compile_error_including_for_empty_input() {
        let without_side = parse_error_message(r#"exports = ["views"]"#);
        let empty_input = parse_error_message("");

        assert!(
            without_side.contains("requires side"),
            "got: {without_side}"
        );
        assert!(empty_input.contains("requires side"), "got: {empty_input}");
    }

    #[test]
    fn side_outside_the_three_documented_values_is_rejected() {
        let message = parse_error_message(r#"side = "both""#);

        assert!(
            message.contains(r#"side must be "server", "client" or "scene""#),
            "got: {message}"
        );
    }

    /// Manual: unknown attribute arguments are a compile error
    #[test]
    fn unknown_argument_is_a_compile_error() {
        let message = parse_error_message(r#"side = "server", colour = "red""#);

        assert!(
            message.contains("unknown ilium_plugin argument `colour`"),
            "got: {message}"
        );
    }

    #[test]
    fn non_string_export_entry_is_a_compile_error() {
        let message = parse_error_message(r#"side = "server", exports = ["views", 3]"#);

        assert!(
            message.contains("exports must be string literals"),
            "got: {message}"
        );
    }
}
