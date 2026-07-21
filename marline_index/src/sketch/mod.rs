mod feature;

pub use feature::{FixedSketch, Sketch, Sketch2, Sketch3, Sketch6};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SketchError {
    EmptySketch,
    DuplicateElement(u32),
}
