use proc_macro::{self, TokenStream};
use quote::{format_ident, quote};
use syn::{Attribute, Data, DataStruct, DeriveInput, Expr, Index, Meta, MetaNameValue};
use thiserror::Error;

fn err(msg: &'static str) -> Result<TokenStream, Error> {
    Err(Error::CompilerError(quote! { compile_error(#msg) }.into()))
}

fn find_skip_value<'a>(attrs: impl IntoIterator<Item = &'a Attribute>) -> Option<&'a Expr> {
    attrs
        .into_iter()
        .filter_map(|a| {
            if let Meta::NameValue(MetaNameValue { ref value, .. }) = a.meta {
                Some(value)
            } else {
                None
            }
        })
        .next()
}

#[derive(Debug, Error)]
enum Error {
    #[error("syn error: {0}")]
    SynError(#[from] syn::Error),
    #[error("compiler error: {0}")]
    CompilerError(TokenStream),
}

macro_rules! parse {
    ($($arg:tt)*) => {
        syn::parse(quote! { $($arg)* }.into())
    };
}

#[proc_macro_derive(Columns, attributes(skip))]
pub fn derive_columns_from_fields(input: TokenStream) -> TokenStream {
    fn helper(input: TokenStream) -> Result<TokenStream, Error> {
        let DeriveInput {
            vis, ident, generics, data: Data::Struct(DataStruct { fields, .. }), ..
        } = syn::parse(input)?
        else {
            return err("");
        };

        // TODO: should items in std::prelude also use the full paths?
        // FIXME: how to decide whether to use crate or niche_collections here?
        let index = quote! { crate::alloc::Index };
        let length = quote! { crate::alloc::Length };
        let intoindex = quote! { crate::alloc::store::IntoIndex };
        let columns = quote! { crate::alloc::store::Columns };
        let validate = quote! { crate::alloc::store::validate_row_index };
        let entry = quote! { crate::alloc::store::FreelistEntry };
        let result = quote! { crate::alloc::store::SResult };
        let rawptr = quote! { std::ptr::NonNull<u8> };
        let phantom = quote! { std::marker::PhantomData };
        let layout = quote! { std::alloc::Layout };
        let layouterr = quote! { std::alloc::LayoutError };

        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        let mut view_generics = generics.clone();
        view_generics.params.push(parse!('a)?);
        view_generics.params.push(parse!(I: #intoindex)?);
        let (impl_generics_view, _ty_generics_view, where_clause_view) =
            view_generics.split_for_impl();
        let (default_fields, column_fields): (Vec<_>, Vec<_>) =
            fields.members().zip(&fields).partition(|(_, f)| find_skip_value(&f.attrs).is_some());
        if column_fields.is_empty() {
            return err("invalid input: can't skip all fields");
        }

        let column_indices = (0..column_fields.len()).map(Index::from).collect::<Vec<_>>();
        let column_names = column_fields.iter().map(|(m, _)| m).collect::<Vec<_>>();
        let column_names_mut =
            column_fields.iter().map(|(m, _)| format_ident!("{}_mut", m)).collect::<Vec<_>>();
        let column_names_into =
            column_fields.iter().map(|(m, _)| format_ident!("into_{}", m)).collect::<Vec<_>>();
        let column_names_into_mut =
            column_fields.iter().map(|(m, _)| format_ident!("into_{}_mut", m)).collect::<Vec<_>>();
        let column_types = column_fields.iter().map(|(_, c)| &c.ty).collect::<Vec<_>>();

        let defaults_names = default_fields.iter().map(|(m, _)| m);
        let defaults_values = default_fields
            .iter()
            .map(|(_, f)| find_skip_value(&f.attrs).expect("defaults were preselected"));

        let tupletype = quote! { (#(#column_types,)*) };
        let typeinit = if column_fields.first().map(|(_, f)| f.ident.is_some()).unwrap_or_default()
        {
            quote! { Self {
                #(#column_names: columns.#column_indices,)*
                #(#defaults_names: #defaults_values,)*
            }}
        } else {
            // TODO: how to get the correct order of arguments?
            return err("tuple structs currently not supported");
        };

        let refname = format_ident!("{}Ref", ident);
        let mutname = format_ident!("{}Mut", ident);

        Ok(quote! {
            #vis struct #refname<'a, I: #intoindex, T> (&'a [#rawptr], #rawptr, #phantom<fn(I) -> &'a T>);
            impl #impl_generics_view #refname<'a, I, #ident #ty_generics> #where_clause_view {
                #(
                    pub fn #column_names(&self, index: I) -> #result<&'a #column_types> {
                        let index = index.into_index();
                        // SAFETY: self.1 is a valid pointer ot an occupation table
                        unsafe { #validate(self.1, index)? };
                        Ok(unsafe {
                            self.0[0].cast::<#entry<#column_types>>().add(index.get() as usize)
                                .cast::<#column_types>().as_ref()
                        })
                    }
                )*
            }
            #vis struct #mutname<'a, I: #intoindex, T> (&'a [#rawptr], #rawptr, #phantom<fn(I) -> &'a mut T>);
            impl #impl_generics_view #mutname<'a, I, #ident #ty_generics> #where_clause_view {
                #(
                    pub fn #column_names(&self, index: I) -> #result<&#column_types> {
                        let index = index.into_index();
                        // SAFETY: self.1 is a valid pointer ot an occupation table
                        unsafe { #validate(self.1, index)? };
                        Ok(unsafe {
                            self.0[0].cast::<#entry<#column_types>>().add(index.get() as usize)
                                .cast::<#column_types>().as_ref()
                        })
                    }
                    pub fn #column_names_mut(&mut self, index: I) -> #result<&mut #column_types> {
                        let index = index.into_index();
                        // SAFETY: self.1 is a valid pointer ot an occupation table
                        unsafe { #validate(self.1, index)? };
                        Ok(unsafe {
                            self.0[0].cast::<#entry<#column_types>>().add(index.get() as usize)
                                .cast::<#column_types>().as_mut()
                        })
                    }
                    pub fn #column_names_into(self, index: I) -> #result<&'a #column_types> {
                        let index = index.into_index();
                        // SAFETY: self.1 is a valid pointer ot an occupation table
                        unsafe { #validate(self.1, index)? };
                        Ok(unsafe {
                            self.0[0].cast::<#entry<#column_types>>().add(index.get() as usize)
                                .cast::<#column_types>().as_ref()
                        })
                    }
                    pub fn #column_names_into_mut(self, index: I) -> #result<&'a mut #column_types> {
                        let index = index.into_index();
                        // SAFETY: self.1 is a valid pointer ot an occupation table
                        unsafe { #validate(self.1, index)? };
                        Ok(unsafe {
                            self.0[0].cast::<#entry<#column_types>>().add(index.get() as usize)
                                .cast::<#column_types>().as_mut()
                        })
                    }
                )*
            }

            unsafe impl #impl_generics #columns for #ident #ty_generics #where_clause {
                const COUNT: usize = <#tupletype as #columns>::COUNT;

                type Ref<'a, I> = #refname<'a, I, Self>
                where
                    I: #intoindex + 'a,
                    Self: 'a;
                type Mut<'a, I> = #mutname<'a, I, Self>
                where
                    I: #intoindex + 'a,
                    Self: 'a;

                fn register_layout(rows: #length, register: &mut impl FnMut(#layout)) -> Result<(), #layouterr> {
                    <#tupletype as #columns>::register_layout(rows, register)
                }
                fn move_into(self, index: #index, columns: &[#rawptr]) {
                    <#tupletype as #columns>::move_into((#(self.#column_names,)*), index, columns)
                }
                fn take(index: #index, columns: &[#rawptr]) -> Self {
                    let columns = <#tupletype as #columns>::take(index, columns);
                    #typeinit
                }
                #[expect(clippy::mut_from_ref, reason = "trait user is responsible for this")]
                fn as_freelist_entry(index: #index, columns: &[#rawptr]) -> &mut Option<#index> {
                    <#tupletype as #columns>::as_freelist_entry(index, columns)
                }
                fn make_ref<'a, I>(columns: &'a [#rawptr], occupation_ptr: #rawptr) -> Self::Ref<'a, I>
                where
                    I: #intoindex + 'a,
                    Self: 'a,
                {
                    #refname(columns, occupation_ptr, #phantom)
                }
                fn make_mut<'a, I>(columns: &'a [#rawptr], occupation_ptr: #rawptr) -> Self::Mut<'a, I>
                where
                    I: #intoindex + 'a,
                    Self: 'a,
                {
                    #mutname(columns, occupation_ptr, #phantom)
                }
            }
        }.into())
    }
    match helper(input) {
        Ok(result) => result,
        Err(Error::SynError(err)) => err.into_compile_error().into(),
        Err(Error::CompilerError(err)) => err,
    }
}
