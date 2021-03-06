use super::{parse as _parse, Helper, Node::*, Partial, *};
use syn::{parse_str, Expr, Stmt};

const WS: Ws = (false, false);

fn parse(rest: &str) -> Vec<SNode> {
    _parse(Cursor { rest, off: 0 })
}

#[test]
fn test_empty() {
    let src = r#""#;
    assert_eq!(parse(src), vec![]);
}

#[test]
fn test_fallback() {
    let src = r#"{{"#;
    let span = Span { lo: 0, hi: 2 };
    assert_eq!(parse(src), vec![S(Lit("", S("{{", span), ""), span)]);
    let src = r#"{{{"#;
    let span = Span { lo: 0, hi: 3 };
    assert_eq!(parse(src), vec![S(Lit("", S("{{{", span), ""), span)]);
    let src = r#"{{#"#;
    assert_eq!(parse(src), vec![S(Lit("", S("{{#", span), ""), span)]);
    let src = r#"{{>"#;
    assert_eq!(parse(src), vec![S(Lit("", S("{{>", span), ""), span)]);
    let src = r#"{"#;
    let span = Span { lo: 0, hi: 1 };
    assert_eq!(parse(src), vec![S(Lit("", S("{", span), ""), span)]);
}

#[test]
fn test_1() {
    let src = r#"{{# unless flag}}{{{/ unless}}"#;
    let span = Span { lo: 17, hi: 18 };
    let expr: syn::Expr = parse_str("flag").unwrap();
    assert_eq!(
        parse(src),
        vec![S(
            Node::Helper(Box::new(Helper::Unless(
                ((false, false), (false, false)),
                S(Box::new(expr), Span { lo: 11, hi: 15 }),
                vec![S(Lit("", S("{", span), ""), span)]
            ))),
            Span { lo: 0, hi: 30 }
        )]
    );
}

#[test]
fn test_eat_comment() {
    let src = r#"{{! Commentary !}}"#;
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(parse(src), vec![S(Comment(" Commentary "), span)]);
    let src = r#"{{!-- Commentary --!}}"#;
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(parse(src), vec![S(Comment(" Commentary "), span)]);
    let src = r#"foo {{!-- Commentary --!}}"#;
    let span = Span {
        lo: 4,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![
            S(
                Lit("", S("foo", Span { lo: 0, hi: 3 }), " "),
                Span { lo: 0, hi: 4 },
            ),
            S(Comment(" Commentary "), span)
        ]
    );
}

#[test]
fn test_eat_expr() {
    let src = r#"{{ var }}"#;
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Expr(
                WS,
                S(
                    Box::new(parse_str::<Expr>("var").unwrap()),
                    Span { lo: 3, hi: 6 },
                ),
            ),
            span,
        )]
    );

    let src = r#"{{ fun() }}"#;
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Expr(
                WS,
                S(
                    Box::new(parse_str::<Expr>("fun()").unwrap()),
                    Span { lo: 3, hi: 8 },
                ),
            ),
            span,
        )]
    );

    let src = r#"{{ fun(|a| a) }}"#;
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Expr(
                WS,
                S(
                    Box::new(parse_str::<Expr>("fun(|a| a)").unwrap()),
                    Span { lo: 3, hi: 13 },
                ),
            ),
            span,
        )]
    );

    let src = r#"{{
            fun(|a| {
                { a }
            })
        }}"#;
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Expr(
                WS,
                S(
                    Box::new(parse_str::<Expr>("fun(|a| {{a}})").unwrap()),
                    Span { lo: 15, hi: 61 },
                ),
            ),
            span,
        )]
    );
}

#[should_panic]
#[test]
fn test_eat_expr_panic_a() {
    let src = r#"{{ fn(|a| {{a}}) }}"#;
    parse(src);
}

#[should_panic]
#[test]
fn test_eat_expr_panic_b() {
    let src = r#"{{ let a = mut a  }}"#;
    parse(src);
}

#[test]
fn test_eat_safe() {
    let src = r#"{{{ var }}}"#;
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Safe(
                WS,
                S(
                    Box::new(parse_str::<Expr>("var").unwrap()),
                    Span { lo: 4, hi: 7 },
                ),
            ),
            span,
        )]
    );

    let src = r#"{{{ fun() }}}"#;
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Safe(
                WS,
                S(
                    Box::new(parse_str::<Expr>("fun()").unwrap()),
                    Span { lo: 4, hi: 9 },
                ),
            ),
            span,
        )]
    );

    let src = r#"{{{ fun(|a| a) }}}"#;
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Safe(
                WS,
                S(
                    Box::new(parse_str::<Expr>("fun(|a| a)").unwrap()),
                    Span { lo: 4, hi: 14 },
                ),
            ),
            span,
        )]
    );

    let src = r#"{{{
            fun(|a| {
                {{ a }}
            })
        }}}"#;
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Safe(
                WS,
                S(
                    Box::new(parse_str::<Expr>("fun(|a| {{{a}}})").unwrap()),
                    Span { lo: 16, hi: 64 },
                ),
            ),
            span,
        )]
    );
}

