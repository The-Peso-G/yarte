// Copyright 2014-2017 The html5ever Project Developers. See the
// COPYRIGHT file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! The HTML5 tree builder.

#![allow(
    clippy::cognitive_complexity,
    clippy::redundant_static_lifetimes,
    clippy::suspicious_else_formatting,
    clippy::unused_unit,
    clippy::wrong_self_convention
)]

use std::{
    borrow::Cow::Borrowed,
    collections::VecDeque,
    fmt,
    iter::{Enumerate, Rev},
    slice,
};

use self::types::*;

use html5ever::{
    interface::{create_element, AppendNode, AppendText, Attribute, NodeOrText, TreeSink},
    tendril::StrTendril,
    tokenizer::{self, Doctype, EndTag, StartTag, Tag, TokenSink, TokenSinkResult},
    ExpandedName, LocalName, Namespace, QualName,
};

use self::tag_sets::*;
use html5ever::tokenizer::states::RawKind;
use log::{debug, log_enabled, Level};
use mac::{_tt_as_expr_hack, format_if, matches};

use lazy_static::lazy_static;

/// ASCII whitespace characters, as defined by
/// tree construction modes that treat them specially.
pub fn is_ascii_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\r' | '\n' | '\x0C' | ' ')
}

pub fn to_escaped_string<T: fmt::Debug>(x: &T) -> String {
    // FIXME: don't allocate twice
    let string = format!("{:?}", x);
    string.chars().flat_map(|c| c.escape_default()).collect()
}

pub use self::PushFlag::*;
use html5ever::tree_builder::TreeBuilderOpts;

#[macro_use]
mod tag_sets;

mod data;
mod types;

include!(concat!(env!("OUT_DIR"), "/rules.rs"));

/// The HTML tree builder.
pub struct TreeBuilder<Handle, Sink> {
    /// Options controlling the behavior of the tree builder.
    opts: TreeBuilderOpts,

    /// Consumer of tree modifications.
    pub sink: Sink,

    /// Insertion mode.
    mode: InsertionMode,

    /// Original insertion mode, used by Text and InTableText modes.
    orig_mode: Option<InsertionMode>,

    /// The document node, which is created by the sink.
    doc_handle: Handle,

    /// Stack of open elements, most recently added at end.
    open_elems: Vec<Handle>,

    /// List of active formatting elements.
    active_formatting: Vec<FormatEntry<Handle>>,

    //§ END
    /// The context element for the fragment parsing algorithm.
    context_elem: Option<Handle>,
}

lazy_static! {
    pub static ref YARTE_TAG: QualName = QualName::new(None, ns!(html), LocalName::from("marquee"));
}

