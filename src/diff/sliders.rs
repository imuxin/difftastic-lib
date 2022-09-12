//! Prefer contiguous novel nodes on the same line.
//!
//! A slider takes the following form:
//!
//! Old:
//!
//! ```text
//! A B
//! C D
//! ```
//!
//! New:
//!
//! ```text
//! A B
//! A B
//! C D
//! ```
//!
//! It would be correct, but ugly, to show the following diff:
//!
//! ```text
//! A +B+
//! +A+ B
//! C D
//! ```
//!
//! This module fixes these sliders by sliding novel region regions
//! forwards or backwards when the before and after nodes are the same
//! (B in this example).

use crate::{
    diff::changes::{insert_deep_novel, insert_deep_unchanged, ChangeKind::*, ChangeMap},
    parse::guess_language,
    parse::syntax::Syntax,
    positions::SingleLineSpan,
};
use Syntax::*;

pub fn fix_all_sliders<'a>(
    language: guess_language::Language,
    nodes: &[&'a Syntax<'a>],
    change_map: &mut ChangeMap<'a>,
) {
    // TODO: fix sliders that require more than two steps.
    fix_all_sliders_one_step(nodes, change_map);
    fix_all_sliders_one_step(nodes, change_map);

    fix_all_nested_sliders(language, nodes, change_map);
}

/// Should nester slider correction prefer the inner or outer
/// delimiter?
fn prefer_outer_delimiter(language: guess_language::Language) -> bool {
    use crate::parse::guess_language::Language::*;
    match language {
        // For Lisp family languages, we get the best result with the
        // outer delimiter.
        EmacsLisp | Clojure | CommonLisp | Janet => true,
        // JSON and TOML are like Lisp: the outer delimiter in an array object
        // is the most relevant.
        Json | Toml | Hcl => true,
        // It's probably the case that outer delimiters are used more
        // frequently than inner delimiters in SQl. (foo = 1 OR bar = 2)
        // is more likely than foo(1).
        Sql => true,
        // For everything else, prefer the inner delimiter. These
        // languages have syntax like `foo(bar)` or `foo[bar]` where
        // the inner delimiter is more relevant.
        Bash | C | CMake | CPlusPlus | CSharp | Css | Dart | Elixir | Elm | Elvish | Gleam | Go
        | Hack | Haskell | Html | Java | JavaScript | Jsx | Julia | Kotlin | Lua | Make | Nix
        | OCaml | OCamlInterface | Perl | Php | Python | Qml | Ruby | Rust | Scala | Swift
        | Tsx | TypeScript | Yaml | Zig => false,
    }
}

fn fix_all_sliders_one_step<'a>(nodes: &[&'a Syntax<'a>], change_map: &mut ChangeMap<'a>) {
    for node in nodes {
        if let List { children, .. } = node {
            fix_all_sliders_one_step(children, change_map);
        }
    }
    fix_sliders(nodes, change_map);
}

/// Correct sliders in middle insertions.
///
/// Consider the code:
///
/// ```text
/// // Before
/// old1(old2);
/// // After
/// old1(new1(old2));
/// ```
///
/// Tree diffing has two possible solution here. Either we've added
/// `new1(...)` or we've added `(new1...)`. Both are valid.
///
/// For C-like languages, the first case matches human intuition much
/// better. Fix the slider to make the inner delimiter novel.
fn fix_all_nested_sliders<'a>(
    language: guess_language::Language,
    nodes: &[&'a Syntax<'a>],
    change_map: &mut ChangeMap<'a>,
) {
    let prefer_outer = prefer_outer_delimiter(language);
    for node in nodes {
        if prefer_outer {
            fix_nested_slider_prefer_outer(node, change_map);
        } else {
            fix_nested_slider_prefer_inner(node, change_map);
        }
    }
}

/// When we see code of the form `(old-1 (novel (old-2)))`, prefer
/// treating the outer delimiter as novel, so `(novel ...)` in this
/// example.
fn fix_nested_slider_prefer_outer<'a>(node: &'a Syntax<'a>, change_map: &mut ChangeMap<'a>) {
    if let List { children, .. } = node {
        match change_map
            .get(node)
            .expect("Changes should be set before slider correction")
        {
            Unchanged(_) => {
                // All children should be novel except one descendant.
                let mut found_unchanged = vec![];
                unchanged_descendants_ignore_delim(children, &mut found_unchanged, change_map);

                if let [unchanged] = found_unchanged[..] {
                    if matches!(unchanged, List { .. })
                        && matches!(change_map.get(unchanged), Some(Novel))
                    {
                        push_unchanged_to_descendant(node, unchanged, change_map);
                    }
                }
            }
            ReplacedComment(_, _) => {}
            Novel => {}
        }

        for child in children {
            fix_nested_slider_prefer_outer(child, change_map);
        }
    }
}

