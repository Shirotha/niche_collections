use proc_macro::{self, TokenStream};
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Meta, MetaNameValue, Attribute, Expr, Index};

fn err(msg: &'static str) -> TokenStream {
    quote! { compile_error(#msg) }.into()
}

fn find_skip_value<'a>(attrs: impl IntoIterator<Item = &'a Attribute>) -> Option<&'a Expr> {
    attrs.into_iter().filter_map(|a| {
        if let Meta::NameValue(MetaNameValue { ref value, .. }) = a.meta { Some(value) }
        else { None }
    }).next()
}

#[proc_macro_derive(Columns, attributes(skip))]
pub fn derive_columns_from_fields(input: TokenStream) -> TokenStream {
    let Ok(DeriveInput { ident, generics,
        data: Data::Struct(DataStruct { fields, .. }),
        .. }) = syn::parse(input) else { return err("invalid input: expected struct") };

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let (defaults, columns): (Vec<_>, Vec<_>) = fields.members().zip(&fields).partition(|(_, f)|
        find_skip_value(&f.attrs).is_some()
    );
    if columns.is_empty() { return err("invalid input: can't skip all fields") }

    let column_indices = (0..columns.len()).map(Index::from);
    let column_names = columns.iter().map(|(m, _)| m);
    let column_types = columns.iter().map(|(_, c)| &c.ty);

    let defaults_names = defaults.iter().map(|(m, _)| m);
    let defaults_values = defaults.iter().map(|(_, f)|
        find_skip_value(&f.attrs).expect("defaults were preselected")
    );

    let tupletype = quote! { (#(#column_types,)*) };
    let typeinit = if columns.first().map(|(_, f)| f.ident.is_some()).unwrap_or_default() {
        let column_names = column_names.clone();
        quote! { Self {
            #(#column_names: columns.#column_indices,)*
            #(#defaults_names: #defaults_values,)*
        }}
    } else {
        return err("tuple structs currently not supported");
        // TODO: how to get the correct order of arguments?
    };
    // FIXME: hygine errors
    quote! {
        unsafe impl #impl_generics Columns for #ident #ty_generics #where_clause {
            const COUNT: usize = <#tupletype as Columns>::COUNT;

            fn register_layout(
                count: Length,
                register: &mut impl FnMut(TypeId, Layout),
            ) -> Result<(), LayoutError> {
                <#tupletype as Columns>::register_layout(count, register)
            }
            fn move_into(self, index: Index, next_column: &mut impl FnMut() -> NonNull<u8>) {
                <#tupletype as Columns>::move_into((#(self.#column_names,)*), index, next_column)
            }
            fn take(index: Index, next_column: &mut impl FnMut() -> NonNull<u8>) -> Self {
                let columns = <#tupletype as Columns>::take(index, next_column);
                #typeinit
            }
            fn as_freelist_entry(
                index: Index,
                get_column: &mut impl FnMut(usize) -> NonNull<u8>,
            ) -> &mut Option<Index> {
                <#tupletype as Columns>::as_freelist_entry(index, get_column)
            }
        }
    }.into()
}
