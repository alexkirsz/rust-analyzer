//! This module add real world mbe example for benchmark tests

use rustc_hash::FxHashMap;
use syntax::{
    ast::{self, NameOwner},
    AstNode, SmolStr,
};
use test_utils::{bench, bench_fixture, skip_slow_tests};

use crate::{
    ast_to_token_tree,
    parser::{Op, RepeatKind, Separator},
    MacroRules,
};

#[test]
fn benchmark_parse_macro_rules() {
    if skip_slow_tests() {
        return;
    }
    let rules = macro_rules_fixtures_tt();
    let hash: usize = {
        let _pt = bench("mbe parse macro rules");
        rules.values().map(|it| MacroRules::parse(it).unwrap().rules.len()).sum()
    };
    assert_eq!(hash, 1144);
}

#[test]
fn benchmark_expand_macro_rules() {
    if skip_slow_tests() {
        return;
    }
    let rules = macro_rules_fixtures();
    let invocations = invocation_fixtures(&rules);

    let hash: usize = {
        let _pt = bench("mbe expand macro rules");
        invocations
            .into_iter()
            .map(|(id, tt)| {
                let res = rules[&id].expand(&tt);
                if res.err.is_some() {
                    // FIXME:
                    // Currently `invocation_fixtures` will generate some correct invocations but
                    // cannot be expanded by mbe. We ignore errors here.
                    // See: https://github.com/rust-analyzer/rust-analyzer/issues/4777
                    eprintln!("err from {} {:?}", id, res.err);
                }
                res.value.token_trees.len()
            })
            .sum()
    };
    assert_eq!(hash, 66995);
}

fn macro_rules_fixtures() -> FxHashMap<String, MacroRules> {
    macro_rules_fixtures_tt()
        .into_iter()
        .map(|(id, tt)| (id, MacroRules::parse(&tt).unwrap()))
        .collect()
}

fn macro_rules_fixtures_tt() -> FxHashMap<String, tt::Subtree> {
    let fixture = bench_fixture::numerous_macro_rules();
    let source_file = ast::SourceFile::parse(&fixture).ok().unwrap();

    source_file
        .syntax()
        .descendants()
        .filter_map(ast::MacroRules::cast)
        .map(|rule| {
            let id = rule.name().unwrap().to_string();
            let (def_tt, _) = ast_to_token_tree(&rule.token_tree().unwrap()).unwrap();
            (id, def_tt)
        })
        .collect()
}

// Generate random invocation fixtures from rules
fn invocation_fixtures(rules: &FxHashMap<String, MacroRules>) -> Vec<(String, tt::Subtree)> {
    let mut seed = 123456789;
    let mut res = Vec::new();

    for (name, it) in rules {
        for rule in &it.rules {
            // Generate twice
            for _ in 0..2 {
                let mut subtree = tt::Subtree::default();
                for op in rule.lhs.iter() {
                    collect_from_op(op, &mut subtree, &mut seed);
                }
                res.push((name.clone(), subtree));
            }
        }
    }
    return res;

    fn collect_from_op(op: &Op, parent: &mut tt::Subtree, seed: &mut usize) {
        return match op {
            Op::Var { kind, .. } => match kind.as_ref().map(|it| it.as_str()) {
                Some("ident") => parent.token_trees.push(make_ident("foo")),
                Some("ty") => parent.token_trees.push(make_ident("Foo")),
                Some("tt") => parent.token_trees.push(make_ident("foo")),
                Some("vis") => parent.token_trees.push(make_ident("pub")),
                Some("pat") => parent.token_trees.push(make_ident("foo")),
                Some("path") => parent.token_trees.push(make_ident("foo")),
                Some("literal") => parent.token_trees.push(make_literal("1")),
                Some("expr") => parent.token_trees.push(make_ident("foo").into()),
                Some("lifetime") => {
                    parent.token_trees.push(make_punct('\''));
                    parent.token_trees.push(make_ident("a"));
                }
                Some("block") => {
                    parent.token_trees.push(make_subtree(tt::DelimiterKind::Brace, None))
                }
                Some("item") => {
                    parent.token_trees.push(make_ident("fn"));
                    parent.token_trees.push(make_ident("foo"));
                    parent.token_trees.push(make_subtree(tt::DelimiterKind::Parenthesis, None));
                    parent.token_trees.push(make_subtree(tt::DelimiterKind::Brace, None));
                }
                Some("meta") => {
                    parent.token_trees.push(make_ident("foo"));
                    parent.token_trees.push(make_subtree(tt::DelimiterKind::Parenthesis, None));
                }

                None => (),
                Some(kind) => panic!("Unhandled kind {}", kind),
            },
            Op::Leaf(leaf) => parent.token_trees.push(leaf.clone().into()),
            Op::Repeat { tokens, kind, separator } => {
                let max = 10;
                let cnt = match kind {
                    RepeatKind::ZeroOrMore => rand(seed) % max,
                    RepeatKind::OneOrMore => 1 + rand(seed) % max,
                    RepeatKind::ZeroOrOne => rand(seed) % 2,
                };
                for i in 0..cnt {
                    for it in tokens.iter() {
                        collect_from_op(it, parent, seed);
                    }
                    if i + 1 != cnt {
                        if let Some(sep) = separator {
                            match sep {
                                Separator::Literal(it) => parent
                                    .token_trees
                                    .push(tt::Leaf::Literal(it.clone().into()).into()),
                                Separator::Ident(it) => parent
                                    .token_trees
                                    .push(tt::Leaf::Ident(it.clone().into()).into()),
                                Separator::Puncts(puncts) => {
                                    for it in puncts {
                                        parent
                                            .token_trees
                                            .push(tt::Leaf::Punct(it.clone().into()).into())
                                    }
                                }
                            };
                        }
                    }
                }
            }
            Op::Subtree { tokens, delimiter } => {
                let mut subtree =
                    tt::Subtree { delimiter: delimiter.clone(), token_trees: Vec::new() };
                tokens.iter().for_each(|it| {
                    collect_from_op(it, &mut subtree, seed);
                });
                parent.token_trees.push(subtree.into());
            }
        };

        // Simple linear congruential generator for determistic result
        fn rand(seed: &mut usize) -> usize {
            let a = 1664525;
            let c = 1013904223;
            *seed = usize::wrapping_add(usize::wrapping_mul(*seed, a), c);
            return *seed;
        }
        fn make_ident(ident: &str) -> tt::TokenTree {
            tt::Leaf::Ident(tt::Ident { id: tt::TokenId::unspecified(), text: SmolStr::new(ident) })
                .into()
        }
        fn make_punct(char: char) -> tt::TokenTree {
            tt::Leaf::Punct(tt::Punct {
                id: tt::TokenId::unspecified(),
                char,
                spacing: tt::Spacing::Alone,
            })
            .into()
        }
        fn make_literal(lit: &str) -> tt::TokenTree {
            tt::Leaf::Literal(tt::Literal {
                id: tt::TokenId::unspecified(),
                text: SmolStr::new(lit),
            })
            .into()
        }
        fn make_subtree(
            kind: tt::DelimiterKind,
            token_trees: Option<Vec<tt::TokenTree>>,
        ) -> tt::TokenTree {
            tt::Subtree {
                delimiter: Some(tt::Delimiter { id: tt::TokenId::unspecified(), kind }),
                token_trees: token_trees.unwrap_or_default(),
            }
            .into()
        }
    }
}