/// When we see code of the form `old1(novel(old2()))`, prefer
/// treating the inner delimiter as novel, so `novel(...)` in this
/// example.
fn fix_nested_slider_prefer_inner<'a>(node: &'a Syntax<'a>, change_map: &mut ChangeMap<'a>) {
    if let List { children, .. } = node {
        match change_map
            .get(node)
            .expect("Changes should be set before slider correction")
        {
            Unchanged(_) => {}
            ReplacedComment(_, _) => {}
            Novel => {
                let mut found_unchanged = vec![];
                unchanged_descendants(children, &mut found_unchanged, change_map);

                if let [List { .. }] = found_unchanged[..] {
                    push_unchanged_to_ancestor(node, found_unchanged[0], change_map);
                }
            }
        }

        for child in children {
            fix_nested_slider_prefer_inner(child, change_map);
        }
    }
}

/// Find the unchanged descendants of `nodes`.
fn unchanged_descendants<'a>(
    nodes: &[&'a Syntax<'a>],
    found: &mut Vec<&'a Syntax<'a>>,
    change_map: &ChangeMap<'a>,
) {
    // We're only interested if there is exactly one unchanged
    // descendant, so return early if we find 2 or more.
    if found.len() > 1 {
        return;
    }

    for node in nodes {
        match change_map.get(node).unwrap() {
            Unchanged(_) => {
                found.push(node);
            }
            Novel | ReplacedComment(_, _) => {
                if let List { children, .. } = node {
                    unchanged_descendants(children, found, change_map);
                }
            }
        }
    }
}

/// Find the descendants of `nodes` that are unchanged, but ignore the
/// delimiter on list nodes.
fn unchanged_descendants_ignore_delim<'a>(
    nodes: &[&'a Syntax<'a>],
    found: &mut Vec<&'a Syntax<'a>>,
    change_map: &ChangeMap<'a>,
) {
    // We're only interested if there is exactly one unchanged
    // descendant, so return early if we find 2 or more.
    if found.len() > 1 {
        return;
    }

    for node in nodes {
        let is_unchanged = matches!(change_map.get(node), Some(Unchanged(_)));

        match node {
            Atom { .. } => {
                if is_unchanged {
                    found.push(node);
                } else {
                    // No problem
                }
            }
            List { children, .. } => {
                let all_children_unchanged = true;

                if is_unchanged {
                    // Outer list is unchanged, not what we wanted.
                    found.push(node);
                } else {
                    // Is changed.
                    if all_children_unchanged {
                        // What we're looking for.
                        found.push(node);
                    } else {
                        unchanged_descendants_ignore_delim(children, found, change_map);
                    }
                }
            }
        }
    }
}

/// Given a nested list where the root delimiters are unchanged but
/// the inner list's delimiters are novel, mark the inner list as
/// unchanged instead.
fn push_unchanged_to_descendant<'a>(
    root: &'a Syntax<'a>,
    inner: &'a Syntax<'a>,
    change_map: &mut ChangeMap<'a>,
) {
    let root_change = change_map
        .get(root)
        .expect("Changes should be set before slider correction");

    let delimiters_match = match (root, inner) {
        (
            List {
                open_content: root_open,
                close_content: root_close,
                ..
            },
            List {
                open_content: inner_open,
                close_content: inner_close,
                ..
            },
        ) => root_open == inner_open && root_close == inner_close,
        _ => false,
    };

    if delimiters_match {
        change_map.insert(root, Novel);
        change_map.insert(inner, root_change);
    }
}