#[should_panic]
#[test]
fn test_eat_safe_panic() {
    let src = r#"{{ fn(|a| {{{a}}}) }}"#;
    parse(src);
}

#[test]
fn test_trim() {
    assert_eq!(trim(" a "), (" ", "a", " "));
    assert_eq!(trim(" a"), (" ", "a", ""));
    assert_eq!(trim("a"), ("", "a", ""));
    assert_eq!(trim(""), ("", "", ""));
    assert_eq!(trim("a "), ("", "a", " "));
    assert_eq!(trim("a a"), ("", "a a", ""));
    assert_eq!(trim("a a "), ("", "a a", " "));
    assert_eq!(trim(" \n\t\ra a "), (" \n\t\r", "a a", " "));
    assert_eq!(trim(" \n\t\r "), (" \n\t\r ", "", ""));
}

#[test]
fn test_eat_if() {
    let rest = r#"foo{{ else }}"#;
    let result = " else }}";
    assert_eq!(
        eat_if(Cursor { rest, off: 0 }).unwrap(),
        (
            Cursor {
                rest: result,
                off: (rest.len() - result.len()) as u32,
            },
            vec![S(
                Lit("", S("foo", Span { lo: 0, hi: 3 }), ""),
                Span { lo: 0, hi: 3 },
            )]
        )
    );
    let rest = r#"{{foo}}{{else}}"#;
    let result = "else}}";
    assert_eq!(
        eat_if(Cursor { rest, off: 0 }).unwrap(),
        (
            Cursor {
                rest: result,
                off: (rest.len() - result.len()) as u32,
            },
            vec![S(
                Expr(
                    WS,
                    S(
                        Box::new(parse_str::<Expr>("foo").unwrap()),
                        Span { lo: 2, hi: 5 },
                    ),
                ),
                Span { lo: 0, hi: 7 },
            )]
        )
    );
    let rest = r#"{{ let a = foo }}{{else if cond}}{{else}}"#;
    let local = if let Stmt::Local(local) = parse_str::<Stmt>("let a = foo;").unwrap() {
        local
    } else {
        unreachable!();
    };
    let result = "else if cond}}{{else}}";
    assert_eq!(
        eat_if(Cursor { rest, off: 0 }).unwrap(),
        (
            Cursor {
                rest: result,
                off: (rest.len() - result.len()) as u32,
            },
            vec![S(
                Local(S(Box::new(local), Span { lo: 3, hi: 14 })),
                Span { lo: 0, hi: 17 },
            )]
        )
    );
}

#[test]
fn test_helpers() {
    let rest = "each name }}{{first}} {{last}}{{/each}}";
    assert_eq!(
        hel(Cursor { rest, off: 0 }, false).unwrap(),
        (
            Cursor {
                rest: "",
                off: rest.len() as u32,
            },
            Helper(Box::new(Helper::Each(
                (WS, WS),
                S(
                    Box::new(parse_str::<Expr>("name").unwrap()),
                    Span { lo: 5, hi: 9 },
                ),
                vec![
                    S(
                        Expr(
                            WS,
                            S(
                                Box::new(parse_str::<Expr>("first").unwrap()),
                                Span { lo: 14, hi: 19 },
                            ),
                        ),
                        Span { lo: 12, hi: 21 },
                    ),
                    S(
                        Lit(" ", S("", Span { lo: 22, hi: 22 }), ""),
                        Span { lo: 21, hi: 22 },
                    ),
                    S(
                        Expr(
                            WS,
                            S(
                                Box::new(parse_str::<Expr>("last").unwrap()),
                                Span { lo: 24, hi: 28 },
                            ),
                        ),
                        Span { lo: 22, hi: 30 },
                    ),
                ],
            )))
        )
    );
}

