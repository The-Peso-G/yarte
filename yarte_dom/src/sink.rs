use std::{
    borrow::{Cow, Cow::Borrowed},
    collections::BTreeMap,
    fmt::{self, Debug, Formatter},
};

use html5ever::{
    tendril::{StrTendril, TendrilSink},
    tree_builder::{
        Attribute as HtmlAttribute, ElementFlags, NodeOrText as HtmlNodeOrText, QuirksMode,
        TreeBuilderOpts, TreeSink,
    },
    ExpandedName, ParseOpts, QualName,
};

use crate::{driver, tree_builder::YARTE_TAG};

pub type ParseNodeId = usize;

#[derive(Clone)]
pub struct ParseNode {
    id: ParseNodeId,
    qual_name: Option<QualName>,
}

impl Debug for ParseNode {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("ParseNode")
            .field("id", &self.id)
            .field(
                "name",
                &self.qual_name.as_ref().map(|x| x.local.to_string()),
            )
            .finish()
    }
}

#[derive(Clone)]
pub struct ParseAttribute {
    pub name: QualName,
    pub value: String,
}

impl Debug for ParseAttribute {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("Attr")
            .field("name", &self.name.local.to_string())
            .field("value", &self.value)
            .finish()
    }
}

pub enum ParseElement {
    Mark(String),
    Node {
        name: QualName,
        attrs: Vec<ParseAttribute>,
        children: Vec<ParseNodeId>,
        parent: Option<ParseNodeId>,
    },
    Text(String),
    Document(Vec<ParseNodeId>),
}

impl Debug for ParseElement {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ParseElement::Node {
                name,
                attrs,
                children,
                parent,
            } => f
                .debug_struct("Node")
                .field("name", &name.local.to_string())
                .field("attributes", attrs)
                .field("children", children)
                .field("parent", parent)
                .finish(),
            ParseElement::Mark(s) => f.debug_tuple("Mark").field(s).finish(),
            ParseElement::Text(s) => f.debug_tuple("Text").field(s).finish(),
            ParseElement::Document(s) => f.debug_tuple("Document").field(s).finish(),
        }
    }
}

#[derive(Debug, Default)]
pub struct Sink {
    count: usize,
    pub nodes: BTreeMap<ParseNodeId, ParseElement>,
    fragment: bool,
    err: Vec<ParseError>,
}

impl Sink {
    fn new_parse_node(&mut self) -> ParseNode {
        let id = self.count;
        self.count += 1;
        ParseNode {
            id,
            qual_name: None,
        }
    }

    fn append_child(
        &mut self,
        p: ParseNodeId,
        child: HtmlNodeOrText<<Self as TreeSink>::Handle>,
    ) -> ParseNodeId {
        match child {
            HtmlNodeOrText::AppendNode(node) => {
                self.nodes
                    .get_mut(&node.id)
                    .and_then(|x| match x {
                        ParseElement::Node { parent, name, .. } => {
                            if name != &*YARTE_TAG {
                                *parent = Some(p);
                            }
                            Some(())
                        }
                        ParseElement::Mark(_) => Some(()),
                        _ => None,
                    })
                    .expect("Get parent");
                node.id
            }
            HtmlNodeOrText::AppendText(text) => {
                let id = self.count;
                self.count += 1;
                self.nodes.insert(id, ParseElement::Text(text.to_string()));
                id
            }
        }
    }
}

