//! Line comparison with exactly the results of Python's `difflib` (`SequenceMatcher`,
//! `unified_diff`), so that change counts and diff texts match the Python version.

use std::collections::{HashMap, HashSet};

/// Operation of a diff opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Equal,
    Replace,
    Delete,
    Insert,
}

/// `(tag, i1, i2, j1, j2)`: `a[i1..i2]` relates to `b[j1..j2]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opcode {
    pub tag: Tag,
    pub i1: usize,
    pub i2: usize,
    pub j1: usize,
    pub j2: usize,
}

/// Python `difflib.SequenceMatcher` without junk function.
#[derive(Debug)]
pub struct SequenceMatcher<'a> {
    a: &'a [String],
    b: &'a [String],
    b2j: HashMap<&'a str, Vec<usize>>,
}

impl<'a> SequenceMatcher<'a> {
    /// `autojunk` as in Python: for `len(b) >= 200`, elements occurring in more than 1 % of `b` are "popular".
    pub fn new(a: &'a [String], b: &'a [String], autojunk: bool) -> Self {
        let mut b2j: HashMap<&'a str, Vec<usize>> = HashMap::new();
        for (index, element) in b.iter().enumerate() {
            b2j.entry(element.as_str()).or_default().push(index);
        }
        let n = b.len();
        if autojunk && n >= 200 {
            let ntest = n / 100 + 1;
            let popular: HashSet<&'a str> =
                b2j.iter().filter(|(_, idxs)| idxs.len() > ntest).map(|(element, _)| *element).collect();
            for element in popular {
                b2j.remove(element);
            }
        }
        Self { a, b, b2j }
    }

    fn same(&self, i: usize, j: usize) -> bool {
        matches!((self.a.get(i), self.b.get(j)), (Some(x), Some(y)) if x == y)
    }

    /// Longest matching block in `a[alo..ahi]` and `b[blo..bhi]` (Python tie-breaking).
    pub fn find_longest_match(&self, alo: usize, ahi: usize, blo: usize, bhi: usize) -> (usize, usize, usize) {
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for i in alo..ahi {
            let mut new_j2len: HashMap<usize, usize> = HashMap::new();
            let indices = self.a.get(i).and_then(|element| self.b2j.get(element.as_str()));
            for &j in indices.map(Vec::as_slice).unwrap_or_default() {
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break;
                }
                let previous = j.checked_sub(1).and_then(|p| j2len.get(&p)).copied().unwrap_or(0);
                let k = previous + 1;
                new_j2len.insert(j, k);
                if k > bestsize {
                    besti = i + 1 - k;
                    bestj = j + 1 - k;
                    bestsize = k;
                }
            }
            j2len = new_j2len;
        }
        // Extend with equal (non-junk) neighbours - relevant for popular elements.
        while besti > alo && bestj > blo && self.same(besti - 1, bestj - 1) {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi && bestj + bestsize < bhi && self.same(besti + bestsize, bestj + bestsize) {
            bestsize += 1;
        }
        (besti, bestj, bestsize)
    }

    pub fn matching_blocks(&self) -> Vec<(usize, usize, usize)> {
        let (la, lb) = (self.a.len(), self.b.len());
        let mut queue = vec![(0usize, la, 0usize, lb)];
        let mut blocks: Vec<(usize, usize, usize)> = Vec::new();
        while let Some((alo, ahi, blo, bhi)) = queue.pop() {
            let (i, j, k) = self.find_longest_match(alo, ahi, blo, bhi);
            if k > 0 {
                blocks.push((i, j, k));
                if alo < i && blo < j {
                    queue.push((alo, i, blo, j));
                }
                if i + k < ahi && j + k < bhi {
                    queue.push((i + k, ahi, j + k, bhi));
                }
            }
        }
        blocks.sort_unstable();
        let (mut i1, mut j1, mut k1) = (0usize, 0usize, 0usize);
        let mut result = Vec::new();
        for (i2, j2, k2) in blocks {
            if i1 + k1 == i2 && j1 + k1 == j2 {
                k1 += k2;
            } else {
                if k1 > 0 {
                    result.push((i1, j1, k1));
                }
                (i1, j1, k1) = (i2, j2, k2);
            }
        }
        if k1 > 0 {
            result.push((i1, j1, k1));
        }
        result.push((la, lb, 0));
        result
    }

    pub fn opcodes(&self) -> Vec<Opcode> {
        let (mut i, mut j) = (0usize, 0usize);
        let mut answer = Vec::new();
        for (ai, bj, size) in self.matching_blocks() {
            let tag = if i < ai && j < bj {
                Some(Tag::Replace)
            } else if i < ai {
                Some(Tag::Delete)
            } else if j < bj {
                Some(Tag::Insert)
            } else {
                None
            };
            if let Some(tag) = tag {
                answer.push(Opcode { tag, i1: i, i2: ai, j1: j, j2: bj });
            }
            i = ai + size;
            j = bj + size;
            if size > 0 {
                answer.push(Opcode { tag: Tag::Equal, i1: ai, i2: i, j1: bj, j2: j });
            }
        }
        answer
    }