/// Given a nested list where the root delimiters are novel but
/// the inner list's delimiters are unchanged, mark the root list as
/// unchanged instead.
fn push_unchanged_to_ancestor<'a>(
    root: &'a Syntax<'a>,
    inner: &'a Syntax<'a>,
    change_map: &mut ChangeMap<'a>,
) {
    let inner_change = change_map.get(inner).expect("Node changes should be set");

    let delimiters_match = match (root, inner) {
        (
            List {
                open_content: root_open,
                close_content: root_close,
                ..
            },
            List {
                open_content: inner_open,
                close_content: inner_close,
                ..
            },
        ) => root_open == inner_open && root_close == inner_close,
        _ => false,
    };

    if delimiters_match {
        change_map.insert(root, inner_change);
        change_map.insert(inner, Novel);
    }
}

fn fix_sliders<'a>(nodes: &[&'a Syntax<'a>], change_map: &mut ChangeMap<'a>) {
    for (region_start, region_end) in novel_regions_after_unchanged(nodes, change_map) {
        slide_to_prev_node(nodes, change_map, region_start, region_end);
    }
    for (region_start, region_end) in novel_regions_before_unchanged(nodes, change_map) {
        slide_to_next_node(nodes, change_map, region_start, region_end);
    }
}

fn novel_regions_after_unchanged<'a>(
    nodes: &[&'a Syntax<'a>],
    change_map: &ChangeMap<'a>,
) -> Vec<(usize, usize)> {
    let mut regions: Vec<Vec<usize>> = vec![];
    let mut region: Option<Vec<usize>> = None;

    for (i, node) in nodes.iter().enumerate() {
        let change = change_map.get(node).expect("Node changes should be set");

        match change {
            Unchanged(_) => {
                // Could have just finished a novel region.
                if let Some(region) = region {
                    regions.push(region);
                }

                // Could be the unchanged node before a novel region.
                region = Some(vec![]);
            }
            Novel => {
                if let Some(mut r) = region {
                    r.push(i);
                    region = Some(r);
                }
            }
            ReplacedComment(_, _) => {
                // Could have just finished a novel region.
                if let Some(region) = region {
                    regions.push(region);
                }

                region = None;
            }
        }
    }

    if let Some(region) = region {
        regions.push(region);
    }

    regions
        .into_iter()
        .filter(|r| !r.is_empty())
        .map(|r| (*r.first().unwrap(), *r.last().unwrap()))
        .collect()
}

fn novel_regions_before_unchanged<'a>(
    nodes: &[&'a Syntax<'a>],
    change_map: &ChangeMap<'a>,
) -> Vec<(usize, usize)> {
    let mut regions: Vec<Vec<usize>> = vec![];
    let mut region: Option<Vec<usize>> = None;

    for (i, node) in nodes.iter().enumerate() {
        let change = change_map.get(node).expect("Node changes should be set");

        match change {
            Unchanged(_) => {
                // Could have just finished a novel region.
                if let Some(region) = region {
                    regions.push(region);
                }

                region = None;
            }
            Novel => {
                let mut r = if let Some(r) = region { r } else { vec![] };
                r.push(i);
                region = Some(r);
            }
            ReplacedComment(_, _) => {
                region = None;
            }
        }
    }

    if let Some(region) = region {
        regions.push(region);
    }

    regions
        .into_iter()
        .filter(|r| !r.is_empty())
        .map(|r| (*r.first().unwrap(), *r.last().unwrap()))
        .collect()
}

fn is_novel_deep<'a>(node: &Syntax<'a>, change_map: &ChangeMap<'a>) -> bool {
    match node {
        List { children, .. } => {
            if !matches!(change_map.get(node), Some(Novel)) {
                return false;
            }
            for child in children {
                if !is_novel_deep(child, change_map) {
                    return false;
                }
            }

            true
        }
        Atom { .. } => matches!(change_map.get(node), Some(Novel)),
    }
}

fn is_unchanged_deep<'a>(node: &Syntax<'a>, change_map: &ChangeMap<'a>) -> bool {
    match node {
        List { children, .. } => {
            if !matches!(change_map.get(node), Some(Unchanged(_))) {
                return false;
            }
            for child in children {
                if !is_unchanged_deep(child, change_map) {
                    return false;
                }
            }

            true
        }
        Atom { .. } => matches!(change_map.get(node), Some(Unchanged(_))),
    }
}

