use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Frame {
    pub buttons: u16,
    pub lt: u8,
    pub rt: u8,
    pub lx: i16,
    pub ly: i16,
    pub rx: i16,
    pub ry: i16,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Recording {
    pub fps: u32,
    pub side: Side,
    pub frames: Vec<Frame>,
}