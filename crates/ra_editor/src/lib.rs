extern crate itertools;
extern crate join_to_string;
extern crate ra_syntax;
extern crate rustc_hash;
extern crate superslice;
#[cfg(test)]
#[macro_use]
extern crate test_utils as _test_utils;

mod code_actions;
mod completion;
mod edit;
mod extend_selection;
mod folding_ranges;
mod line_index;
mod scope;
mod symbols;
#[cfg(test)]
mod test_utils;
mod typing;

pub use self::{
    code_actions::{add_derive, add_impl, flip_comma, introduce_variable, LocalEdit},
    completion::{scope_completion, complete_module_items, CompletionItem},
    edit::{Edit, EditBuilder},
    extend_selection::extend_selection,
    folding_ranges::{folding_ranges, Fold, FoldKind},
    line_index::{LineCol, LineIndex},
    symbols::{file_structure, file_symbols, FileSymbol, StructureNode},
    typing::{join_lines, on_enter, on_eq_typed},
};
pub use ra_syntax::AtomEdit;
use ra_syntax::{
    algo::find_leaf_at_offset,
    ast::{self, AstNode, NameOwner},
    File, SmolStr,
    SyntaxKind::{self, *},
    SyntaxNodeRef, TextRange, TextUnit,
};

#[derive(Debug)]
pub struct HighlightedRange {
    pub range: TextRange,
    pub tag: &'static str,
}

#[derive(Debug)]
pub struct Diagnostic {
    pub range: TextRange,
    pub msg: String,
}

#[derive(Debug)]
pub struct Runnable {
    pub range: TextRange,
    pub kind: RunnableKind,
}

#[derive(Debug)]
pub enum RunnableKind {
    Test { name: String },
    Bin,
}

pub fn matching_brace(file: &File, offset: TextUnit) -> Option<TextUnit> {
    const BRACES: &[SyntaxKind] = &[
        L_CURLY, R_CURLY, L_BRACK, R_BRACK, L_PAREN, R_PAREN, L_ANGLE, R_ANGLE,
    ];
    let (brace_node, brace_idx) = find_leaf_at_offset(file.syntax(), offset)
        .filter_map(|node| {
            let idx = BRACES.iter().position(|&brace| brace == node.kind())?;
            Some((node, idx))
        })
        .next()?;
    let parent = brace_node.parent()?;
    let matching_kind = BRACES[brace_idx ^ 1];
    let matching_node = parent
        .children()
        .find(|node| node.kind() == matching_kind)?;
    Some(matching_node.range().start())
}

pub fn highlight(file: &File) -> Vec<HighlightedRange> {
    let mut res = Vec::new();
    for node in file.syntax().descendants() {
        let tag = match node.kind() {
            COMMENT | DOC_COMMENT => "comment",
            STRING | RAW_STRING | RAW_BYTE_STRING | BYTE_STRING => "string",
            ATTR => "attribute",
            NAME_REF => "text",
            NAME => "function",
            INT_NUMBER | FLOAT_NUMBER | CHAR | BYTE => "literal",
            LIFETIME => "parameter",
            k if k.is_keyword() => "keyword",
            _ => continue,
        };
        res.push(HighlightedRange {
            range: node.range(),
            tag,
        })
    }
    res
}

pub fn diagnostics(file: &File) -> Vec<Diagnostic> {
    file.errors()
        .into_iter()
        .map(|err| Diagnostic {
            range: TextRange::offset_len(err.offset, 1.into()),
            msg: "Syntax Error: ".to_string() + &err.msg,
        })
        .collect()
}

pub fn syntax_tree(file: &File) -> String {
    ::ra_syntax::utils::dump_tree(file.syntax())
}

pub fn runnables(file: &File) -> Vec<Runnable> {
    file.syntax()
        .descendants()
        .filter_map(ast::FnDef::cast)
        .filter_map(|f| {
            let name = f.name()?.text();
            let kind = if name == "main" {
                RunnableKind::Bin
            } else if f.has_atom_attr("test") {
                RunnableKind::Test {
                    name: name.to_string(),
                }
            } else {
                return None;
            };
            Some(Runnable {
                range: f.syntax().range(),
                kind,
            })
        })
        .collect()
}

pub fn find_node_at_offset<'a, N: AstNode<'a>>(
    syntax: SyntaxNodeRef<'a>,
    offset: TextUnit,
) -> Option<N> {
    let leaves = find_leaf_at_offset(syntax, offset);
    let leaf = leaves
        .clone()
        .find(|leaf| !leaf.kind().is_trivia())
        .or_else(|| leaves.right_biased())?;
    leaf.ancestors().filter_map(N::cast).next()
}

pub fn resolve_local_name(
    name_ref: ast::NameRef,
) -> Option<(SmolStr, TextRange)> {
    let fn_def = name_ref.syntax().ancestors().find_map(ast::FnDef::cast)?;
    let scopes = scope::FnScopes::new(fn_def);
    let scope_entry = scope::resolve_local_name(name_ref, &scopes)?;
    let name = scope_entry.ast().name()?;
    Some((scope_entry.name(), name.syntax().range()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{add_cursor, assert_eq_dbg, extract_offset};

    #[test]
    fn test_highlighting() {
        let file = File::parse(
            r#"
// comment
fn main() {}
    println!("Hello, {}!", 92);
"#,
        );
        let hls = highlight(&file);
        assert_eq_dbg(
            r#"[HighlightedRange { range: [1; 11), tag: "comment" },
                HighlightedRange { range: [12; 14), tag: "keyword" },
                HighlightedRange { range: [15; 19), tag: "function" },
                HighlightedRange { range: [29; 36), tag: "text" },
                HighlightedRange { range: [38; 50), tag: "string" },
                HighlightedRange { range: [52; 54), tag: "literal" }]"#,
            &hls,
        );
    }

    #[test]
    fn test_runnables() {
        let file = File::parse(
            r#"
fn main() {}

#[test]
fn test_foo() {}

#[test]
#[ignore]
fn test_foo() {}
"#,
        );
        let runnables = runnables(&file);
        assert_eq_dbg(
            r#"[Runnable { range: [1; 13), kind: Bin },
                Runnable { range: [15; 39), kind: Test { name: "test_foo" } },
                Runnable { range: [41; 75), kind: Test { name: "test_foo" } }]"#,
            &runnables,
        )
    }

    #[test]
    fn test_matching_brace() {
        fn do_check(before: &str, after: &str) {
            let (pos, before) = extract_offset(before);
            let file = File::parse(&before);
            let new_pos = match matching_brace(&file, pos) {
                None => pos,
                Some(pos) => pos,
            };
            let actual = add_cursor(&before, new_pos);
            assert_eq_text!(after, &actual);
        }

        do_check("struct Foo { a: i32, }<|>", "struct Foo <|>{ a: i32, }");
    }
}
