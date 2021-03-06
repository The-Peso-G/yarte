// Copyright 2014-2017 The html5ever Project Developers. See the
// COPYRIGHT file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Types used within the tree builder code.  Not exported to users.

use html5ever::tokenizer::{states::RawKind, Tag};

use html5ever::tendril::StrTendril;

pub use self::{FormatEntry::*, InsertionMode::*, ProcessResult::*, SplitStatus::*, Token::*};

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum InsertionMode {
    Initial,
    BeforeHtml,
    AfterBody,
    AfterAfterBody,
    InHtml,
    RawText,
}

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum SplitStatus {
    NotSplit,
    Whitespace,
    NotWhitespace,
}

/// A subset/refinement of `tokenizer::Token`.  Everything else is handled
/// specially at the beginning of `process_token`.
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum Token {
    TagToken(Tag),
    CommentToken(StrTendril),
    CharacterTokens(SplitStatus, StrTendril),
    NullCharacterToken,
    EOFToken,
}

pub enum ProcessResult<Handle> {
    Done,
    DoneAckSelfClosing,
    SplitWhitespace(StrTendril),
    Reprocess(InsertionMode, Token),
    ReprocessForeign(Token),
    Script(Handle),
    ToPlaintext,
    ToRawData(RawKind),
}

pub enum FormatEntry<Handle> {
    Element(Handle, Tag),
}
