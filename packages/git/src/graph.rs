// Адаптировано из Zed: crates/git_ui/src/git_graph.rs
//! Lanes of a commit graph. Zed's `GraphData` assigns every commit a lane
//! and joins children to parents with lines of straight runs and curves; here
//! the lines are cut into the halves of rows the `lanes` widget draws.

use std::collections::HashMap;

/// Distinct line colors of the client palette.
pub const COLORS: usize = 8;
/// Lanes the widget accepts; wider graphs are squeezed into the last lane.
pub const LANES: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub from: usize,
    pub to: usize,
    pub color: usize,
}

/// One commit row: its dot and the lines above (`up`) and below (`down`) it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub lane: usize,
    pub color: usize,
    pub up: Vec<Segment>,
    pub down: Vec<Segment>,
}

const OPEN: usize = usize::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Curve {
    /// Leaves a merge commit sideways towards another parent.
    Merge,
    /// Bends into the lane of the parent it reaches.
    Checkout,
}

#[derive(Clone, Copy, Debug)]
enum Piece {
    Straight {
        to_row: usize,
    },
    Curve {
        to_column: usize,
        on_row: usize,
        kind: Curve,
    },
}

#[derive(Debug)]
enum Lane {
    Empty,
    Active {
        color: Option<usize>,
        starting_row: usize,
        starting_col: usize,
        pieces: Vec<Piece>,
    },
}

struct Line {
    child_column: usize,
    start: usize,
    color: usize,
    pieces: Vec<Piece>,
}

impl Lane {
    /// Ends the line of this lane at `ending_row`, in `parent_column`.
    fn close(
        &mut self,
        ending_row: usize,
        lane_column: usize,
        parent_column: usize,
        parent_color: usize,
    ) -> Option<Line> {
        let Lane::Active {
            color,
            starting_row,
            starting_col,
            mut pieces,
        } = std::mem::replace(self, Lane::Empty)
        else {
            return None;
        };
        let destination = parent_column;
        let checkout = Piece::Curve {
            to_column: destination,
            on_row: ending_row,
            kind: Curve::Checkout,
        };
        let last = pieces.len().saturating_sub(1);
        match pieces.last_mut() {
            Some(Piece::Straight { to_row }) if *to_row == OPEN => {
                if destination != lane_column {
                    *to_row = ending_row - 1;
                    if *to_row == starting_row {
                        pieces[last] = checkout;
                    } else {
                        pieces.push(checkout);
                    }
                } else {
                    *to_row = ending_row;
                }
            }
            Some(Piece::Curve {
                on_row,
                to_column,
                kind,
            }) if *on_row == OPEN => {
                if *to_column == OPEN {
                    *to_column = destination;
                }
                let to_column = *to_column;
                if *kind == Curve::Merge {
                    *on_row = starting_row + 1;
                    if *on_row < ending_row {
                        if to_column != destination {
                            pieces.push(Piece::Straight {
                                to_row: ending_row - 1,
                            });
                            pieces.push(checkout);
                        } else {
                            pieces.push(Piece::Straight { to_row: ending_row });
                        }
                    } else if to_column != destination {
                        pieces.push(checkout);
                    }
                } else {
                    *on_row = ending_row;
                    if to_column != destination {
                        pieces.push(Piece::Straight { to_row: ending_row });
                        pieces.push(checkout);
                    }
                }
            }
            Some(Piece::Curve {
                on_row, to_column, ..
            }) => {
                if *on_row < ending_row {
                    if *to_column != destination {
                        pieces.push(Piece::Straight {
                            to_row: ending_row - 1,
                        });
                        pieces.push(checkout);
                    } else {
                        pieces.push(Piece::Straight { to_row: ending_row });
                    }
                } else if *to_column != destination {
                    pieces.push(checkout);
                }
            }
            _ => {}
        }
        Some(Line {
            child_column: starting_col,
            start: starting_row,
            color: color.unwrap_or(parent_color),
            pieces,
        })
    }
}

#[derive(Default)]
struct Graph<'a> {
    lanes: Vec<Lane>,
    lane_colors: HashMap<usize, usize>,
    parent_to_lanes: HashMap<&'a str, Vec<usize>>,
    next_color: usize,
    /// Lane of every commit row.
    commits: Vec<(usize, usize)>,
    lines: Vec<Line>,
}

