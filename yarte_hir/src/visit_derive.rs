use std::path::PathBuf;

use quote::quote;
use syn::visit::Visit;

use yarte_config::Config;

use proc_macro2::TokenStream;
use syn::{parse_str, ItemEnum};

pub fn visit_derive<'a>(i: &'a syn::DeriveInput, config: &Config) -> Struct<'a> {
    StructBuilder::default().build(i, config)
}

#[derive(Debug)]
pub struct Struct<'a> {
    pub src: String,
    pub path: PathBuf,
    pub print: Print,
    pub mode: Mode,
    pub err_msg: String,
    pub msgs: Option<ItemEnum>,
    pub script: Option<String>,
    pub fields: Vec<syn::Field>,
    pub ident: &'a syn::Ident,
    generics: &'a syn::Generics,
}

impl<'a> Struct<'a> {
    pub fn implement_head(&self, t: TokenStream, body: &TokenStream) -> TokenStream {
        let Struct {
            ident, generics, ..
        } = *self;
        let (impl_generics, orig_ty_generics, where_clause) = generics.split_for_impl();

        quote!(impl#impl_generics #t for #ident #orig_ty_generics #where_clause { #body })
    }
}

struct StructBuilder {
    err_msg: Option<String>,
    ext: Option<String>,
    fields: Vec<syn::Field>,
    mode: Option<String>,
    path: Option<String>,
    print: Option<String>,
    script: Option<String>,
    src: Option<String>,
}

impl Default for StructBuilder {
    fn default() -> Self {
        StructBuilder {
            err_msg: None,
            ext: None,
            fields: vec![],
            mode: None,
            path: None,
            print: None,
            script: None,
            src: None,
        }
    }
}

impl StructBuilder {
    fn build<'n>(
        mut self,
        syn::DeriveInput {
            attrs,
            ident,
            generics,
            data,
            ..
        }: &'n syn::DeriveInput,
        config: &Config,
    ) -> Struct<'n> {
        let mut msgs = None;
        for i in attrs {
            if i.path.is_ident("template") {
                self.visit_meta(&i.parse_meta().expect("valid meta attributes"));
            } else if i.path.is_ident("msg") {
                let tokens = i.tokens.to_string();
                let tokens = tokens.get(1..tokens.len() - 1).expect("some");
                let enu: ItemEnum = parse_str(tokens).expect("valid enum");
                msgs = Some(enu);
            }
        }

        self.visit_data(data);

        let (path, src) = match (self.src, self.ext) {
            (Some(src), ext) => (
                config.get_dir().join(
                    PathBuf::from(quote!(#ident).to_string())
                        .with_extension(ext.unwrap_or_else(|| DEFAULT_EXTENSION.to_owned())),
                ),
                src.trim_end().to_owned(),
            ),
            (None, None) => config.get_template(&self.path.expect("some valid path")),
            (None, Some(_)) => panic!("'ext' attribute cannot be used with 'path' attribute"),
        };

        let mode = self.mode.map(Into::into).unwrap_or_else(|| {
            if let Some(e) = path.extension() {
                if HTML_EXTENSIONS.contains(&e.to_str().unwrap()) {
                    return Mode::HTMLMin;
                }
            }

            Mode::Text
        });

        Struct {
            err_msg: self
                .err_msg
                .unwrap_or_else(|| "Template parsing error".into()),
            fields: self.fields,
            generics,
            ident,
            mode,
            msgs,
            path,
            print: self.print.into(),
            script: self.script,
            src,
        }
    }
}

impl<'a> Visit<'a> for StructBuilder {
    fn visit_data(&mut self, i: &'a syn::Data) {
        use syn::Data::*;
        match i {
            Struct(ref i) => {
                self.visit_data_struct(i);
            }
            Enum(_) | Union(_) => panic!("Not valid need a `struc`"),
        }
    }

    fn visit_field(&mut self, e: &'a syn::Field) {
        self.fields.push(e.clone());
    }

