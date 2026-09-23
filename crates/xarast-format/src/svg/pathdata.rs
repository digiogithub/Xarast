//! The compact path-data serialiser: pass 2 of `research/06 §4.5.1`.
//!
//! Every segment is written in whichever of its absolute and relative
//! spellings is shorter — a per-segment choice, which is never longer than
//! choosing once per path — with:
//!
//! - repeated commands collapsed (`l1 0 2 0`, not `l1 0l2 0`);
//! - `h`/`v` for axial lines;
//! - `s` for a cubic whose first control point reflects the previous
//!   cubic's second one (the model has no quadratics, so there is no `t`);
//! - `z` for closure, and no line back to the start when the closure
//!   already draws it.
//!
//! Coordinates arrive as integral millipoints in SVG space (Y down), so a
//! relative coordinate is an exact integer difference: no drift accumulates
//! along a long relative path, which is what makes the relative form safe.

use xarast_geom::{Path, Verb};

use super::num::Num;

/// A point in SVG user space, in millipoints.
pub type SvgPt = (i64, i64);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cmd {
    M,
    L,
    H,
    V,
    C,
    S,
    Z,
}

impl Cmd {
    fn letter(self, rel: bool) -> char {
        let c = match self {
            Cmd::M => 'M',
            Cmd::L => 'L',
            Cmd::H => 'H',
            Cmd::V => 'V',
            Cmd::C => 'C',
            Cmd::S => 'S',
            Cmd::Z => 'Z',
        };
        if rel { c.to_ascii_lowercase() } else { c }
    }
}

/// Up to six numbers (a cubic), formatted on the stack.
#[derive(Clone, Copy)]
struct Nums {
    v: [Num; 6],
    n: usize,
}

impl Nums {
    fn of(values: &[i64]) -> Nums {
        let mut s = Nums {
            v: [Num::mp(0); 6],
            n: 0,
        };
        for (slot, &x) in s.v.iter_mut().zip(values) {
            *slot = Num::mp(x);
            s.n += 1;
        }
        s
    }

    fn points(points: &[SvgPt]) -> Nums {
        let mut flat = [0i64; 6];
        let mut k = 0usize;
        for (p, [x, y]) in points.iter().zip(flat.as_chunks_mut::<2>().0) {
            *x = p.0;
            *y = p.1;
            k += 2;
        }
        Nums::of(flat.get(..k).unwrap_or(&[]))
    }

    fn iter(&self) -> impl Iterator<Item = &Num> {
        self.v.iter().take(self.n)
    }

    /// The written length, separators included (upper bound: one
    /// separator between each pair).
    fn cost(&self) -> usize {
        self.iter().map(Num::len).sum::<usize>() + self.n.saturating_sub(1)
    }
}

struct Writer {
    out: String,
    last: Option<(Cmd, bool)>,
    sep: bool,
    /// Whether the last number written has a fractional part.
    last_dot: bool,
}

impl Writer {
    /// Emits one command, omitting the letter when it repeats (except `M`,
    /// whose repetition means a line).
    fn cmd(&mut self, cmd: Cmd, rel: bool, nums: &Nums) {
        // After a moveto, further coordinate pairs are linetos of the same
        // relativity, so an `L` straight after an `M` needs no letter.
        if self.needs_letter(cmd, rel) {
            self.out.push(cmd.letter(rel));
            self.sep = false;
        }
        for n in nums.iter() {
            // The fewest separators a number parser accepts: none before a
            // `-`, none before a leading `.` when the previous number
            // already has a fraction, a space otherwise.
            let s = n.as_str();
            if self.sep && !(s.starts_with('-') || (s.starts_with('.') && self.last_dot)) {
                self.out.push(' ');
            }
            self.out.push_str(s);
            self.sep = true;
            self.last_dot = n.has_dot();
        }
        self.last = Some((cmd, rel));
    }
}

impl Writer {
    /// Whether writing `cmd` with this relativity needs its letter.
    fn needs_letter(&self, cmd: Cmd, rel: bool) -> bool {
        !((self.last == Some((cmd, rel)) && cmd != Cmd::M && cmd != Cmd::Z)
            || (cmd == Cmd::L && self.last == Some((Cmd::M, rel))))
    }

    /// Picks the shorter spelling, counting the command letter it would
    /// need. A tie keeps the current relativity, then prefers absolute.
    fn pick(&self, cmd: Cmd, abs: Nums, rel: Nums) -> (bool, Nums) {
        let ca = abs.cost() + usize::from(self.needs_letter(cmd, false));
        let cr = rel.cost() + usize::from(self.needs_letter(cmd, true));
        if cr < ca || (cr == ca && self.last.is_some_and(|(_, r)| r)) {
            (true, rel)
        } else {
            (false, abs)
        }
    }
}

fn nums(points: &[SvgPt]) -> Nums {
    Nums::points(points)
}

fn delta(p: SvgPt, from: SvgPt) -> SvgPt {
    (p.0 - from.0, p.1 - from.1)
}

