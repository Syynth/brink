//! Procedural derive for [`BrinkCommand`].
//!
//! `#[derive(BrinkCommand)]` generates a `from_ink_args` implementation for
//! a struct whose fields are all `i32`, `f32`, `bool`, or `String`. Each
//! field is parsed, in declaration order, from the corresponding ink
//! argument with **strict** type matching — no coercion (an ink `int`
//! does not satisfy an `f32` field, etc.). For anything fancier (custom
//! types, coercion, or a non-default [`reply`]), implement the trait by
//! hand.
//!
//! Supported field → ink value mappings:
//!
//! | Rust field | ink `Value` |
//! |------------|-------------|
//! | `i32`      | `Int`       |
//! | `f32`      | `Float`     |
//! | `bool`     | `Bool`      |
//! | `String`   | `String`    |
//!
//! Named structs, tuple structs, and unit structs (zero arguments) are all
//! supported. Generated code refers to everything through `::bevy_brink`,
//! so the deriving crate needs only a dependency on `bevy-brink`.
//!
//! ```ignore
//! #[derive(Event, Clone, BrinkCommand)]
//! struct PlaySound { name: String, volume: f32 }
//! ```
//!
//! [`BrinkCommand`]: trait@bevy_brink::BrinkCommand
//! [`reply`]: bevy_brink::BrinkCommand::reply

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident, Type, parse_macro_input};

/// Derive [`BrinkCommand`](trait@bevy_brink::BrinkCommand) for a struct of
/// supported scalar fields. See the crate docs for the field-type mapping
/// and limitations.
#[proc_macro_derive(BrinkCommand)]
pub fn derive_brink_command(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(data) => &data.fields,
        _ => {
            return syn::Error::new_spanned(
                &input,
                "BrinkCommand can only be derived for structs",
            )
            .to_compile_error()
            .into();
        }
    };

    let field_count = fields.len();

    // One extractor expression per field, in declaration order.
    let extractors: Result<Vec<_>, syn::Error> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let (variant, extract, expected) = value_mapping(&field.ty)?;
            Ok(quote! {
                match &args[#index] {
                    ::bevy_brink::Value::#variant(v) => #extract,
                    _ => {
                        return ::core::result::Result::Err(
                            ::bevy_brink::BrinkArgError::Type {
                                index: #index,
                                expected: #expected,
                            },
                        );
                    }
                }
            })
        })
        .collect();
    let extractors = match extractors {
        Ok(e) => e,
        Err(e) => return e.to_compile_error().into(),
    };

    let constructor = match fields {
        Fields::Named(named) => {
            let field_names: Vec<Ident> = named
                .named
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    f.ident
                        .clone()
                        .unwrap_or_else(|| Ident::new(&format!("_f{i}"), Span::call_site()))
                })
                .collect();
            quote! { #name { #(#field_names: #extractors),* } }
        }
        Fields::Unnamed(_) => quote! { #name ( #(#extractors),* ) },
        Fields::Unit => quote! { #name },
    };

    let expanded = quote! {
        impl #impl_generics ::bevy_brink::BrinkCommand for #name #ty_generics #where_clause {
            fn from_ink_args(
                args: &[::bevy_brink::Value],
            ) -> ::core::result::Result<Self, ::bevy_brink::BrinkArgError> {
                if args.len() != #field_count {
                    return ::core::result::Result::Err(
                        ::bevy_brink::BrinkArgError::Count {
                            expected: #field_count,
                            got: args.len(),
                        },
                    );
                }
                ::core::result::Result::Ok(#constructor)
            }
        }
    };
    expanded.into()
}

/// Map a supported field type to its `(Value variant, extraction expr,
/// human-readable type name)`. Errors on unsupported types.
fn value_mapping(
    ty: &Type,
) -> Result<(Ident, proc_macro2::TokenStream, &'static str), syn::Error> {
    let type_name = quote!(#ty).to_string();
    let span = Span::call_site();
    match type_name.as_str() {
        "i32" => Ok((Ident::new("Int", span), quote!(*v), "int")),
        "f32" => Ok((Ident::new("Float", span), quote!(*v), "float")),
        "bool" => Ok((Ident::new("Bool", span), quote!(*v), "bool")),
        "String" => Ok((Ident::new("String", span), quote!(v.to_string()), "string")),
        other => Err(syn::Error::new_spanned(
            ty,
            format!(
                "BrinkCommand: unsupported field type `{other}`. Supported: \
                 i32, f32, bool, String. For other types, implement \
                 BrinkCommand by hand."
            ),
        )),
    }
}