    fn visit_meta_name_value(
        &mut self,
        syn::MetaNameValue { path, lit, .. }: &'a syn::MetaNameValue,
    ) {
        if path.is_ident("path") {
            if let syn::Lit::Str(ref s) = lit {
                if self.src.is_some() {
                    panic!("must specify 'src' or 'path', not both");
                }
                self.path = Some(s.value());
            } else {
                panic!("attribute 'path' must be string literal");
            }
        } else if path.is_ident("src") {
            if let syn::Lit::Str(ref s) = lit {
                if self.path.is_some() {
                    panic!("must specify 'src' or 'path', not both");
                }
                self.src = Some(s.value());
            } else {
                panic!("attribute 'src' must be string literal");
            }
        } else if path.is_ident("print") {
            if let syn::Lit::Str(ref s) = lit {
                self.print = Some(s.value());
            } else {
                panic!("attribute 'print' must be string literal");
            }
        } else if path.is_ident("mode") {
            if let syn::Lit::Str(ref s) = lit {
                self.mode = Some(s.value());
            } else {
                panic!("attribute 'mode' must be string literal");
            }
        } else if path.is_ident("ext") {
            if let syn::Lit::Str(ref s) = lit {
                self.ext = Some(s.value());
            } else {
                panic!("attribute 'ext' must be string literal");
            }
        } else if path.is_ident("script") {
            if let syn::Lit::Str(ref s) = lit {
                self.script = Some(s.value());
            } else {
                panic!("attribute 'script' must be string literal");
            }
        } else if cfg!(feature = "actix-web") && path.is_ident("err") {
            if let syn::Lit::Str(ref s) = lit {
                self.err_msg = Some(s.value());
            } else {
                panic!("attribute 'err' must be string literal");
            }
        } else {
            panic!("invalid attribute '{:?}'", path.get_ident());
        }
    }
}

#[derive(PartialEq, Debug)]
pub enum Print {
    All,
    Ast,
    Code,
    None,
}

impl From<Option<String>> for Print {
    fn from(s: Option<String>) -> Print {
        match s {
            Some(s) => match s.as_ref() {
                "all" => Print::All,
                "ast" => Print::Ast,
                "code" => Print::Code,
                v => panic!("invalid value for print attribute: {}", v),
            },
            None => Print::None,
        }
    }
}

#[derive(PartialEq, Debug)]
pub enum Mode {
    Text,
    HTML,
    HTMLMin,
    WASM,
    WASMServer,
}

impl From<String> for Mode {
    fn from(s: String) -> Mode {
        match s.as_ref() {
            "text" => Mode::Text,
            "html" => Mode::HTML,
            "html-min" => Mode::HTMLMin,
            "wasm" | "client" | "front" => Mode::WASM,
            "wasm-server" | "iso" | "server" | "back" => Mode::WASMServer,
            v => panic!("invalid value for mode attribute: {}", v),
        }
    }
}

static DEFAULT_EXTENSION: &str = "hbs";
static HTML_EXTENSIONS: [&str; 6] = [
    DEFAULT_EXTENSION,
    "htm",
    "xml",
    "html",
    "handlebars",
    "mustache",
];

#[cfg(test)]
mod test {
    use super::*;
    use syn::parse_str;

    #[test]
    #[should_panic]
    fn test_panic() {
        let src = r#"
            #[derive(Template)]
            #[template(path = "no-exist.html")]
            struct Test;
        "#;
        let i = parse_str::<syn::DeriveInput>(src).unwrap();
        let config = Config::new("");
        let _ = visit_derive(&i, &config);
    }

    #[test]
    fn test() {
        let src = r#"
            #[derive(Template)]
            #[template(src = "", ext = "txt", print = "code")]
            struct Test;
        "#;
        let i = parse_str::<syn::DeriveInput>(src).unwrap();
        let config = Config::new("");
        let s = visit_derive(&i, &config);

        assert_eq!(s.src, "");
        assert_eq!(s.path, config.get_dir().join(PathBuf::from("Test.txt")));
        assert_eq!(s.print, Print::Code);
        assert_eq!(s.mode, Mode::Text);
    }

    #[test]
    fn test_msg() {
        let src = r#"
            #[derive(Template)]
            #[template(src = "", ext = "txt", mode = "wasm")]
            #[msg(enum Msg {
                #[foo::bar]
                Foo(usize, Bar)
            })]
            struct Test;
        "#;
        let i = parse_str::<syn::DeriveInput>(src).unwrap();
        let config = Config::new("");
        let s = visit_derive(&i, &config);

        assert_eq!(s.mode, Mode::WASM);
    }
}