#[test]
fn test_if_else() {
    let rest = "foo{{/if}}";
    let args = S(
        Box::new(parse_str::<Expr>("bar").unwrap()),
        Span { lo: 0, hi: 0 },
    );

    assert_eq!(
        if_else(WS, Cursor { rest, off: 0 }, args.clone()).unwrap(),
        (
            Cursor {
                rest: "",
                off: rest.len() as u32,
            },
            Helper(Box::new(Helper::If(
                (
                    (WS, WS),
                    args,
                    vec![S(
                        Lit("", S("foo", Span { lo: 0, hi: 3 }), ""),
                        Span { lo: 0, hi: 3 },
                    )]
                ),
                vec![],
                None,
            )))
        )
    );

    let rest = "foo{{else}}bar{{/if}}";
    let args = S(
        Box::new(parse_str::<Expr>("bar").unwrap()),
        Span { lo: 0, hi: 0 },
    );

    assert_eq!(
        if_else(WS, Cursor { rest, off: 0 }, args.clone()).unwrap(),
        (
            Cursor {
                rest: "",
                off: rest.len() as u32,
            },
            Helper(Box::new(Helper::If(
                (
                    (WS, WS),
                    args,
                    vec![S(
                        Lit("", S("foo", Span { lo: 0, hi: 3 }), ""),
                        Span { lo: 0, hi: 3 },
                    )]
                ),
                vec![],
                Some((
                    WS,
                    vec![S(
                        Lit("", S("bar", Span { lo: 11, hi: 14 }), ""),
                        Span { lo: 11, hi: 14 },
                    )]
                )),
            )))
        )
    );
}

#[test]
fn test_else_if() {
    let rest = "foo{{else if cond }}bar{{else}}foO{{/if}}";
    let args = S(
        Box::new(parse_str::<Expr>("bar").unwrap()),
        Span { lo: 0, hi: 0 },
    );

    assert_eq!(
        if_else(WS, Cursor { rest, off: 0 }, args.clone()).unwrap(),
        (
            Cursor {
                rest: "",
                off: rest.len() as u32,
            },
            Helper(Box::new(Helper::If(
                (
                    (WS, WS),
                    args,
                    vec![S(
                        Lit("", S("foo", Span { lo: 0, hi: 3 }), ""),
                        Span { lo: 0, hi: 3 },
                    )]
                ),
                vec![(
                    WS,
                    S(
                        Box::new(parse_str::<Expr>("cond").unwrap()),
                        Span { lo: 13, hi: 17 },
                    ),
                    vec![S(
                        Lit("", S("bar", Span { lo: 20, hi: 23 }), ""),
                        Span { lo: 20, hi: 23 },
                    )]
                )],
                Some((
                    WS,
                    vec![S(
                        Lit("", S("foO", Span { lo: 31, hi: 34 }), ""),
                        Span { lo: 31, hi: 34 },
                    )]
                )),
            )))
        )
    );
}

#[test]
fn test_defined() {
    let src = "{{#foo bar}}hello{{/foo}}";
    assert_eq!(&src[12..17], "hello");
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Helper(Box::new(Helper::Defined(
                (WS, WS),
                "foo",
                S(
                    Box::new(parse_str::<Expr>("bar").unwrap()),
                    Span { lo: 7, hi: 10 },
                ),
                vec![S(
                    Lit("", S("hello", Span { lo: 12, hi: 17 }), ""),
                    Span { lo: 12, hi: 17 },
                )],
            ))),
            span,
        )]
    );
}

#[test]
fn test_ws_expr() {
    let src = "{{~foo~}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Expr(
                (true, true),
                S(
                    Box::new(parse_str::<Expr>("foo").unwrap()),
                    Span { lo: 3, hi: 6 },
                ),
            ),
            span,
        )]
    );
    let src = "{{~ foo~}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Expr(
                (true, true),
                S(
                    Box::new(parse_str::<Expr>("foo").unwrap()),
                    Span { lo: 4, hi: 7 },
                ),
            ),
            span,
        )]
    );
    let src = "{{~ foo}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Expr(
                (true, false),
                S(
                    Box::new(parse_str::<Expr>("foo").unwrap()),
                    Span { lo: 4, hi: 7 },
                ),
            ),
            span,
        )]
    );
    let src = "{{foo    ~}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Expr(
                (false, true),
                S(
                    Box::new(parse_str::<Expr>("foo").unwrap()),
                    Span { lo: 2, hi: 5 },
                ),
            ),
            span,
        )]
    );
    let src = "{{~{foo }~}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Safe(
                (true, true),
                S(
                    Box::new(parse_str::<Expr>("foo").unwrap()),
                    Span { lo: 4, hi: 7 },
                ),
            ),
            span,
        )]
    );
    let src = "{{{foo }~}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Safe(
                (false, true),
                S(
                    Box::new(parse_str::<Expr>("foo").unwrap()),
                    Span { lo: 3, hi: 6 },
                ),
            ),
            span,
        )]
    );
}

#[test]
fn test_ws_each() {
    let src = "{{~#each bar~}}{{~/each~}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Helper(Box::new(Helper::Each(
                ((true, true), (true, true)),
                S(
                    Box::new(parse_str::<Expr>("bar").unwrap()),
                    Span { lo: 9, hi: 12 },
                ),
                vec![],
            ))),
            span,
        )]
    );
}

