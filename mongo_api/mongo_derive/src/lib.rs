use proc_macro::TokenStream;
use quote::quote;
use syn;

#[proc_macro_derive(MongoModel)]
pub fn model_macro_derive(input: TokenStream) -> TokenStream {
    // Construct a representation of Rust code as a syntax tree
    // that we can manipulate
    let ast = syn::parse(input).expect("Model macro needs to be changed");

    // Build the trait implementation
    impl_model_trait(&ast)
}

fn impl_model_trait(ast: &syn::DeriveInput) -> TokenStream {    
    let name = &ast.ident;
    let gen = quote! {
        impl MongoDbModel for #name {
            fn model_name()-> String {
                stringify!(#name).to_owned()
            }
        }
    };
    gen.into()
}