fn slide_to_prev_node<'a>(
    nodes: &[&'a Syntax<'a>],
    change_map: &mut ChangeMap<'a>,
    start_idx: usize,
    end_idx: usize,
) {
    if start_idx == 0 {
        return;
    }
    if start_idx == end_idx {
        return;
    }

    let start_node = nodes[start_idx];
    let last_node = nodes[end_idx];
    let before_start_node = nodes[start_idx - 1];
    let before_last_node = nodes[end_idx - 1];

    if before_start_node.content_id() != last_node.content_id() {
        return;
    }

    let distance_to_before_start = distance_between(before_start_node, start_node);
    let distance_to_last = distance_between(before_last_node, last_node);

    if distance_to_before_start <= distance_to_last {
        // Deep checks walk the whole tree, so do these last.
        if !is_unchanged_deep(before_start_node, change_map) {
            return;
        }
        for node in &nodes[start_idx..=end_idx] {
            if !is_novel_deep(node, change_map) {
                return;
            }
        }

        let opposite = match change_map
            .get(before_start_node)
            .expect("Node changes should be set")
        {
            Unchanged(n) => n,
            _ => unreachable!(),
        };

        insert_deep_novel(before_start_node, change_map);
        insert_deep_unchanged(last_node, opposite, change_map);
        insert_deep_unchanged(opposite, last_node, change_map);
    }
}

fn slide_to_next_node<'a>(
    nodes: &[&'a Syntax<'a>],
    change_map: &mut ChangeMap<'a>,
    start_idx: usize,
    end_idx: usize,
) {
    if end_idx == nodes.len() - 1 {
        return;
    }
    if start_idx == end_idx {
        return;
    }

    let start_node = nodes[start_idx];
    let last_node = nodes[end_idx];
    let after_start_node = nodes[start_idx + 1];
    let after_last_node = nodes[end_idx + 1];

    if after_last_node.content_id() != start_node.content_id() {
        return;
    }

    let distance_to_start = distance_between(start_node, after_start_node);
    let distance_to_after_last = distance_between(last_node, after_last_node);

    if distance_to_after_last < distance_to_start {
        // Deep checks walk the whole tree, so do these last.
        if !is_unchanged_deep(after_last_node, change_map) {
            return;
        }
        for node in &nodes[start_idx..=end_idx] {
            if !is_novel_deep(node, change_map) {
                return;
            }
        }

        let opposite = match change_map
            .get(after_last_node)
            .expect("Node changes should be set")
        {
            Unchanged(n) => n,
            _ => unreachable!(),
        };

        insert_deep_unchanged(start_node, opposite, change_map);
        insert_deep_unchanged(opposite, start_node, change_map);
        insert_deep_novel(after_last_node, change_map);
    }
}

/// Return the distance between two syntax nodes, as a tuple of number
/// of lines and number of columns.
fn distance_between(prev: &Syntax, next: &Syntax) -> (u32, u32) {
    let prev_pos = prev.last_line_span();
    let next_pos = next.first_line_span();

    if let (Some(prev_pos), Some(next_pos)) = (prev_pos, next_pos) {
        if prev_pos.line != next_pos.line {
            return (next_pos.line.0 - prev_pos.line.0, 0);
        }

        return (0, next_pos.start_col - prev_pos.end_col);
    }

    (0, 0)
}

impl<'a> Syntax<'a> {
    fn first_line_span(&self) -> Option<SingleLineSpan> {
        match self {
            List {
                open_position,
                children,
                close_position,
                ..
            } => {
                if let Some(position) = open_position.first() {
                    return Some(*position);
                }
                for child in children {
                    if let Some(position) = child.first_line_span() {
                        return Some(position);
                    }
                }

                close_position.first().copied()
            }
            Atom { position, .. } => position.first().copied(),
        }
    }

