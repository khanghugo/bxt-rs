use std::cmp::min;
use std::num::NonZeroU32;

use hltas::types::{FrameBulk, Line};
use hltas::HLTAS;
use serde::{Deserialize, Serialize};

use super::utils::line_first_frame_idx;
use crate::modules::tas_studio::editor::utils::{
    bulk_and_first_frame_idx, line_idx_and_repeat_at_frame, FrameBulkExt,
};

// This enum is stored in a SQLite DB as bincode bytes. All changes MUST BE BACKWARDS COMPATIBLE to
// be able to load old projects.
/// A basic operation on a HLTAS.
///
/// All operations can be applied and undone. They therefore store enough information to be able to
/// do that. For example, [`SetFrameCount`] stores the original frame count together with the new
/// one.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Operation {
    SetFrameCount {
        bulk_idx: usize,
        from: u32,
        to: u32,
    },
    SetYaw {
        bulk_idx: usize,
        from: f32,
        to: f32,
    },
    Delete {
        line_idx: usize,
        line: String,
    },
    Split {
        frame_idx: usize,
    },
    Replace {
        line_idx: usize,
        from: String,
        to: String,
    },
    ToggleKey {
        bulk_idx: usize,
        key: Key,
        to: bool,
    },
    Insert {
        line_idx: usize,
        line: String,
    },
    SetLeftRightCount {
        bulk_idx: usize,
        from: u32,
        to: u32,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Key {
    Forward,
    Left,
    Right,
    Back,
    Up,
    Down,

    Jump,
    Duck,
    Use,
    Attack1,
    Attack2,
    Reload,
}

// The semantics of apply() or undo() MUST NOT CHANGE, because that will break persistent undo/redo
// for old projects.
impl Operation {
    /// Applies operation to HLTAS and returns index of first affected frame.
    ///
    /// Returns `None` if all frames remain valid.
    pub fn apply(&self, hltas: &mut HLTAS) -> Option<usize> {
        match *self {
            Operation::SetFrameCount { bulk_idx, from, to } => {
                let (bulk, first_frame_idx) = bulk_and_first_frame_idx(hltas)
                    .nth(bulk_idx)
                    .expect("invalid bulk index");

                assert_eq!(bulk.frame_count.get(), from, "wrong current frame count");

                if from != to {
                    bulk.frame_count = NonZeroU32::new(to).expect("invalid new frame count");
                    return Some(first_frame_idx + min(from, to) as usize);
                }
            }
            Operation::SetYaw { bulk_idx, from, to } => {
                let (bulk, first_frame_idx) = bulk_and_first_frame_idx(hltas)
                    .nth(bulk_idx)
                    .expect("invalid bulk index");

                let yaw = bulk.yaw_mut().expect("frame bulk should have yaw");
                assert_eq!(*yaw, from, "wrong current yaw");

                if *yaw != to {
                    *yaw = to;
                    return Some(first_frame_idx);
                }
            }
            Operation::Delete { line_idx, .. } => {
                let first_frame_idx = line_first_frame_idx(hltas)
                    .nth(line_idx)
                    .expect("invalid line index");

                hltas.lines.remove(line_idx);
                return Some(first_frame_idx);
            }
            Operation::Split { frame_idx } => {
                let (line_idx, repeat) = line_idx_and_repeat_at_frame(&hltas.lines, frame_idx)
                    .expect("invalid frame index");

                assert!(repeat > 0, "repeat should be above 0");

                let bulk = hltas.lines[line_idx].frame_bulk_mut().unwrap();
                let mut new_bulk = bulk.clone();
                new_bulk.frame_count = NonZeroU32::new(bulk.frame_count.get() - repeat)
                    .expect("frame bulk should have more than 1 repeat");
                bulk.frame_count = NonZeroU32::new(repeat).unwrap();

                hltas.lines.insert(line_idx + 1, Line::FrameBulk(new_bulk));

                // Splitting does not invalidate any frames.
            }
            Operation::Replace {
                line_idx, ref to, ..
            } => {
                let to = hltas::read::line(to).expect("line should be parse-able").1;

                let first_frame_idx = line_first_frame_idx(hltas)
                    .nth(line_idx)
                    .expect("invalid line index");

                hltas.lines[line_idx] = to;
                return Some(first_frame_idx);
            }
            Operation::ToggleKey { bulk_idx, key, to } => {
                let (bulk, first_frame_idx) = bulk_and_first_frame_idx(hltas)
                    .nth(bulk_idx)
                    .expect("invalid bulk index");

                let value = key.value_mut(bulk);
                assert_ne!(*value, to);
                *value = to;
                return Some(first_frame_idx);
            }
            Operation::Insert { line_idx, ref line } => {
                let line = hltas::read::line(line)
                    .expect("line should be parse-able")
                    .1;

                hltas.lines.insert(line_idx, line);

                let first_frame_idx = line_first_frame_idx(hltas)
                    .nth(line_idx)
                    .expect("invalid line index");

                return Some(first_frame_idx);
            }
            Operation::SetLeftRightCount { bulk_idx, from, to } => {
                let (bulk, first_frame_idx) = bulk_and_first_frame_idx(hltas)
                    .nth(bulk_idx)
                    .expect("invalid bulk index");

                let count = bulk
                    .left_right_count_mut()
                    .expect("frame bulk should have left-right count");
                assert_eq!(count.get(), from, "wrong current left-right count");

                if from != to {
                    *count = NonZeroU32::new(to).expect("invalid new left-right count");
                    return Some(first_frame_idx);
                }
            }
        }

        None
    }

    /// Undoes operation on HLTAS and returns index of first affected frame.
    ///
    /// Returns `None` if all frames remain valid.
    pub fn undo(&self, hltas: &mut HLTAS) -> Option<usize> {
        match *self {
            Operation::SetFrameCount { bulk_idx, from, to } => {
                let (bulk, first_frame_idx) = bulk_and_first_frame_idx(hltas)
                    .nth(bulk_idx)
                    .expect("invalid bulk index");

                assert_eq!(bulk.frame_count.get(), to, "wrong current frame count");

                if from != to {
                    bulk.frame_count = NonZeroU32::new(from).expect("invalid original frame count");
                    return Some(first_frame_idx + min(from, to) as usize);
                }
            }
            Operation::SetYaw { bulk_idx, from, to } => {
                let (bulk, first_frame_idx) = bulk_and_first_frame_idx(hltas)
                    .nth(bulk_idx)
                    .expect("invalid bulk index");

                let yaw = bulk.yaw_mut().expect("frame bulk should have yaw");
                assert_eq!(*yaw, to, "wrong current yaw");

                if *yaw != from {
                    *yaw = from;
                    return Some(first_frame_idx);
                }
            }
            Operation::Delete { line_idx, ref line } => {
                let line = hltas::read::line(line)
                    .expect("line should be parse-able")
                    .1;

                hltas.lines.insert(line_idx, line);

                let first_frame_idx = line_first_frame_idx(hltas)
                    .nth(line_idx)
                    .expect("invalid line index");

                return Some(first_frame_idx);
            }
            Operation::Split { frame_idx } => {
                let (line_idx, repeat) = line_idx_and_repeat_at_frame(&hltas.lines, frame_idx)
                    .expect("invalid frame index");

                assert_eq!(repeat, 0, "current repeat should be 0");
                assert!(line_idx > 0, "line index should be above 0");

                let prev_bulk = match hltas.lines.remove(line_idx - 1) {
                    Line::FrameBulk(prev_bulk) => prev_bulk,
                    _ => panic!("previous line should be frame bulk"),
                };
                let bulk = hltas.lines[line_idx - 1].frame_bulk_mut().unwrap();
                bulk.frame_count = bulk
                    .frame_count
                    .checked_add(prev_bulk.frame_count.get())
                    .expect("combined frame count should fit");

                // Merging equal frame bulks (undoing a split) does not invalidate any frames.
            }
            Operation::Replace {
                line_idx, ref from, ..
            } => {
                let from = hltas::read::line(from)
                    .expect("line should be parse-able")
                    .1;

                let first_frame_idx = line_first_frame_idx(hltas)
                    .nth(line_idx)
                    .expect("invalid line index");

                hltas.lines[line_idx] = from;
                return Some(first_frame_idx);
            }
            Operation::ToggleKey { bulk_idx, key, to } => {
                let (bulk, first_frame_idx) = bulk_and_first_frame_idx(hltas)
                    .nth(bulk_idx)
                    .expect("invalid bulk index");

                let value = key.value_mut(bulk);
                assert_eq!(*value, to);
                *value = !to;
                return Some(first_frame_idx);
            }
            Operation::Insert { line_idx, .. } => {
                let first_frame_idx = line_first_frame_idx(hltas)
                    .nth(line_idx)
                    .expect("invalid line index");

                hltas.lines.remove(line_idx);
                return Some(first_frame_idx);
            }
            Operation::SetLeftRightCount { bulk_idx, from, to } => {
                let (bulk, first_frame_idx) = bulk_and_first_frame_idx(hltas)
                    .nth(bulk_idx)
                    .expect("invalid bulk index");

                let count = bulk
                    .left_right_count_mut()
                    .expect("frame bulk should have left-right count");
                assert_eq!(count.get(), to, "wrong current left-right count");

                if from != to {
                    *count = NonZeroU32::new(from).expect("invalid original left-right count");
                    return Some(first_frame_idx);
                }
            }
        }

        None
    }
}

impl Key {
    pub fn value_mut(self, bulk: &mut FrameBulk) -> &mut bool {
        match self {
            Key::Forward => &mut bulk.movement_keys.forward,
            Key::Left => &mut bulk.movement_keys.left,
            Key::Right => &mut bulk.movement_keys.right,
            Key::Back => &mut bulk.movement_keys.back,
            Key::Up => &mut bulk.movement_keys.up,
            Key::Down => &mut bulk.movement_keys.down,

            Key::Jump => &mut bulk.action_keys.jump,
            Key::Duck => &mut bulk.action_keys.duck,
            Key::Use => &mut bulk.action_keys.use_,
            Key::Attack1 => &mut bulk.action_keys.attack_1,
            Key::Attack2 => &mut bulk.action_keys.attack_2,
            Key::Reload => &mut bulk.action_keys.reload,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn check_op(input: &str, op: Operation, output: &str) {
        let header = "version 1\nframes\n";
        let input = HLTAS::from_str(&(header.to_string() + input)).unwrap();
        let output = HLTAS::from_str(&(header.to_string() + output)).unwrap();

        let mut modified = input.clone();
        assert_ne!(
            op.apply(&mut modified),
            Some(0),
            "initial frame should never be invalidated"
        );
        assert_eq!(modified, output, "apply produced wrong result");

        assert_ne!(
            op.undo(&mut modified),
            Some(0),
            "initial frame should never be invalidated"
        );
        assert_eq!(modified, input, "undo produced wrong result");
    }

    #[test]
    fn op_set_yaw() {
        check_op(
            "----------|------|------|0.004|10|-|6",
            Operation::SetYaw {
                bulk_idx: 0,
                from: 10.,
                to: 15.,
            },
            "----------|------|------|0.004|15|-|6",
        );
    }

    #[test]
    fn op_set_frame_count() {
        check_op(
            "----------|------|------|0.004|10|-|6",
            Operation::SetFrameCount {
                bulk_idx: 0,
                from: 6,
                to: 10,
            },
            "----------|------|------|0.004|10|-|10",
        );
    }

    #[test]
    fn op_set_left_right_count() {
        check_op(
            "s06-------|------|------|0.004|10|-|6",
            Operation::SetLeftRightCount {
                bulk_idx: 0,
                from: 10,
                to: 20,
            },
            "s06-------|------|------|0.004|20|-|6",
        );
        check_op(
            "s07-------|------|------|0.004|10|-|6",
            Operation::SetLeftRightCount {
                bulk_idx: 0,
                from: 10,
                to: 20,
            },
            "s07-------|------|------|0.004|20|-|6",
        );
    }

    #[test]
    fn op_split() {
        check_op(
            "----------|------|------|0.004|10|-|6",
            Operation::Split { frame_idx: 4 },
            "----------|------|------|0.004|10|-|4\n\
            ----------|------|------|0.004|10|-|2",
        );
    }

    #[test]
    fn op_delete() {
        check_op(
            "----------|------|------|0.004|10|-|4\n\
            ----------|------|------|0.004|10|-|2",
            Operation::Delete {
                line_idx: 0,
                line: "----------|------|------|0.004|10|-|4".to_string(),
            },
            "----------|------|------|0.004|10|-|2",
        );
    }

    #[test]
    fn op_insert() {
        check_op(
            "----------|------|------|0.004|10|-|2",
            Operation::Insert {
                line_idx: 0,
                line: "----------|------|------|0.004|10|-|4".to_string(),
            },
            "----------|------|------|0.004|10|-|4\n\
            ----------|------|------|0.004|10|-|2",
        );
    }

    #[test]
    fn op_replace() {
        check_op(
            "----------|------|------|0.004|10|-|4",
            Operation::Replace {
                line_idx: 0,
                from: "----------|------|------|0.004|10|-|4".to_string(),
                to: "s03lj-----|------|------|0.001|15|10|2".to_string(),
            },
            "s03lj-----|------|------|0.001|15|10|2",
        );
    }

    #[test]
    fn op_toggle_key() {
        fn check_key(result: &str, key: Key) {
            check_op(
                "----------|------|------|0.004|10|-|4",
                Operation::ToggleKey {
                    bulk_idx: 0,
                    key,
                    to: true,
                },
                &("----------|".to_string() + result + "|0.004|10|-|4"),
            );
        }

        check_key("f-----|------", Key::Forward);
        check_key("-l----|------", Key::Left);
        check_key("--r---|------", Key::Right);
        check_key("---b--|------", Key::Back);
        check_key("----u-|------", Key::Up);
        check_key("-----d|------", Key::Down);
        check_key("------|j-----", Key::Jump);
        check_key("------|-d----", Key::Duck);
        check_key("------|--u---", Key::Use);
        check_key("------|---1--", Key::Attack1);
        check_key("------|----2-", Key::Attack2);
        check_key("------|-----r", Key::Reload);
    }
}
