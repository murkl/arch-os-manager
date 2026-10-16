//! pacman's version order, ported from alpm_pkg_vercmp and rpmvercmp in libalpm/version.c.

use std::cmp::Ordering::{self, Equal, Greater, Less};

pub fn vercmp(a: &str, b: &str) -> Ordering {
    if a == b {
        return Equal;
    }
    let (ea, va, ra) = split(a);
    let (eb, vb, rb) = split(b);
    rpmvercmp(ea, eb)
        .then_with(|| rpmvercmp(va, vb))
        .then_with(|| match (ra, rb) {
            (Some(x), Some(y)) => rpmvercmp(x, y),
            _ => Equal,
        })
}

/// Epoch, version and release, as parseEVR splits them.
fn split(v: &str) -> (&str, &str, Option<&str>) {
    let digits = v.bytes().take_while(u8::is_ascii_digit).count();
    let (epoch, rest) = match v.as_bytes().get(digits) {
        Some(b':') => (
            if digits == 0 { "0" } else { &v[..digits] },
            &v[digits + 1..],
        ),
        _ => ("0", v),
    };
    match rest.rsplit_once('-') {
        Some((ver, rel)) => (epoch, ver, Some(rel)),
        None => (epoch, rest, None),
    }
}

fn rpmvercmp(a: &str, b: &str) -> Ordering {
    if a == b {
        return Equal;
    }
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let at = |s: &[u8], i: usize| s.get(i).copied().unwrap_or(0);
    let (mut one, mut two, mut p1, mut p2) = (0, 0, 0, 0);
    while at(a, one) != 0 && at(b, two) != 0 {
        while at(a, one) != 0 && !at(a, one).is_ascii_alphanumeric() {
            one += 1;
        }
        while at(b, two) != 0 && !at(b, two).is_ascii_alphanumeric() {
            two += 1;
        }
        if at(a, one) == 0 || at(b, two) == 0 {
            break;
        }
        // Separators of different length decide on their own.
        if one - p1 != two - p2 {
            return (one - p1).cmp(&(two - p2));
        }
        p1 = one;
        p2 = two;
        let numeric = at(a, p1).is_ascii_digit();
        let same = |c: u8| {
            if numeric {
                c.is_ascii_digit()
            } else {
                c.is_ascii_alphabetic()
            }
        };
        while at(a, p1) != 0 && same(at(a, p1)) {
            p1 += 1;
        }
        while at(b, p2) != 0 && same(at(b, p2)) {
            p2 += 1;
        }
        if two == p2 {
            // A number is always newer than letters.
            return if numeric { Greater } else { Less };
        }
        let (mut s1, mut s2) = (&a[one..p1], &b[two..p2]);
        if numeric {
            s1 = &s1[s1.iter().take_while(|c| **c == b'0').count()..];
            s2 = &s2[s2.iter().take_while(|c| **c == b'0').count()..];
            if s1.len() != s2.len() {
                return s1.len().cmp(&s2.len());
            }
        }
        match s1.cmp(s2) {
            Equal => {}
            other => return other,
        }
        one = p1;
        two = p2;
    }
    if at(a, one) == 0 && at(b, two) == 0 {
        return Equal;
    }
    // A remaining letter segment never beats an empty one.
    if (at(a, one) == 0 && !at(b, two).is_ascii_alphabetic()) || at(a, one).is_ascii_alphabetic() {
        Less
    } else {
        Greater
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_as_pacman_does() {
        // Each checked against vercmp(8) on Arch.
        for (a, b, want) in [
            ("1.5.0", "1.5.0", Equal),
            ("1.5.1", "1.5.0", Greater),
            ("1.5.1", "1.5", Greater),
            ("1.5.0", "1.5", Greater),
            ("1.5b", "1.5", Less),
            ("1.5b", "1.5.1", Less),
            ("1.0a", "1.0alpha", Less),
            ("1.0alpha", "1.0b", Less),
            ("1.0b", "1.0beta", Less),
            ("1.0beta", "1.0pre", Less),
            ("1.0pre", "1.0rc", Less),
            ("1.0rc", "1.0", Less),
            ("1.0", "1.0.a", Less),
            ("1.0.a", "1.0.1", Less),
            ("1.0", "1.0a", Greater),
            ("1.5-1", "1.5-2", Less),
            ("1.5-2", "1.5", Equal),
            ("0:1.0", "1.0", Equal),
            ("1:1.0", "2.0", Greater),
            ("1.0", "1:1.0", Less),
            ("1.1", "1.01", Equal),
            ("10.0", "9.9", Greater),
            ("7.2.9.zen1-1", "7.2.10.zen1-1", Less),
            ("1.1.a", "1.1+a", Equal),
            ("1.1..a", "1.1.a", Greater),
        ] {
            assert_eq!(vercmp(a, b), want, "{a} vs {b}");
            assert_eq!(vercmp(b, a), want.reverse(), "{b} vs {a}");
        }
    }
}