impl<Handle, Sink> TreeBuilder<Handle, Sink>
where
    Handle: Clone,
    Sink: TreeSink<Handle = Handle>,
{
    /// Create a new tree builder which sends tree modifications to a particular `TreeSink`.
    ///
    /// The tree builder is also a `TokenSink`.
    pub fn new(mut sink: Sink, opts: TreeBuilderOpts) -> TreeBuilder<Handle, Sink> {
        let doc_handle = sink.get_document();
        TreeBuilder {
            opts,
            sink,
            mode: Initial,
            orig_mode: None,
            doc_handle,
            open_elems: vec![],
            active_formatting: vec![],
            context_elem: None,
        }
    }

    /// Create a new tree builder which sends tree modifications to a particular `TreeSink`.
    /// This is for parsing fragments.
    ///
    /// The tree builder is also a `TokenSink`.
    pub fn new_for_fragment(
        mut sink: Sink,
        context_elem: Handle,
        opts: TreeBuilderOpts,
    ) -> TreeBuilder<Handle, Sink> {
        let doc_handle = sink.get_document();
        let mut tb = TreeBuilder {
            opts,
            sink,
            mode: InHtml,
            orig_mode: None,
            doc_handle,
            open_elems: vec![],
            active_formatting: vec![],
            context_elem: Some(context_elem),
        };

        tb.create_root(vec![]);

        tb
    }

    fn debug_step(&self, mode: InsertionMode, token: &Token) {
        if log_enabled!(Level::Debug) {
            debug!(
                "processing {} in insertion mode {:?}",
                to_escaped_string(token),
                mode
            );
        }
    }

    fn process_to_completion(&mut self, mut token: Token) -> TokenSinkResult<Handle> {
        // Queue of additional tokens yet to be processed.
        // This stays empty in the common case where we don't split whitespace.
        let mut more_tokens = VecDeque::new();

        loop {
            let should_have_acknowledged_self_closing_flag = matches!(
                token,
                TagToken(Tag {
                    self_closing: true,
                    kind: StartTag,
                    ..
                })
            );
            let result = if self.is_foreign(&token) {
                self.step_foreign(token.clone())
            } else {
                let mode = self.mode;

                self.step(mode, token.clone())
            };
            match result {
                Done => {
                    if should_have_acknowledged_self_closing_flag {
                        self.sink
                            .parse_error(Borrowed("Unacknowledged self-closing tag"));
                    }
                    token = unwrap_or_return!(
                        more_tokens.pop_front(),
                        tokenizer::TokenSinkResult::Continue
                    );
                }
                DoneAckSelfClosing => {
                    token = unwrap_or_return!(
                        more_tokens.pop_front(),
                        tokenizer::TokenSinkResult::Continue
                    );
                }
                Reprocess(m, t) => {
                    self.mode = m;
                    token = t;
                }
                ReprocessForeign(t) => {
                    token = t;
                }
                SplitWhitespace(mut buf) => {
                    let p = buf.pop_front_char_run(is_ascii_whitespace);
                    let (first, is_ws) = unwrap_or_return!(p, tokenizer::TokenSinkResult::Continue);
                    let status = if is_ws { Whitespace } else { NotWhitespace };
                    token = CharacterTokens(status, first);

                    if buf.len32() > 0 {
                        more_tokens.push_back(CharacterTokens(NotSplit, buf));
                    }
                }
                Script(node) => {
                    assert!(more_tokens.is_empty());
                    return tokenizer::TokenSinkResult::Script(node);
                }
                ToPlaintext => {
                    assert!(more_tokens.is_empty());
                    return tokenizer::TokenSinkResult::Plaintext;
                }
                ToRawData(k) => {
                    assert!(more_tokens.is_empty());
                    return tokenizer::TokenSinkResult::RawData(k);
                }
            }
        }
    }

    /// Are we parsing a HTML fragment?
    pub fn is_fragment(&self) -> bool {
        self.context_elem.is_some()
    }

    fn appropriate_place_for_insertion(&mut self, override_target: Option<Handle>) -> Handle {
        override_target.unwrap_or_else(|| self.current_node().clone())
    }

    fn insert_at(&mut self, insertion_point: Handle, child: NodeOrText<Handle>) {
        self.sink.append(&insertion_point, child)
    }
}

impl<Handle, Sink> TokenSink for TreeBuilder<Handle, Sink>
where
    Handle: Clone,
    Sink: TreeSink<Handle = Handle>,
{
    type Handle = Handle;

    fn process_token(
        &mut self,
        token: tokenizer::Token,
        _line_number: u64,
    ) -> TokenSinkResult<Handle> {
        // Handle `ParseError` and `DoctypeToken`; convert everything else to the local `Token` type.
        let token = match token {
            tokenizer::ParseError(e) => {
                self.sink.parse_error(e);
                return tokenizer::TokenSinkResult::Continue;
            }

            tokenizer::DoctypeToken(dt) => {
                if self.mode == Initial {
                    if data::doctype_error(&dt) {
                        self.sink.parse_error(format_if!(
                            self.opts.exact_errors,
                            "Bad DOCTYPE",
                            "Bad DOCTYPE: {:?}",
                            dt
                        ));
                    }
                    let Doctype {
                        name,
                        public_id,
                        system_id,
                        ..
                    } = dt;
                    if !self.opts.drop_doctype {
                        self.sink.append_doctype_to_document(
                            name.unwrap_or_default(),
                            public_id.unwrap_or_default(),
                            system_id.unwrap_or_default(),
                        );
                    }

                    self.mode = BeforeHtml;
                    return tokenizer::TokenSinkResult::Continue;
                } else {
                    self.sink.parse_error(format_if!(
                        self.opts.exact_errors,
                        "DOCTYPE in body",
                        "DOCTYPE in insertion mode {:?}",
                        self.mode
                    ));
                    return tokenizer::TokenSinkResult::Continue;
                }
            }

            tokenizer::TagToken(x) => TagToken(x),
            tokenizer::CommentToken(x) => CommentToken(x),
            tokenizer::NullCharacterToken => NullCharacterToken,
            tokenizer::EOFToken => EOFToken,

            tokenizer::CharacterTokens(x) => {
                if x.is_empty() {
                    return tokenizer::TokenSinkResult::Continue;
                }
                CharacterTokens(NotSplit, x)
            }
        };

        self.process_to_completion(token)
    }

    fn end(&mut self) {
        for elem in self.open_elems.drain(..).rev() {
            self.sink.pop(&elem);
        }
    }

    fn adjusted_current_node_present_but_not_in_html_namespace(&self) -> bool {
        !self.open_elems.is_empty()
            && self.sink.elem_name(self.adjusted_current_node()).ns != &ns!(html)
    }
}

