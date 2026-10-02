//! Finding faces in a picture. Pixels in, rectangles out: this module knows nothing about
//! the library, the thumbnail cache or the engine, and nothing outside it names `tract`.

use crate::{Error, Result};
use tract_onnx::prelude::*;

/// YuNet, from OpenCV's model zoo (`models/README.md`).
static MODEL: &[u8] = include_bytes!("../../models/face_detection_yunet_2023mar.onnx");

/// The side of the square the model is run at. A face narrower than about 15 px at this
/// size is missed: at 640 a group photo's faces are 14-21 px wide, on the edge, and at 320
/// the 29 people of the spike's test photograph came out as one.
pub const INPUT: usize = 1280;

type Run = dyn Fn(Tensor) -> TractResult<TVec<TValue>> + Send + Sync;

/// The loaded model. Parsing and optimising it takes about 25 ms, so a pass makes one and
/// shares it between its workers.
pub struct Detector {
    // Unread until the detection method lands in the next task.
    #[allow(dead_code)]
    run: Box<Run>,
}

fn model_error(err: impl std::fmt::Display) -> Error {
    Error::FaceModel(err.to_string())
}

impl Detector {
    pub fn new() -> Result<Self> {
        // The file declares a 640 input and the shapes that follow from it; left in, tract
        // refuses any other size ("Impossible to unify 320 with 160").
        let model = tract_onnx::onnx()
            .with_ignore_output_shapes(true)
            .with_ignore_value_info(true)
            .model_for_read(&mut std::io::Cursor::new(MODEL))
            .map_err(model_error)?
            .with_input_fact(0, f32::fact([1, 3, INPUT, INPUT]).into())
            .map_err(model_error)?
            .into_optimized()
            .map_err(model_error)?
            .into_runnable()
            .map_err(model_error)?;
        Ok(Self {
            run: Box::new(move |input| model.run(tvec!(input.into()))),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bundled model parses and optimises at the input size the pass uses. This is the
    /// test CI runs on Windows and macOS to show `tract` builds and runs there at all.
    #[test]
    fn the_bundled_model_loads() {
        Detector::new().unwrap();
    }
}