    /// Python `get_grouped_opcodes(n)`.
    pub fn grouped_opcodes(&self, n: usize) -> Vec<Vec<Opcode>> {
        let mut codes = self.opcodes();
        if codes.is_empty() {
            codes.push(Opcode { tag: Tag::Equal, i1: 0, i2: 1, j1: 0, j2: 1 });
        }
        if let Some(first) = codes.first_mut() {
            if first.tag == Tag::Equal {
                first.i1 = first.i1.max(first.i2.saturating_sub(n));
                first.j1 = first.j1.max(first.j2.saturating_sub(n));
            }
        }
        if let Some(last) = codes.last_mut() {
            if last.tag == Tag::Equal {
                last.i2 = last.i2.min(last.i1 + n);
                last.j2 = last.j2.min(last.j1 + n);
            }
        }
        let nn = n + n;
        let mut groups = Vec::new();
        let mut group: Vec<Opcode> = Vec::new();
        for code in codes {
            let mut code = code;
            if code.tag == Tag::Equal && code.i2 - code.i1 > nn {
                group.push(Opcode {
                    tag: Tag::Equal,
                    i1: code.i1,
                    i2: code.i2.min(code.i1 + n),
                    j1: code.j1,
                    j2: code.j2.min(code.j1 + n),
                });
                groups.push(std::mem::take(&mut group));
                code.i1 = code.i1.max(code.i2.saturating_sub(n));
                code.j1 = code.j1.max(code.j2.saturating_sub(n));
            }
            group.push(code);
        }
        let only_equal = group.len() == 1 && group.first().is_some_and(|c| c.tag == Tag::Equal);
        if !group.is_empty() && !only_equal {
            groups.push(group);
        }
        groups
    }
}

fn format_range_unified(start: usize, stop: usize) -> String {
    let mut beginning = start + 1;
    let length = stop - start;
    if length == 1 {
        return beginning.to_string();
    }
    if length == 0 {
        beginning -= 1;
    }
    format!("{beginning},{length}")
}

fn slice(lines: &[String], from: usize, to: usize) -> &[String] {
    lines.get(from..to).unwrap_or_default()
}

/// Python `difflib.unified_diff(a, b, fromfile, tofile, n=n, lineterm="")` (autojunk on, as in Python).
pub fn unified_diff(a: &[String], b: &[String], from_file: &str, to_file: &str, n: usize) -> Vec<String> {
    let matcher = SequenceMatcher::new(a, b, true);
    let mut out = Vec::new();
    for group in matcher.grouped_opcodes(n) {
        if out.is_empty() {
            out.push(format!("--- {from_file}"));
            out.push(format!("+++ {to_file}"));
        }
        let (Some(first), Some(last)) = (group.first(), group.last()) else {
            continue;
        };
        out.push(format!(
            "@@ -{} +{} @@",
            format_range_unified(first.i1, last.i2),
            format_range_unified(first.j1, last.j2)
        ));
        for code in &group {
            if code.tag == Tag::Equal {
                out.extend(slice(a, code.i1, code.i2).iter().map(|line| format!(" {line}")));
                continue;
            }
            if matches!(code.tag, Tag::Replace | Tag::Delete) {
                out.extend(slice(a, code.i1, code.i2).iter().map(|line| format!("-{line}")));
            }
            if matches!(code.tag, Tag::Replace | Tag::Insert) {
                out.extend(slice(b, code.j1, code.j2).iter().map(|line| format!("+{line}")));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.split(' ').filter(|s| !s.is_empty()).map(String::from).collect()
    }

    #[test]
    fn opcodes_like_python() {
        // difflib.SequenceMatcher(None, "a b c d".split(), "a x c d e".split()).get_opcodes()
        let a = lines("a b c d");
        let b = lines("a x c d e");
        let codes = SequenceMatcher::new(&a, &b, false).opcodes();
        let tags: Vec<(Tag, usize, usize, usize, usize)> =
            codes.iter().map(|c| (c.tag, c.i1, c.i2, c.j1, c.j2)).collect();
        assert_eq!(
            tags,
            vec![
                (Tag::Equal, 0, 1, 0, 1),
                (Tag::Replace, 1, 2, 1, 2),
                (Tag::Equal, 2, 4, 2, 4),
                (Tag::Insert, 4, 4, 4, 5)
            ]
        );
    }

    #[test]
    fn unified_like_python() {
        let a = lines("1 2 3 4 5 6 7 8");
        let b = lines("1 2 X 4 5 6 7 8 9");
        let diff = unified_diff(&a, &b, "alt", "neu", 1);
        assert_eq!(
            diff,
            vec!["--- alt", "+++ neu", "@@ -2,3 +2,3 @@", " 2", "-3", "+X", " 4", "@@ -8 +8,2 @@", " 8", "+9"]
        );
        assert!(unified_diff(&a, &a, "x", "y", 1).is_empty());
        let empty: Vec<String> = Vec::new();
        assert_eq!(unified_diff(&empty, &lines("a"), "x", "y", 1), vec!["--- x", "+++ y", "@@ -0,0 +1 @@", "+a"]);
    }
}