#[test]
fn test_ws_if() {
    let src = "{{~#if bar~}}{{~/if~}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Helper(Box::new(Helper::If(
                (
                    ((true, true), (true, true)),
                    S(
                        Box::new(parse_str::<Expr>("bar").unwrap()),
                        Span { lo: 7, hi: 10 },
                    ),
                    vec![],
                ),
                vec![],
                None,
            ))),
            span,
        )]
    );
}

#[test]
fn test_ws_if_else() {
    let src = "{{~#if bar~}}{{~else~}}{{~/if~}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Helper(Box::new(Helper::If(
                (
                    ((true, true), (true, true)),
                    S(
                        Box::new(parse_str::<Expr>("bar").unwrap()),
                        Span { lo: 7, hi: 10 },
                    ),
                    vec![],
                ),
                vec![],
                Some(((true, true), vec![])),
            ))),
            span,
        )]
    );
}

#[test]
fn test_ws_if_else_if() {
    let src = "{{~#if bar~}}{{~else if bar~}}{{~else~}}{{~/if~}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Helper(Box::new(Helper::If(
                (
                    ((true, true), (true, true)),
                    S(
                        Box::new(parse_str::<Expr>("bar").unwrap()),
                        Span { lo: 7, hi: 10 },
                    ),
                    vec![],
                ),
                vec![(
                    (true, true),
                    S(
                        Box::new(parse_str::<Expr>("bar").unwrap()),
                        Span { lo: 24, hi: 27 },
                    ),
                    vec![],
                )],
                Some(((true, true), vec![])),
            ))),
            span,
        )]
    );
}

#[test]
fn test_ws_raw() {
    let src = "{{~R~}}{{#some }}{{/some}}{{~/R ~}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Raw(
                ((true, true), (true, true)),
                "",
                S("{{#some }}{{/some}}", Span { lo: 7, hi: 26 }),
                "",
            ),
            span,
        )]
    );
    let src = "{{R  ~}}{{#some }}{{/some}}{{/R ~}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Raw(
                ((false, true), (false, true)),
                "",
                S("{{#some }}{{/some}}", Span { lo: 8, hi: 27 }),
                "",
            ),
            span,
        )]
    );
}

#[test]
fn test_partial_ws() {
    let src = "{{~> partial ~}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };

    assert_eq!(
        parse(src),
        vec![S(
            Node::Partial(Partial(
                (true, true),
                S("partial", Span { lo: 5, hi: 12 }),
                S(vec![], Span { lo: 13, hi: 13 }),
            )),
            span,
        )]
    );
    let src = "{{> partial scope }}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Node::Partial(Partial(
                WS,
                S("partial", Span { lo: 4, hi: 11 }),
                S(
                    vec![parse_str::<Expr>("scope").unwrap()],
                    Span { lo: 12, hi: 17 },
                ),
            )),
            span,
        )]
    );
    let src = "{{> partial scope ~}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Node::Partial(Partial(
                (false, true),
                S("partial", Span { lo: 4, hi: 11 }),
                S(
                    vec![parse_str::<Expr>("scope").unwrap()],
                    Span { lo: 12, hi: 17 },
                ),
            )),
            span,
        )]
    );
}

#[test]
fn test_partial() {
    let src = "{{> partial }}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Node::Partial(Partial(
                WS,
                S("partial", Span { lo: 4, hi: 11 }),
                S(vec![], Span { lo: 12, hi: 12 }),
            )),
            span,
        )]
    );
    let src = "{{> partial scope }}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Node::Partial(Partial(
                WS,
                S("partial", Span { lo: 4, hi: 11 }),
                S(
                    vec![parse_str::<Expr>("scope").unwrap()],
                    Span { lo: 12, hi: 17 },
                ),
            )),
            span,
        )]
    );
}

#[test]
fn test_raw() {
    let src = "{{R}}{{#some }}{{/some}}{{/R}}";
    let span = Span {
        lo: 0,
        hi: src.len() as u32,
    };
    assert_eq!(
        parse(src),
        vec![S(
            Raw(
                (WS, WS),
                "",
                S("{{#some }}{{/some}}", Span { lo: 5, hi: 24 }),
                "",
            ),
            span,
        )]
    );
}

#[test]
fn test_expr_list() {
    let src = "bar, foo = \"bar\"\n, fuu = 1  , goo = true,    ";
    assert_eq!(
        eat_expr_list(src).unwrap(),
        vec![
            parse_str::<Expr>("bar").unwrap(),
            parse_str::<Expr>("foo=\"bar\"").unwrap(),
            parse_str::<Expr>("fuu=1").unwrap(),
            parse_str::<Expr>("goo=true").unwrap(),
        ]
    );
}