#[derive(Debug)]
pub struct ParseError(Cow<'static, str>);

pub type ParseResult<T> = Result<T, Vec<ParseError>>;

pub const MARK: &str = "yarteHashHTMLExpressionsATTT";
pub const HEAD: &str = "<!--yarteHashHTMLExpressionsATTT";
pub const TAIL: &str = "-->";

impl TreeSink for Sink {
    type Handle = ParseNode;
    type Output = ParseResult<Self>;

    fn finish(self) -> Self::Output {
        if self.err.is_empty() {
            Ok(self)
        } else {
            Err(self.err)
        }
    }

    fn parse_error(&mut self, msg: Cow<'static, str>) {
        self.err.push(ParseError(msg))
    }

    fn get_document(&mut self) -> Self::Handle {
        let node = self.new_parse_node();
        self.fragment = node.id != 0;
        node
    }

    fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> ExpandedName<'a> {
        target
            .qual_name
            .as_ref()
            .expect("Expected qual name of node!")
            .expanded()
    }

    fn create_element(
        &mut self,
        name: QualName,
        html_attrs: Vec<HtmlAttribute>,
        _flags: ElementFlags,
    ) -> Self::Handle {
        let mut new_node = self.new_parse_node();
        new_node.qual_name = Some(name.clone());
        let attrs = html_attrs
            .into_iter()
            .map(|attr| ParseAttribute {
                name: attr.name,
                value: String::from(attr.value),
            })
            .collect();

        self.nodes.insert(
            new_node.id,
            ParseElement::Node {
                name,
                attrs,
                children: vec![],
                parent: None,
            },
        );

        new_node
    }

    fn create_comment(&mut self, text: StrTendril) -> Self::Handle {
        let node = self.new_parse_node();
        if text.as_bytes().starts_with(MARK.as_bytes()) {
            self.nodes.insert(
                node.id,
                ParseElement::Mark(
                    text.to_string()
                        .get(MARK.len()..)
                        .expect("SOME")
                        .to_string(),
                ),
            );
        } else {
            self.parse_error(Borrowed("No use html comment, use yarte comments instead"))
        }

        node
    }

    #[allow(unused_variables)]
    fn create_pi(&mut self, target: StrTendril, data: StrTendril) -> Self::Handle {
        unreachable!()
    }

    fn append(&mut self, p: &Self::Handle, child: HtmlNodeOrText<Self::Handle>) {
        let id = self.append_child(p.id, child);

        match self.nodes.get_mut(&p.id) {
            Some(ParseElement::Document(children)) | Some(ParseElement::Node { children, .. }) => {
                children.push(id);
            }
            _ if p.id == 0 || self.fragment => (),
            _ => panic!("append without parent {:?}, {:?} {:?}", p, id, self.nodes),
        };
    }

    fn append_based_on_parent_node(
        &mut self,
        _: &Self::Handle,
        _: &Self::Handle,
        _: HtmlNodeOrText<Self::Handle>,
    ) {
        unreachable!()
    }

    fn append_doctype_to_document(&mut self, _: StrTendril, _: StrTendril, _: StrTendril) {
        if self
            .nodes
            .insert(0, ParseElement::Document(vec![]))
            .is_some()
        {
            panic!("Double Doctype")
        }
    }

    fn get_template_contents(&mut self, target: &Self::Handle) -> Self::Handle {
        target.clone()
    }

    fn same_node(&self, x: &Self::Handle, y: &Self::Handle) -> bool {
        x.id == y.id
    }

    fn set_quirks_mode(&mut self, _mode: QuirksMode) {
        unreachable!()
    }

    fn append_before_sibling(&mut self, _: &Self::Handle, _: HtmlNodeOrText<Self::Handle>) {
        unreachable!()
    }

    fn add_attrs_if_missing(&mut self, _: &Self::Handle, _: Vec<HtmlAttribute>) {
        unreachable!()
    }

    fn remove_from_parent(&mut self, _target: &Self::Handle) {
        unreachable!()
    }

    fn reparent_children(&mut self, _node: &Self::Handle, _new_parent: &Self::Handle) {
        unreachable!()
    }
}

pub fn parse_document(doc: &str) -> ParseResult<Sink> {
    let parser = driver::parse_document(
        Sink::default(),
        ParseOpts {
            tree_builder: TreeBuilderOpts {
                exact_errors: cfg!(debug_assertions),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .from_utf8();

    parser.one(doc.as_bytes())
}

pub fn parse_fragment(doc: &str) -> ParseResult<Sink> {
    let parser = driver::parse_fragment(
        Sink::default(),
        ParseOpts {
            tree_builder: TreeBuilderOpts {
                exact_errors: cfg!(debug_assertions),
                ..Default::default()
            },
            ..Default::default()
        },
        YARTE_TAG.clone(),
        vec![],
    )
    .from_utf8();
    parser.one(doc.as_bytes()).and_then(|mut a| {
        a.nodes
            .remove(&0)
            .and_then(|_| {
                if let Some(ParseElement::Node { name, .. }) = a.nodes.get_mut(&2) {
                    *name = YARTE_TAG.clone();
                    Some(a)
                } else {
                    None
                }
            })
            .ok_or_else(|| vec![])
    })
}