pub struct ActiveFormattingIter<'a, Handle: 'a> {
    iter: Rev<Enumerate<slice::Iter<'a, FormatEntry<Handle>>>>,
}

impl<'a, Handle> Iterator for ActiveFormattingIter<'a, Handle> {
    type Item = (usize, &'a Handle, &'a Tag);
    fn next(&mut self) -> Option<(usize, &'a Handle, &'a Tag)> {
        match self.iter.next() {
            None => None,
            Some((i, &Element(ref h, ref t))) => Some((i, h, t)),
        }
    }
}

pub enum PushFlag {
    Push,
    NoPush,
}

macro_rules! qualname {
    ("", $local:tt) => {
        QualName {
            prefix: None,
            ns: ns!(),
            local: local_name!($local),
        }
    };
    ($prefix: tt $ns:tt $local:tt) => {
        QualName {
            prefix: Some(namespace_prefix!($prefix)),
            ns: ns!($ns),
            local: local_name!($local),
        }
    };
}

#[doc(hidden)]
impl<Handle, Sink> TreeBuilder<Handle, Sink>
where
    Handle: Clone,
    Sink: TreeSink<Handle = Handle>,
{
    fn unexpected<T: fmt::Debug>(&mut self, _thing: &T) -> ProcessResult<Handle> {
        self.sink.parse_error(format_if!(
            self.opts.exact_errors,
            "Unexpected token",
            "Unexpected token {} in insertion mode {:?}",
            to_escaped_string(_thing),
            self.mode
        ));
        Done
    }

    /// Iterate over the active formatting elements (with index in the list) from the end
    /// to the last marker, or the beginning if there are no markers.
    fn active_formatting_end_to_marker(&self) -> ActiveFormattingIter<Handle> {
        ActiveFormattingIter {
            iter: self.active_formatting.iter().enumerate().rev(),
        }
    }

    fn position_in_active_formatting(&self, element: &Handle) -> Option<usize> {
        // TODO
        self.active_formatting.iter().position(|n| match n {
            Element(ref handle, _) => self.sink.same_node(handle, element),
        })
    }

    fn stop_parsing(&mut self) -> ProcessResult<Handle> {
        Done
    }

    //§ parsing-elements-that-contain-only-text
    // Switch to `Text` insertion mode, save the old mode, and
    // switch the tokenizer to a raw-data state.
    // The latter only takes effect after the current / next
    // `process_token` of a start tag returns!
    fn to_raw_text_mode(&mut self, k: RawKind) -> ProcessResult<Handle> {
        self.orig_mode = Some(self.mode);
        self.mode = RawText;
        ToRawData(k)
    }

    // The generic raw text / RCDATA parsing algorithm.
    fn parse_raw_data(&mut self, tag: Tag, k: RawKind) -> ProcessResult<Handle> {
        self.insert_element_for(tag);
        self.to_raw_text_mode(k)
    }
    //§ END

    fn current_node(&self) -> &Handle {
        self.open_elems.last().expect("no current element")
    }

    fn adjusted_current_node(&self) -> &Handle {
        if self.open_elems.len() == 1 {
            if let Some(ctx) = self.context_elem.as_ref() {
                return ctx;
            }
        }
        self.current_node()
    }

    fn current_node_in<TagSet>(&self, set: TagSet) -> bool
    where
        TagSet: Fn(ExpandedName) -> bool,
    {
        set(self.sink.elem_name(self.current_node()))
    }

    // Insert at the "appropriate place for inserting a node".
    fn insert_appropriately(&mut self, child: NodeOrText<Handle>, override_target: Option<Handle>) {
        let insertion_point = self.appropriate_place_for_insertion(override_target);
        self.insert_at(insertion_point, child);
    }

    fn adoption_agency(&mut self, subject: LocalName) {
        // 1.
        if self.current_node_named(subject.clone())
            && self
                .position_in_active_formatting(self.current_node())
                .is_none()
        {
            self.pop();
            return;
        }

        // 5.
        let (fmt_elem_index, fmt_elem, _) = unwrap_or_return!(
            // We clone the Handle and Tag so they don't cause an immutable borrow of self.
            self.active_formatting_end_to_marker()
                .find(|&(_, _, tag)| tag.name == subject)
                .map(|(i, h, t)| (i, h.clone(), t.clone())),
            {
                self.process_end_tag_in_body(Tag {
                    kind: EndTag,
                    name: subject,
                    self_closing: false,
                    attrs: vec![],
                });
            }
        );

        let fmt_elem_stack_index = unwrap_or_return!(
            self.open_elems
                .iter()
                .rposition(|n| self.sink.same_node(n, &fmt_elem)),
            {
                self.sink
                    .parse_error(Borrowed("Formatting element not open"));
                self.active_formatting.remove(fmt_elem_index);
            }
        );

        // 7.
        if !self.in_scope(default_scope, |n| self.sink.same_node(&n, &fmt_elem)) {
            self.sink
                .parse_error(Borrowed("Formatting element not in scope"));
            return;
        }

        // 8.
        if !self.sink.same_node(self.current_node(), &fmt_elem) {
            self.sink
                .parse_error(Borrowed("Formatting element not current node"));
        }

        // 9.
        self.open_elems.truncate(fmt_elem_stack_index);
        self.active_formatting.remove(fmt_elem_index);
    }

    fn push(&mut self, elem: &Handle) {
        self.open_elems.push(elem.clone());
    }

    fn pop(&mut self) -> Handle {
        let elem = self.open_elems.pop().expect("no current element");
        self.sink.pop(&elem);
        elem
    }

    fn remove_from_stack(&mut self, elem: &Handle) {
        let sink = &mut self.sink;
        let position = self
            .open_elems
            .iter()
            .rposition(|x| sink.same_node(elem, &x));
        if let Some(position) = position {
            self.open_elems.remove(position);
            sink.pop(elem);
        }
    }

    fn is_marker_or_open(&self, entry: &FormatEntry<Handle>) -> bool {
        // TODO
        match *entry {
            Element(ref node, _) => self
                .open_elems
                .iter()
                .rev()
                .any(|n| self.sink.same_node(&n, &node)),
        }
    }

    /// Reconstruct the active formatting elements.
    fn reconstruct_formatting(&mut self) {
        {
            let last = unwrap_or_return!(self.active_formatting.last(), ());
            if self.is_marker_or_open(last) {
                return;
            }
        }

        let mut entry_index = self.active_formatting.len() - 1;
        loop {
            if entry_index == 0 {
                break;
            }
            entry_index -= 1;
            if self.is_marker_or_open(&self.active_formatting[entry_index]) {
                entry_index += 1;
                break;
            }
        }

        loop {
            // TODO
            let tag = match self.active_formatting[entry_index] {
                Element(_, ref t) => t.clone(),
            };

            // FIXME: Is there a way to avoid cloning the attributes twice here (once on their own,
            // once as part of t.clone() above)?
            let new_element =
                self.insert_element(Push, ns!(html), tag.name.clone(), tag.attrs.clone());
            self.active_formatting[entry_index] = Element(new_element, tag);
            if entry_index == self.active_formatting.len() - 1 {
                break;
            }
            entry_index += 1;
        }
    }

    /// Signal an error depending on the state of the stack of open elements at
    /// the end of the body.
    fn check_body_end(&mut self) {
        declare_tag_set!(body_end_ok =
            "dd" "dt" "li" "optgroup" "option" "p" "rp" "rt" "tbody" "td" "tfoot" "th"
            "thead" "tr" "body" "html");

        for elem in self.open_elems.iter() {
            let error;
            {
                let name = self.sink.elem_name(elem);
                if body_end_ok(name) {
                    continue;
                }
                error = format_if!(
                    self.opts.exact_errors,
                    "Unexpected open tag at end of body",
                    "Unexpected open tag {:?} at end of body",
                    name
                );
            }
            self.sink.parse_error(error);
            // FIXME: Do we keep checking after finding one bad tag?
            // The spec suggests not.
            return;
        }
    }

    fn in_scope<TagSet, Pred>(&self, scope: TagSet, pred: Pred) -> bool
    where
        TagSet: Fn(ExpandedName) -> bool,
        Pred: Fn(Handle) -> bool,
    {
        for node in self.open_elems.iter().rev() {
            if pred(node.clone()) {
                return true;
            }
            if scope(self.sink.elem_name(node)) {
                return false;
            }
        }

        // supposed to be impossible, because <html> is always in scope

        false
    }

    fn elem_in<TagSet>(&self, elem: &Handle, set: TagSet) -> bool
    where
        TagSet: Fn(ExpandedName) -> bool,
    {
        set(self.sink.elem_name(elem))
    }

    fn html_elem_named(&self, elem: &Handle, name: LocalName) -> bool {
        let expanded = self.sink.elem_name(elem);
        *expanded.ns == ns!(html) && *expanded.local == name
    }

    fn current_node_named(&self, name: LocalName) -> bool {
        self.html_elem_named(self.current_node(), name)
    }

    fn in_scope_named<TagSet>(&self, scope: TagSet, name: LocalName) -> bool
    where
        TagSet: Fn(ExpandedName) -> bool,
    {
        self.in_scope(scope, |elem| self.html_elem_named(&elem, name.clone()))
    }

    //§ closing-elements-that-have-implied-end-tags
    fn generate_implied_end<TagSet>(&mut self, set: TagSet)
    where
        TagSet: Fn(ExpandedName) -> bool,
    {
        loop {
            {
                let elem = unwrap_or_return!(self.open_elems.last(), ());
                let nsname = self.sink.elem_name(elem);
                if !set(nsname) {
                    return;
                }
            }
            self.pop();
        }
    }

    fn generate_implied_end_except(&mut self, except: LocalName) {
        self.generate_implied_end(|p| {
            if *p.ns == ns!(html) && *p.local == except {
                false
            } else {
                cursory_implied_end(p)
            }
        });
    }
    //§ END

    // Pop elements until an element from the set has been popped.  Returns the
    // number of elements popped.
    fn pop_until<P>(&mut self, pred: P) -> usize
    where
        P: Fn(ExpandedName) -> bool,
    {
        let mut n = 0;
        loop {
            n += 1;
            match self.open_elems.pop() {
                None => break,
                Some(elem) => {
                    if pred(self.sink.elem_name(&elem)) {
                        break;
                    }
                }
            }
        }
        n
    }

    fn pop_until_named(&mut self, name: LocalName) -> usize {
        self.pop_until(|p| *p.ns == ns!(html) && *p.local == name)
    }

    // Pop elements until one with the specified name has been popped.
    // Signal an error if it was not the first one.
    fn expect_to_close(&mut self, name: LocalName) {
        if self.pop_until_named(name.clone()) != 1 {
            self.sink.parse_error(format_if!(
                self.opts.exact_errors,
                "Unexpected open element",
                "Unexpected open element while closing {:?}",
                name
            ));
        }
    }

    fn close_p_element(&mut self) {
        declare_tag_set!(implied = [cursory_implied_end] - "p");
        self.generate_implied_end(implied);
        self.expect_to_close(local_name!("p"));
    }

    fn append_text(&mut self, text: StrTendril) -> ProcessResult<Handle> {
        self.insert_appropriately(AppendText(text), None);
        Done
    }

    fn append_comment(&mut self, text: StrTendril) -> ProcessResult<Handle> {
        let comment = self.sink.create_comment(text);
        self.insert_appropriately(AppendNode(comment), None);
        Done
    }

    //§ creating-and-inserting-nodes
    fn create_root(&mut self, attrs: Vec<Attribute>) {
        let elem = create_element(
            &mut self.sink,
            QualName::new(None, ns!(html), local_name!("html")),
            attrs,
        );
        self.push(&elem);
        self.sink.append(&self.doc_handle, AppendNode(elem));
        // FIXME: application cache selection algorithm
    }

    fn insert_element(
        &mut self,
        push: PushFlag,
        ns: Namespace,
        name: LocalName,
        attrs: Vec<Attribute>,
    ) -> Handle {
        // Step 7.
        let qname = QualName::new(None, ns, name);
        let elem = create_element(&mut self.sink, qname, attrs);

        let insertion_point = self.appropriate_place_for_insertion(None);
        self.insert_at(insertion_point, AppendNode(elem.clone()));

        match push {
            Push => self.push(&elem),
            NoPush => (),
        }
        // FIXME: Remove from the stack if we can't append?
        elem
    }

    fn insert_element_for(&mut self, tag: Tag) -> Handle {
        self.insert_element(Push, ns!(html), tag.name, tag.attrs)
    }

    fn insert_and_pop_element_for(&mut self, tag: Tag) -> Handle {
        self.insert_element(NoPush, ns!(html), tag.name, tag.attrs)
    }

    fn insert_phantom(&mut self, name: LocalName) -> Handle {
        self.insert_element(Push, ns!(html), name, vec![])
    }
    //§ END

    fn create_formatting_element_for(&mut self, tag: Tag) -> Handle {
        // FIXME: This really wants unit tests.
        let mut first_match = None;
        let mut matches = 0usize;
        for (i, _, old_tag) in self.active_formatting_end_to_marker() {
            if tag.equiv_modulo_attr_order(old_tag) {
                first_match = Some(i);
                matches += 1;
            }
        }

        if matches >= 3 {
            self.active_formatting
                .remove(first_match.expect("matches with no index"));
        }

        let elem = self.insert_element(Push, ns!(html), tag.name.clone(), tag.attrs.clone());
        self.active_formatting.push(Element(elem.clone(), tag));
        elem
    }

    fn process_end_tag_in_body(&mut self, tag: Tag) {
        // Look back for a matching open element.
        let mut match_idx = None;
        for (i, elem) in self.open_elems.iter().enumerate().rev() {
            if self.html_elem_named(elem, tag.name.clone()) {
                match_idx = Some(i);
                break;
            }

            if self.elem_in(elem, special_tag) {
                self.sink
                    .parse_error(Borrowed("Found special tag while closing generic tag"));
                return;
            }
        }

        // Can't use unwrap_or_return!() due to rust-lang/rust#16617.
        let match_idx = match match_idx {
            None => {
                // I believe this is impossible, because the root
                // <html> element is in special_tag.
                self.unexpected(&tag);
                return;
            }
            Some(x) => x,
        };

        self.generate_implied_end_except(tag.name.clone());

        if match_idx != self.open_elems.len() - 1 {
            // mis-nested tags
            self.unexpected(&tag);
        }
        self.open_elems.truncate(match_idx);
    }

    fn handle_misnested_a_tags(&mut self, tag: &Tag) {
        let node = unwrap_or_return!(
            self.active_formatting_end_to_marker()
                .find(|&(_, n, _)| self.html_elem_named(n, local_name!("a")))
                .map(|(_, n, _)| n.clone()),
            ()
        );

        self.unexpected(tag);
        self.adoption_agency(local_name!("a"));
        self.position_in_active_formatting(&node)
            .map(|index| self.active_formatting.remove(index));
        self.remove_from_stack(&node);
    }

    //§ tree-construction
    fn is_foreign(&self, token: &Token) -> bool {
        if let EOFToken = *token {
            return false;
        }

        if self.open_elems.is_empty() {
            return false;
        }

        let name = self.sink.elem_name(self.adjusted_current_node());
        if let ns!(html) = *name.ns {
            return false;
        }

        if mathml_text_integration_point(name) {
            match *token {
                CharacterTokens(..) | NullCharacterToken => return false,
                TagToken(Tag {
                    kind: StartTag,
                    ref name,
                    ..
                }) if !matches!(*name, local_name!("mglyph") | local_name!("malignmark")) => {
                    return false;
                }
                _ => (),
            }
        }

        if svg_html_integration_point(name) {
            match *token {
                CharacterTokens(..) | NullCharacterToken => return false,
                TagToken(Tag { kind: StartTag, .. }) => return false,
                _ => (),
            }
        }

        if let expanded_name!(mathml "annotation-xml") = name {
            match *token {
                TagToken(Tag {
                    kind: StartTag,
                    name: local_name!("svg"),
                    ..
                }) => return false,
                CharacterTokens(..) | NullCharacterToken | TagToken(Tag { kind: StartTag, .. }) => {
                    return !self
                        .sink
                        .is_mathml_annotation_xml_integration_point(self.adjusted_current_node());
                }
                _ => {}
            };
        }

        true
    }
    //§ END

    fn enter_foreign(&mut self, mut tag: Tag, ns: Namespace) -> ProcessResult<Handle> {
        match ns {
            ns!(mathml) => self.adjust_mathml_attributes(&mut tag),
            ns!(svg) => self.adjust_svg_attributes(&mut tag),
            _ => (),
        }
        self.adjust_foreign_attributes(&mut tag);

        if tag.self_closing {
            self.insert_element(NoPush, ns, tag.name, tag.attrs);
            DoneAckSelfClosing
        } else {
            self.insert_element(Push, ns, tag.name, tag.attrs);
            Done
        }
    }

    fn adjust_svg_tag_name(&mut self, tag: &mut Tag) {
        let Tag { ref mut name, .. } = *tag;
        match *name {
            local_name!("altglyph") => *name = local_name!("altGlyph"),
            local_name!("altglyphdef") => *name = local_name!("altGlyphDef"),
            local_name!("altglyphitem") => *name = local_name!("altGlyphItem"),
            local_name!("animatecolor") => *name = local_name!("animateColor"),
            local_name!("animatemotion") => *name = local_name!("animateMotion"),
            local_name!("animatetransform") => *name = local_name!("animateTransform"),
            local_name!("clippath") => *name = local_name!("clipPath"),
            local_name!("feblend") => *name = local_name!("feBlend"),
            local_name!("fecolormatrix") => *name = local_name!("feColorMatrix"),
            local_name!("fecomponenttransfer") => *name = local_name!("feComponentTransfer"),
            local_name!("fecomposite") => *name = local_name!("feComposite"),
            local_name!("feconvolvematrix") => *name = local_name!("feConvolveMatrix"),
            local_name!("fediffuselighting") => *name = local_name!("feDiffuseLighting"),
            local_name!("fedisplacementmap") => *name = local_name!("feDisplacementMap"),
            local_name!("fedistantlight") => *name = local_name!("feDistantLight"),
            local_name!("fedropshadow") => *name = local_name!("feDropShadow"),
            local_name!("feflood") => *name = local_name!("feFlood"),
            local_name!("fefunca") => *name = local_name!("feFuncA"),
            local_name!("fefuncb") => *name = local_name!("feFuncB"),
            local_name!("fefuncg") => *name = local_name!("feFuncG"),
            local_name!("fefuncr") => *name = local_name!("feFuncR"),
            local_name!("fegaussianblur") => *name = local_name!("feGaussianBlur"),
            local_name!("feimage") => *name = local_name!("feImage"),
            local_name!("femerge") => *name = local_name!("feMerge"),
            local_name!("femergenode") => *name = local_name!("feMergeNode"),
            local_name!("femorphology") => *name = local_name!("feMorphology"),
            local_name!("feoffset") => *name = local_name!("feOffset"),
            local_name!("fepointlight") => *name = local_name!("fePointLight"),
            local_name!("fespecularlighting") => *name = local_name!("feSpecularLighting"),
            local_name!("fespotlight") => *name = local_name!("feSpotLight"),
            local_name!("fetile") => *name = local_name!("feTile"),
            local_name!("feturbulence") => *name = local_name!("feTurbulence"),
            local_name!("foreignobject") => *name = local_name!("foreignObject"),
            local_name!("glyphref") => *name = local_name!("glyphRef"),
            local_name!("lineargradient") => *name = local_name!("linearGradient"),
            local_name!("radialgradient") => *name = local_name!("radialGradient"),
            local_name!("textpath") => *name = local_name!("textPath"),
            _ => (),
        }
    }

    fn adjust_attributes<F>(&mut self, tag: &mut Tag, mut map: F)
    where
        F: FnMut(LocalName) -> Option<QualName>,
    {
        for &mut Attribute { ref mut name, .. } in &mut tag.attrs {
            if let Some(replacement) = map(name.local.clone()) {
                *name = replacement;
            }
        }
    }

    fn adjust_svg_attributes(&mut self, tag: &mut Tag) {
        self.adjust_attributes(tag, |k| match k {
            local_name!("attributename") => Some(qualname!("", "attributeName")),
            local_name!("attributetype") => Some(qualname!("", "attributeType")),
            local_name!("basefrequency") => Some(qualname!("", "baseFrequency")),
            local_name!("baseprofile") => Some(qualname!("", "baseProfile")),
            local_name!("calcmode") => Some(qualname!("", "calcMode")),
            local_name!("clippathunits") => Some(qualname!("", "clipPathUnits")),
            local_name!("diffuseconstant") => Some(qualname!("", "diffuseConstant")),
            local_name!("edgemode") => Some(qualname!("", "edgeMode")),
            local_name!("filterunits") => Some(qualname!("", "filterUnits")),
            local_name!("glyphref") => Some(qualname!("", "glyphRef")),
            local_name!("gradienttransform") => Some(qualname!("", "gradientTransform")),
            local_name!("gradientunits") => Some(qualname!("", "gradientUnits")),
            local_name!("kernelmatrix") => Some(qualname!("", "kernelMatrix")),
            local_name!("kernelunitlength") => Some(qualname!("", "kernelUnitLength")),
            local_name!("keypoints") => Some(qualname!("", "keyPoints")),
            local_name!("keysplines") => Some(qualname!("", "keySplines")),
            local_name!("keytimes") => Some(qualname!("", "keyTimes")),
            local_name!("lengthadjust") => Some(qualname!("", "lengthAdjust")),
            local_name!("limitingconeangle") => Some(qualname!("", "limitingConeAngle")),
            local_name!("markerheight") => Some(qualname!("", "markerHeight")),
            local_name!("markerunits") => Some(qualname!("", "markerUnits")),
            local_name!("markerwidth") => Some(qualname!("", "markerWidth")),
            local_name!("maskcontentunits") => Some(qualname!("", "maskContentUnits")),
            local_name!("maskunits") => Some(qualname!("", "maskUnits")),
            local_name!("numoctaves") => Some(qualname!("", "numOctaves")),
            local_name!("pathlength") => Some(qualname!("", "pathLength")),
            local_name!("patterncontentunits") => Some(qualname!("", "patternContentUnits")),
            local_name!("patterntransform") => Some(qualname!("", "patternTransform")),
            local_name!("patternunits") => Some(qualname!("", "patternUnits")),
            local_name!("pointsatx") => Some(qualname!("", "pointsAtX")),
            local_name!("pointsaty") => Some(qualname!("", "pointsAtY")),
            local_name!("pointsatz") => Some(qualname!("", "pointsAtZ")),
            local_name!("preservealpha") => Some(qualname!("", "preserveAlpha")),
            local_name!("preserveaspectratio") => Some(qualname!("", "preserveAspectRatio")),
            local_name!("primitiveunits") => Some(qualname!("", "primitiveUnits")),
            local_name!("refx") => Some(qualname!("", "refX")),
            local_name!("refy") => Some(qualname!("", "refY")),
            local_name!("repeatcount") => Some(qualname!("", "repeatCount")),
            local_name!("repeatdur") => Some(qualname!("", "repeatDur")),
            local_name!("requiredextensions") => Some(qualname!("", "requiredExtensions")),
            local_name!("requiredfeatures") => Some(qualname!("", "requiredFeatures")),
            local_name!("specularconstant") => Some(qualname!("", "specularConstant")),
            local_name!("specularexponent") => Some(qualname!("", "specularExponent")),
            local_name!("spreadmethod") => Some(qualname!("", "spreadMethod")),
            local_name!("startoffset") => Some(qualname!("", "startOffset")),
            local_name!("stddeviation") => Some(qualname!("", "stdDeviation")),
            local_name!("stitchtiles") => Some(qualname!("", "stitchTiles")),
            local_name!("surfacescale") => Some(qualname!("", "surfaceScale")),
            local_name!("systemlanguage") => Some(qualname!("", "systemLanguage")),
            local_name!("tablevalues") => Some(qualname!("", "tableValues")),
            local_name!("targetx") => Some(qualname!("", "targetX")),
            local_name!("targety") => Some(qualname!("", "targetY")),
            local_name!("textlength") => Some(qualname!("", "textLength")),
            local_name!("viewbox") => Some(qualname!("", "viewBox")),
            local_name!("viewtarget") => Some(qualname!("", "viewTarget")),
            local_name!("xchannelselector") => Some(qualname!("", "xChannelSelector")),
            local_name!("ychannelselector") => Some(qualname!("", "yChannelSelector")),
            local_name!("zoomandpan") => Some(qualname!("", "zoomAndPan")),
            _ => None,
        });
    }

    fn adjust_mathml_attributes(&mut self, tag: &mut Tag) {
        self.adjust_attributes(tag, |k| match k {
            local_name!("definitionurl") => Some(qualname!("", "definitionURL")),
            _ => None,
        });
    }

    fn adjust_foreign_attributes(&mut self, tag: &mut Tag) {
        self.adjust_attributes(tag, |k| match k {
            local_name!("xlink:actuate") => Some(qualname!("xlink" xlink "actuate")),
            local_name!("xlink:arcrole") => Some(qualname!("xlink" xlink "arcrole")),
            local_name!("xlink:href") => Some(qualname!("xlink" xlink "href")),
            local_name!("xlink:role") => Some(qualname!("xlink" xlink "role")),
            local_name!("xlink:show") => Some(qualname!("xlink" xlink "show")),
            local_name!("xlink:title") => Some(qualname!("xlink" xlink "title")),
            local_name!("xlink:type") => Some(qualname!("xlink" xlink "type")),
            local_name!("xml:base") => Some(qualname!("xml" xml "base")),
            local_name!("xml:lang") => Some(qualname!("xml" xml "lang")),
            local_name!("xml:space") => Some(qualname!("xml" xml "space")),
            local_name!("xmlns") => Some(qualname!("" xmlns "xmlns")),
            local_name!("xmlns:xlink") => Some(qualname!("xmlns" xmlns "xlink")),
            _ => None,
        });
    }

    fn foreign_start_tag(&mut self, mut tag: Tag) -> ProcessResult<Handle> {
        let current_ns = self.sink.elem_name(self.adjusted_current_node()).ns.clone();
        match current_ns {
            ns!(mathml) => self.adjust_mathml_attributes(&mut tag),
            ns!(svg) => {
                self.adjust_svg_tag_name(&mut tag);
                self.adjust_svg_attributes(&mut tag);
            }
            _ => (),
        }
        self.adjust_foreign_attributes(&mut tag);
        if tag.self_closing {
            // FIXME(#118): <script /> in SVG
            self.insert_element(NoPush, current_ns, tag.name, tag.attrs);
            DoneAckSelfClosing
        } else {
            self.insert_element(Push, current_ns, tag.name, tag.attrs);
            Done
        }
    }

    fn unexpected_start_tag_in_foreign_content(&mut self, tag: Tag) -> ProcessResult<Handle> {
        self.unexpected(&tag);
        if self.is_fragment() {
            self.foreign_start_tag(tag)
        } else {
            self.pop();
            while !self.current_node_in(|n| {
                *n.ns == ns!(html)
                    || mathml_text_integration_point(n)
                    || svg_html_integration_point(n)
            }) {
                self.pop();
            }
            ReprocessForeign(TagToken(tag))
        }
    }
}
