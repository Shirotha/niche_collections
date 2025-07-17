use proc_macro::{self, TokenStream};
use quote::{format_ident, quote};
use syn::{Attribute, Data, DataStruct, DeriveInput, Expr, Index, Meta, MetaNameValue};
use thiserror::Error;

macro_rules! parse {
    ($($arg:tt)*) => {
        syn::parse(quote! { $($arg)* }.into())
    };
}

fn err(msg: &'static str) -> Result<TokenStream, Error> {
    Err(Error::CompilerError(quote! { compile_error(#msg) }.into()))
}

fn has_attr_with_name<'a>(attrs: impl IntoIterator<Item = &'a Attribute>, name: &'a str) -> bool {
    attrs.into_iter().any(|a| a.path().is_ident(name))
}

fn attr_get_value<'a>(
    attrs: impl IntoIterator<Item = &'a Attribute>,
    name: &'a str,
) -> Option<&'a Expr> {
    attrs
        .into_iter()
        .filter_map(|a| {
            if let Meta::NameValue(MetaNameValue { ref path, ref value, .. }) = a.meta
                && path.is_ident(name)
            {
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

#[proc_macro_derive(Columns, attributes(internal, skip, freelist_entry, as_bits))]
pub fn derive_columns_from_fields(input: TokenStream) -> TokenStream {
    fn helper(input: TokenStream) -> Result<TokenStream, Error> {
        let DeriveInput {
            attrs,
            vis,
            ident,
            generics,
            data: Data::Struct(DataStruct { fields, .. }),
            ..
        } = syn::parse(input)?
        else {
            return err("");
        };

        let this_crate = if has_attr_with_name(&attrs, "internal") {
            quote! { crate }
        } else {
            quote! { niche_collections }
        };
        let index = quote! { #this_crate::alloc::Index };
        let length = quote! { #this_crate::alloc::Length };
        let intoindex = quote! { #this_crate::alloc::store::IntoIndex };
        let columns = quote! { #this_crate::alloc::store::Columns };
        let validate = quote! { #this_crate::alloc::store::validate_row_index };
        let entry = quote! { #this_crate::alloc::store::FreelistEntry };
        let result = quote! { #this_crate::alloc::store::SResult };
        let bitsref = quote! { #this_crate::alloc::store::atomic::BitsRef };
        let bitsmut = quote! { #this_crate::alloc::store::atomic::BitsMut };
        let items = quote! { #this_crate::alloc::store::atomic::items_per_chunk };
        let rawptr = quote! { std::ptr::NonNull<u8> };
        let phantom = quote! { std::marker::PhantomData };
        let layout = quote! { std::alloc::Layout };
        let layouterr = quote! { std::alloc::LayoutError };
        let atomicint = quote! { std::sync::atomic::AtomicUsize };

        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        let mut view_generics = generics.clone();
        view_generics.params.push(parse!('a)?);
        view_generics.params.push(parse!(I: #intoindex)?);
        let (impl_generics_view, _ty_generics_view, where_clause_view) =
            view_generics.split_for_impl();
        let (default_fields, column_fields): (Vec<_>, Vec<_>) = fields
            .members()
            .zip(&fields)
            .partition(|(_, f)| attr_get_value(&f.attrs, "skip").is_some());
        if !column_fields.first().map(|(_, f)| f.ident.is_some()).unwrap_or_default() {
            return err("only structs with at least one named field are supported");
        };

        let column_count = column_fields.len();
        let column_indices = (0..column_count).map(Index::from).collect::<Vec<_>>();
        let column_names = column_fields.iter().map(|(m, _)| m).collect::<Vec<_>>();
        let column_names_mut =
            column_fields.iter().map(|(m, _)| format_ident!("{}_mut", m)).collect::<Vec<_>>();
        let column_names_into =
            column_fields.iter().map(|(m, _)| format_ident!("into_{}", m)).collect::<Vec<_>>();
        let column_names_into_mut =
            column_fields.iter().map(|(m, _)| format_ident!("into_{}_mut", m)).collect::<Vec<_>>();

        let column_types = column_fields.iter().map(|(_, c)| &c.ty).collect::<Vec<_>>();

        let default_names = default_fields.iter().map(|(m, _)| m);
        let default_values = default_fields
            .iter()
            .map(|(_, f)| attr_get_value(&f.attrs, "skip").expect("defaults were preselected"));

        let column_is_bits = column_fields
            .iter()
            .map(|(_, f)| has_attr_with_name(&f.attrs, "as_bits"))
            .collect::<Vec<_>>();
        let mut freelist_entries = column_fields
            .iter()
            .enumerate()
            .filter_map(|(i, (_, f))| has_attr_with_name(&f.attrs, "freelist_entry").then_some(i));
        let freelist_index = freelist_entries.next().unwrap_or_default();
        if column_is_bits[freelist_index] {
            return err("an as_bits column cannot be the freelist entry");
        }
        if freelist_entries.next().is_some() {
            return err("cannot mark more than one field as a freelist entry");
        }

        let mut column_stored_types = column_types.iter().map(|&c| c.clone()).collect::<Vec<_>>();
        column_stored_types[freelist_index] = {
            let bare = column_types[freelist_index];
            parse! { #entry<#bare> }.expect("this is a valid type")
        };
        for t in
            column_is_bits.iter().zip(&mut column_stored_types).filter_map(|(b, t)| b.then_some(t))
        {
            *t = parse! { #atomicint }.expect("this is a valid type");
        }
        let freelist_stored_type = &column_stored_types[freelist_index];

        let refname = format_ident!("{}Ref", ident);
        let mutname = format_ident!("{}Mut", ident);

        let column_ref_ = column_types
            .iter()
            .zip(&column_is_bits)
            .map(|(t, b)| {
                if *b {
                    quote! { #bitsref<'_, #t> }
                } else {
                    quote! { &#t }
                }
            })
            .collect::<Vec<_>>();
        let column_refa = column_types
            .iter()
            .zip(&column_is_bits)
            .map(|(t, b)| {
                if *b {
                    quote! { #bitsref<'a, #t> }
                } else {
                    quote! { &'a #t }
                }
            })
            .collect::<Vec<_>>();
        let column_as_ref = column_indices
            .iter()
            .zip(&column_types)
            .zip(&column_stored_types)
            .zip(&column_is_bits)
            .map(|(((i, t), st), b)| {
                if *b {
                    quote! { unsafe {
                        #bitsref::from_column_index(self.0[#i], index)
                    } }
                } else {
                    quote! { unsafe {
                         self.0[#i].cast::<#st>().add(index.get() as usize).cast::<#t>().as_ref()
                    } }
                }
            })
            .collect::<Vec<_>>();
        let column_mut_ = column_types
            .iter()
            .zip(&column_is_bits)
            .map(|(t, b)| {
                if *b {
                    quote! { #bitsmut<'_, #t> }
                } else {
                    quote! { &mut #t }
                }
            })
            .collect::<Vec<_>>();
        let column_muta = column_types
            .iter()
            .zip(&column_is_bits)
            .map(|(t, b)| {
                if *b {
                    quote! { #bitsmut<'a, #t> }
                } else {
                    quote! { &'a mut #t }
                }
            })
            .collect::<Vec<_>>();
        let column_as_mut = column_indices
            .iter()
            .zip(&column_types)
            .zip(&column_stored_types)
            .zip(&column_is_bits)
            .map(|(((i, t), st), b)| {
                if *b {
                    quote! { unsafe {
                        #bitsmut::from_column_index(self.0[#i], index)
                    } }
                } else {
                    quote! { unsafe {
                         self.0[#i].cast::<#st>().add(index.get() as usize).cast::<#t>().as_mut()
                    } }
                }
            })
            .collect::<Vec<_>>();
        let column_layout = column_stored_types
            .iter()
            .zip(&column_types)
            .zip(&column_is_bits)
            .map(|((st, t), b)| {
                if *b {
                    quote! {
                        #layout::from_size_align(
                                (rows as usize).div_ceil(#items::<#t>()),
                                align_of::<#st>()
                            )?
                    }
                } else {
                    quote! {
                        #layout::from_size_align(
                                rows as usize * size_of::<#st>(),
                                align_of::<#st>()
                            )?
                    }
                }
            })
            .collect::<Vec<_>>();
        let column_read = column_indices
            .iter()
            .zip(&column_types)
            .zip(&column_stored_types)
            .zip(&column_is_bits)
            .map(|(((i, t), st), b)| {
                if *b {
                    quote! { unsafe {
                        #bitsref::from_column_index(columns[#i], index).load()
                    } }
                } else {
                    quote! { unsafe {
                        columns[#i].cast::<#st>().add(index.get() as usize).cast::<#t>().read()
                    } }
                }
            })
            .collect::<Vec<_>>();
        let column_write = column_indices
            .iter()
            .zip(&column_names)
            .zip(&column_types)
            .zip(&column_stored_types)
            .zip(&column_is_bits)
            .map(|((((i, n), t), st), b)| {
                if *b {
                    quote! { unsafe {
                        #bitsmut::from_column_index(columns[#i], index).store(self.#n)
                    } }
                } else {
                    quote! { unsafe {
                        columns[#i].cast::<#st>().add(index.get() as usize).cast::<#t>().write(self.#n)
                    } }
                }
            })
            .collect::<Vec<_>>();

        Ok(quote! {
            #vis struct #refname<'a, I: #intoindex, T> (&'a [#rawptr], #rawptr, #phantom<fn(I) -> &'a T>);
            impl #impl_generics_view #refname<'a, I, #ident #ty_generics> #where_clause_view {
                #(
                    pub fn #column_names(&self, index: I) -> #result<#column_refa> {
                        let index = index.into_index();
                        // SAFETY: self.1 is a valid pointer ot an occupation table
                        unsafe { #validate(self.1, index)? };
                        Ok(#column_as_ref)
                    }
                )*
                pub fn as_tuple(&self, index: I) -> #result<(#(#column_refa,)*)> {
                    let index = index.into_index();
                    // SAFETY: self.1 is a valid pointer ot an occupation table
                    unsafe { #validate(self.1, index)? };
                    Ok((#(#column_as_ref,)*))
                }
            }
            #vis struct #mutname<'a, I: #intoindex, T> (&'a [#rawptr], #rawptr, #phantom<fn(I) -> &'a mut T>);
            impl #impl_generics_view #mutname<'a, I, #ident #ty_generics> #where_clause_view {
                #(
                    pub fn #column_names(&self, index: I) -> #result<#column_ref_> {
                        let index = index.into_index();
                        // SAFETY: self.1 is a valid pointer ot an occupation table
                        unsafe { #validate(self.1, index)? };
                        // SAFETY: #column_indices-th column has type #column_types
                        Ok(#column_as_ref)
                    }
                    pub fn #column_names_mut(&mut self, index: I) -> #result<#column_mut_> {
                        let index = index.into_index();
                        // SAFETY: self.1 is a valid pointer ot an occupation table
                        unsafe { #validate(self.1, index)? };
                        // SAFETY: #column_indices-th column has type #column_types
                        Ok(#column_as_mut)
                    }
                    pub fn #column_names_into(self, index: I) -> #result<#column_refa> {
                        let index = index.into_index();
                        // SAFETY: self.1 is a valid pointer ot an occupation table
                        unsafe { #validate(self.1, index)? };
                        // SAFETY: #column_indices-th column has type #column_types
                        Ok(#column_as_ref)
                    }
                    pub fn #column_names_into_mut(self, index: I) -> #result<#column_muta> {
                        let index = index.into_index();
                        // SAFETY: self.1 is a valid pointer ot an occupation table
                        unsafe { #validate(self.1, index)? };
                        // SAFETY: #column_indices-th column has type #column_types
                        Ok(#column_as_mut)
                    }
                )*
                pub fn into_tuple(self, index: I) -> #result<(#(#column_refa,)*)> {
                    let index = index.into_index();
                    // SAFETY: self.1 is a valid pointer ot an occupation table
                    unsafe { #validate(self.1, index)? };
                    // SAFETY: #column_indices-th column has type #column_types
                    Ok((#(#column_as_ref,)*))
                }
                pub fn into_tuple_mut(self, index: I) -> #result<(#(#column_muta,)*)> {
                    let index = index.into_index();
                    // SAFETY: self.1 is a valid pointer ot an occupation table
                    unsafe { #validate(self.1, index)? };
                    // SAFETY: #column_indices-th column has type #column_types
                    Ok((#(#column_as_mut,)*))
                }
            }

            unsafe impl #impl_generics #columns for #ident #ty_generics #where_clause {
                const COUNT: usize = #column_count;

                type Ref<'a, I> = #refname<'a, I, Self>
                where
                    I: #intoindex + 'a,
                    Self: 'a;
                type Mut<'a, I> = #mutname<'a, I, Self>
                where
                    I: #intoindex + 'a,
                    Self: 'a;

                fn register_layout(rows: #length, register: &mut impl FnMut(#layout)) -> Result<(), #layouterr> {
                    #(register(#column_layout);)*
                    Ok(())
                }
                fn move_into(self, index: #index, columns: &[#rawptr]) {
                    #(#column_write;)*
                }
                fn take(index: #index, columns: &[#rawptr]) -> Self {
                    Self {
                        #(#default_names: #default_values),*
                        // SAFETY: column column_indices was registered to be of type #column_types
                        #(#column_names: #column_read),*
                    }
                }
                #[expect(clippy::mut_from_ref, reason = "trait user is responsible for this")]
                fn as_freelist_entry(index: #index, columns: &[#rawptr]) -> &mut Option<#index> {
                    // SAFETY: the #freelist_index-th column was registered to be of type #freelist_stored_type
                    unsafe {
                        columns[#freelist_index].cast::<#freelist_stored_type>().add(index.get() as usize)
                            .cast::<Option<#index>>().as_mut()
                    }
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