impl<'a> Graph<'a> {
    fn first_empty_lane(&mut self) -> usize {
        self.lanes
            .iter()
            .position(|lane| matches!(lane, Lane::Empty))
            .unwrap_or_else(|| {
                self.lanes.push(Lane::Empty);
                self.lanes.len() - 1
            })
    }

    fn lane_color(&mut self, lane: usize) -> usize {
        *self.lane_colors.entry(lane).or_insert_with(|| {
            let color = self.next_color;
            self.next_color = (self.next_color + 1) % COLORS;
            color
        })
    }

    fn add(&mut self, hash: &'a str, parents: &[&'a str]) {
        let row = self.commits.len();
        let lane = self
            .parent_to_lanes
            .get(hash)
            .and_then(|lanes| lanes.iter().min().copied())
            .unwrap_or_else(|| self.first_empty_lane());
        let color = self.lane_color(lane);
        for column in self.parent_to_lanes.remove(hash).unwrap_or_default() {
            let state = &mut self.lanes[column];
            if let Lane::Active {
                starting_row,
                pieces,
                ..
            } = state
                && let Some(Piece::Curve {
                    to_column,
                    kind: Curve::Merge,
                    ..
                }) = pieces.first_mut()
            {
                // The merge curve would cross commits drawn on the parent's lane.
                let curve_row = *starting_row + 1;
                if column != lane
                    && curve_row < row
                    && self.commits[curve_row..row]
                        .iter()
                        .any(|(at, _)| *at == lane)
                {
                    *to_column = column;
                }
            }
            if let Some(line) = state.close(row, column, lane, color) {
                self.lines.push(line);
            }
        }
        for (index, parent) in parents.iter().enumerate() {
            let (column, color, piece) = if index == 0 {
                (lane, Some(color), Piece::Straight { to_row: OPEN })
            } else {
                let merge = Piece::Curve {
                    to_column: OPEN,
                    on_row: OPEN,
                    kind: Curve::Merge,
                };
                (self.first_empty_lane(), None, merge)
            };
            self.lanes[column] = Lane::Active {
                color,
                starting_row: row,
                starting_col: lane,
                pieces: vec![piece],
            };
            self.parent_to_lanes.entry(parent).or_default().push(column);
        }
        self.commits.push((lane, color));
    }

    /// Lines to parents outside the loaded commits run off the bottom edge.
    fn finish(&mut self) {
        let end = self.commits.len();
        for column in 0..self.lanes.len() {
            if matches!(self.lanes[column], Lane::Empty) {
                continue;
            }
            let color = self.lane_color(column);
            if let Some(line) = self.lanes[column].close(end, column, column, color) {
                self.lines.push(line);
            }
        }
    }
}

fn put(halves: &mut [Row], index: usize, up: bool, from: usize, to: usize, color: usize) {
    let Some(row) = halves.get_mut(index) else {
        return;
    };
    let segment = Segment {
        from: from.min(LANES - 1),
        to: to.min(LANES - 1),
        color,
    };
    let list = if up { &mut row.up } else { &mut row.down };
    if !list.contains(&segment) {
        list.push(segment);
    }
}

/// A pass of a line from the middle of row `row` to the middle of the next
/// one, moving from lane `from` to lane `to`.
struct Step {
    row: usize,
    from: usize,
    to: usize,
    kind: Curve,
}

fn steps(line: &Line, end: usize) -> Vec<Step> {
    let (mut row, mut column) = (line.start, line.child_column);
    let mut steps: Vec<Step> = Vec::new();
    let straight = |steps: &mut Vec<Step>, row: &mut usize, column: usize, to_row: usize| {
        while *row < to_row.min(end) {
            steps.push(Step {
                row: *row,
                from: column,
                to: column,
                kind: Curve::Checkout,
            });
            *row += 1;
        }
    };
    for piece in &line.pieces {
        match *piece {
            Piece::Straight { to_row } => straight(&mut steps, &mut row, column, to_row),
            Piece::Curve {
                to_column, on_row, ..
            } if row >= on_row => {
                // Moves sideways on the row it already reached: the last pass bends further.
                if let Some(step) = steps.last_mut() {
                    step.to = to_column;
                }
                column = to_column;
            }
            Piece::Curve {
                to_column,
                on_row,
                kind,
            } => {
                if kind == Curve::Checkout {
                    straight(&mut steps, &mut row, column, on_row - 1);
                }
                if row < end {
                    steps.push(Step {
                        row,
                        from: column,
                        to: to_column,
                        kind,
                    });
                    row += 1;
                }
                column = to_column;
                straight(&mut steps, &mut row, column, on_row);
            }
        }
    }
    steps
}