/// Serialises a path, mapping each model point through `map`.
#[must_use]
pub fn path_data(path: &Path, map: impl Fn(xarast_geom::Point) -> SvgPt) -> String {
    let mut w = Writer {
        out: String::with_capacity(16 + path.points().len() * 12),
        last: None,
        sep: false,
        last_dot: false,
    };
    let points = path.points();
    let mut pi = 0usize;
    let mut next = || {
        let p = points.get(pi).map(|p| map(*p));
        pi += 1;
        p
    };
    let mut cur: SvgPt = (0, 0);
    let mut start: SvgPt = (0, 0);
    // The previous cubic's second control point, for `S`.
    let mut prev_c2: Option<SvgPt> = None;
    let mut open = false;

    for verb in path.verbs() {
        match verb {
            Verb::MoveTo => {
                let Some(p) = next() else { break };
                let (rel, n) = if w.last.is_none() {
                    // The first moveto is absolute whatever it is written as.
                    (false, nums(&[p]))
                } else {
                    w.pick(Cmd::M, nums(&[p]), nums(&[delta(p, cur)]))
                };
                w.cmd(Cmd::M, rel, &n);
                cur = p;
                start = p;
                prev_c2 = None;
                open = true;
            }
            Verb::LineTo => {
                let Some(p) = next() else { break };
                if w.last.is_none() {
                    // Path data must start with a moveto.
                    w.cmd(Cmd::M, false, &nums(&[cur]));
                }
                // A line with no moveto after a closure starts from the
                // subpath's start, which is also what SVG does after `z`.
                open = true;
                if p.1 == cur.1 && p.0 != cur.0 {
                    let (rel, n) = w.pick(Cmd::H, Nums::of(&[p.0]), Nums::of(&[p.0 - cur.0]));
                    w.cmd(Cmd::H, rel, &n);
                } else if p.0 == cur.0 && p.1 != cur.1 {
                    let (rel, n) = w.pick(Cmd::V, Nums::of(&[p.1]), Nums::of(&[p.1 - cur.1]));
                    w.cmd(Cmd::V, rel, &n);
                } else {
                    let (rel, n) = w.pick(Cmd::L, nums(&[p]), nums(&[delta(p, cur)]));
                    w.cmd(Cmd::L, rel, &n);
                }
                cur = p;
                prev_c2 = None;
            }
            Verb::CubicTo => {
                let (Some(c1), Some(c2), Some(p)) = (next(), next(), next()) else {
                    break;
                };
                if w.last.is_none() {
                    w.cmd(Cmd::M, false, &nums(&[cur]));
                }
                open = true;
                let reflected = prev_c2.map(|q| (2 * cur.0 - q.0, 2 * cur.1 - q.1));
                if reflected == Some(c1) {
                    let (rel, n) = w.pick(
                        Cmd::S,
                        nums(&[c2, p]),
                        nums(&[delta(c2, cur), delta(p, cur)]),
                    );
                    w.cmd(Cmd::S, rel, &n);
                } else {
                    let (rel, n) = w.pick(
                        Cmd::C,
                        nums(&[c1, c2, p]),
                        nums(&[delta(c1, cur), delta(c2, cur), delta(p, cur)]),
                    );
                    w.cmd(Cmd::C, rel, &n);
                }
                cur = p;
                prev_c2 = Some(c2);
            }
            Verb::Close => {
                if !open {
                    continue;
                }
                w.cmd(Cmd::Z, true, &Nums::of(&[]));
                cur = start;
                prev_c2 = None;
                open = false;
            }
        }
    }
    w.out
}

#[cfg(test)]
mod tests {
    use super::*;
    use xarast_geom::Point;

    fn id(p: Point) -> SvgPt {
        (i64::from(p.x.raw()), i64::from(p.y.raw()))
    }

    #[test]
    fn a_square_uses_h_and_v_and_closes() {
        let mut b = Path::builder();
        b.move_to(Point::raw(10_000, 10_000))
            .line_to(Point::raw(110_000, 10_000))
            .line_to(Point::raw(110_000, 60_000))
            .line_to(Point::raw(10_000, 60_000))
            .close();
        assert_eq!(path_data(&b.build(), id), "M10 10H110V60H10z");
    }

    #[test]
    fn repeated_commands_collapse_and_relative_wins_when_shorter() {
        let mut b = Path::builder();
        b.move_to(Point::raw(100_000, 100_000))
            .line_to(Point::raw(101_000, 101_500))
            .line_to(Point::raw(102_000, 103_000));
        assert_eq!(path_data(&b.build(), id), "M100 100l1 1.5 1 1.5");
    }

    #[test]
    fn a_reflected_control_point_becomes_s() {
        let mut b = Path::builder();
        b.move_to(Point::raw(0, 0))
            .cubic_to(
                Point::raw(0, 10_000),
                Point::raw(10_000, 10_000),
                Point::raw(10_000, 0),
            )
            .cubic_to(
                Point::raw(10_000, -10_000),
                Point::raw(20_000, -10_000),
                Point::raw(20_000, 0),
            );
        assert_eq!(path_data(&b.build(), id), "M0 0C0 10 10 10 10 0S20-10 20 0");
    }

    #[test]
    fn fractions_drop_their_leading_zero_and_share_separators() {
        let mut b = Path::builder();
        b.move_to(Point::raw(500, -250))
            .line_to(Point::raw(-500, 750));
        assert_eq!(path_data(&b.build(), id), "M.5-.25l-1 1");
    }

    #[test]
    fn a_fraction_after_a_signed_integer_keeps_its_space() {
        // `1.5-5.5` would read as 1.5, -5.5: the dot that allows dropping
        // the separator must be the previous number's own.
        let mut b = Path::builder();
        b.move_to(Point::raw(0, 0)).cubic_to(
            Point::raw(1_500, -5_000),
            Point::raw(500, 2_000),
            Point::raw(3_000, 3_000),
        );
        assert_eq!(path_data(&b.build(), id), "M0 0C1.5-5 .5 2 3 3");
    }

    #[test]
    fn a_second_subpath_starts_relative_to_the_closed_one() {
        let mut b = Path::builder();
        b.move_to(Point::raw(100_000, 100_000))
            .line_to(Point::raw(200_000, 150_000))
            .close()
            .move_to(Point::raw(101_000, 101_000))
            .line_to(Point::raw(102_000, 103_000));
        assert_eq!(path_data(&b.build(), id), "M100 100 200 150zm1 1 1 2");
    }
}