    fn last_line_span(&self) -> Option<SingleLineSpan> {
        match self {
            List {
                open_position,
                children,
                close_position,
                ..
            } => {
                if let Some(position) = close_position.last() {
                    return Some(*position);
                }
                for child in children.iter().rev() {
                    if let Some(position) = child.last_line_span() {
                        return Some(position);
                    }
                }

                open_position.last().copied()
            }
            Atom { position, .. } => position.last().copied(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        parse::guess_language,
        parse::tree_sitter_parser::{from_language, parse},
        parse::syntax::{init_all_info, AtomKind},
    };
    use pretty_assertions::assert_eq;
    use typed_arena::Arena;

    /// Test that we slide at the start if the unchanged node is
    /// closer than the trailing novel node.
    #[test]
    fn test_slider_at_start() {
        let arena = Arena::new();

        let line1a = vec![SingleLineSpan {
            line: 0.into(),
            start_col: 0,
            end_col: 1,
        }];
        let line1b = vec![SingleLineSpan {
            line: 0.into(),
            start_col: 10,
            end_col: 11,
        }];
        let line2 = vec![SingleLineSpan {
            line: 1.into(),
            start_col: 3,
            end_col: 4,
        }];

        let lhs = [
            Syntax::new_atom(&arena, line1a, "a", AtomKind::Comment),
            Syntax::new_atom(&arena, line1b, "b", AtomKind::Comment),
            Syntax::new_atom(&arena, line2, "a", AtomKind::Comment),
        ];

        let pos = vec![SingleLineSpan {
            line: 99.into(),
            start_col: 1,
            end_col: 2,
        }];
        let rhs = [Syntax::new_atom(&arena, pos, "a", AtomKind::Comment)];

        init_all_info(&lhs, &rhs);

        let mut change_map = ChangeMap::default();
        change_map.insert(lhs[0], Unchanged(rhs[0]));
        change_map.insert(lhs[1], Novel);
        change_map.insert(lhs[2], Novel);

        fix_all_sliders(guess_language::Language::EmacsLisp, &lhs, &mut change_map);
        assert_eq!(change_map.get(lhs[0]), Some(Novel));
        assert_eq!(change_map.get(lhs[1]), Some(Novel));
        assert_eq!(change_map.get(lhs[2]), Some(Unchanged(rhs[0])));
        assert_eq!(change_map.get(rhs[0]), Some(Unchanged(lhs[2])));
    }

    /// Test that we slide at the end if the unchanged node is
    /// closer than the leading novel node.
    #[test]
    fn test_slider_at_end() {
        let arena = Arena::new();

        let line1 = vec![SingleLineSpan {
            line: 0.into(),
            start_col: 0,
            end_col: 1,
        }];
        let line2a = vec![SingleLineSpan {
            line: 1.into(),
            start_col: 10,
            end_col: 11,
        }];
        let line2b = vec![SingleLineSpan {
            line: 1.into(),
            start_col: 12,
            end_col: 13,
        }];

        let lhs = [
            Syntax::new_atom(&arena, line1, "a", AtomKind::Comment),
            Syntax::new_atom(&arena, line2a, "b", AtomKind::Comment),
            Syntax::new_atom(&arena, line2b, "a", AtomKind::Comment),
        ];

        let pos = vec![SingleLineSpan {
            line: 99.into(),
            start_col: 1,
            end_col: 2,
        }];
        let rhs = [Syntax::new_atom(&arena, pos, "a", AtomKind::Comment)];

        init_all_info(&lhs, &rhs);

        let mut change_map = ChangeMap::default();
        change_map.insert(lhs[0], Novel);
        change_map.insert(lhs[1], Novel);
        change_map.insert(lhs[2], Unchanged(rhs[0]));

        fix_all_sliders(guess_language::Language::EmacsLisp, &lhs, &mut change_map);

        assert_eq!(change_map.get(rhs[0]), Some(Unchanged(lhs[0])));
        assert_eq!(change_map.get(lhs[0]), Some(Unchanged(rhs[0])));
        assert_eq!(change_map.get(lhs[1]), Some(Novel));
        assert_eq!(change_map.get(lhs[2]), Some(Novel));
    }
    #[test]
    fn test_slider_two_steps() {
        let arena = Arena::new();
        let config = from_language(guess_language::Language::EmacsLisp);

        let lhs = parse(&arena, "A B", &config);
        let rhs = parse(&arena, "A B X\n A B", &config);
        init_all_info(&lhs, &rhs);

        let mut change_map = ChangeMap::default();
        change_map.insert(rhs[0], Unchanged(lhs[0]));
        change_map.insert(rhs[1], Unchanged(lhs[1]));
        change_map.insert(rhs[2], Novel);
        change_map.insert(rhs[3], Novel);
        change_map.insert(rhs[4], Novel);

        fix_all_sliders(guess_language::Language::EmacsLisp, &rhs, &mut change_map);
        assert_eq!(change_map.get(rhs[0]), Some(Novel));
        assert_eq!(change_map.get(rhs[1]), Some(Novel));
        assert_eq!(change_map.get(rhs[2]), Some(Novel));
        assert_eq!(change_map.get(rhs[3]), Some(Unchanged(rhs[0])));
    }
}