fn draw(rows: &mut [Row], line: &Line) {
    let color = line.color;
    for step in steps(line, rows.len()) {
        // A merge bends right below its commit, a checkout right above the parent.
        let (down, up) = match step.kind {
            Curve::Merge => ((step.from, step.to), (step.to, step.to)),
            Curve::Checkout => ((step.from, step.from), (step.from, step.to)),
        };
        put(rows, step.row, false, down.0, down.1, color);
        put(rows, step.row + 1, true, up.0, up.1, color);
    }
}

/// Rows of commits listed newest first, each with the hashes of its parents.
pub fn layout<'a>(commits: &[(&'a str, Vec<&'a str>)]) -> Vec<Row> {
    let mut graph = Graph::default();
    for (hash, parents) in commits {
        graph.add(hash, parents);
    }
    graph.finish();
    let mut rows: Vec<Row> = graph
        .commits
        .iter()
        .map(|&(lane, color)| Row {
            lane: lane.min(LANES - 1),
            color,
            ..Row::default()
        })
        .collect();
    for line in &graph.lines {
        draw(&mut rows, line);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(from: usize, to: usize, color: usize) -> Segment {
        Segment { from, to, color }
    }

    #[test]
    fn a_linear_history_stays_on_one_lane() {
        let rows = layout(&[("c", vec!["b"]), ("b", vec!["a"]), ("a", vec![])]);
        assert!(rows.iter().all(|row| row.lane == 0 && row.color == 0));
        assert_eq!(rows[0].up, []);
        assert_eq!(rows[0].down, [segment(0, 0, 0)]);
        assert_eq!(rows[1].up, [segment(0, 0, 0)]);
        assert_eq!(rows[2].down, []);
    }

    #[test]
    fn a_merge_opens_a_lane_and_the_fork_closes_it() {
        // m merges b into a; both come from c.
        let rows = layout(&[
            ("m", vec!["a", "b"]),
            ("a", vec!["c"]),
            ("b", vec!["c"]),
            ("c", vec![]),
        ]);
        let lanes: Vec<usize> = rows.iter().map(|row| row.lane).collect();
        assert_eq!(lanes, [0, 0, 1, 0]);
        assert_eq!(rows[0].down, [segment(0, 0, 0), segment(0, 1, 1)]);
        assert!(rows[1].up.contains(&segment(1, 1, 1)));
        assert!(rows[1].down.contains(&segment(1, 1, 1)));
        let mut up = rows[2].up.clone();
        up.sort_by_key(|segment| segment.from);
        assert_eq!(up, [segment(0, 0, 0), segment(1, 1, 1)]);
        assert!(rows[2].down.contains(&segment(1, 1, 1)));
        assert!(rows[3].up.contains(&segment(1, 0, 1)));
        assert!(rows[3].up.contains(&segment(0, 0, 0)));
        assert_eq!(rows[3].down, []);
    }

    #[test]
    fn unloaded_parents_run_off_the_bottom() {
        let rows = layout(&[("b", vec!["a"]), ("x", vec!["y"])]);
        assert_eq!((rows[0].lane, rows[1].lane), (0, 1));
        assert_eq!(rows[1].up, [segment(0, 0, 0)]);
        assert_eq!(rows[1].down, [segment(0, 0, 0), segment(1, 1, 1)]);
    }

    #[test]
    fn a_short_merge_bends_straight_into_its_parent() {
        // The merged parent is the very next row on another lane.
        let rows = layout(&[("m", vec!["a", "b"]), ("b", vec!["a"]), ("a", vec![])]);
        assert_eq!(
            rows.iter().map(|row| row.lane).collect::<Vec<_>>(),
            [0, 1, 0]
        );
        assert!(rows[0].down.contains(&segment(0, 1, 1)));
        assert!(rows[1].up.contains(&segment(1, 1, 1)));
        assert!(rows[2].up.contains(&segment(1, 0, 1)));
        assert!(rows[2].up.contains(&segment(0, 0, 0)));
    }
}
