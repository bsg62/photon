# Bundled models

| File | What | Source | Licence |
|---|---|---|---|
| `face_detection_yunet_2023mar.onnx` | YuNet face detector, 232,589 bytes | [opencv_zoo](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet), fetched 2026-10-01 | MIT, Copyright (c) 2020 Shiqi Yu |
| `face_recognition_sface_2021dec.onnx` | SFace face recognition, 38,696,353 bytes | [opencv_zoo](https://github.com/opencv/opencv_zoo/tree/main/models/face_recognition_sface), fetched 2026-10-03 | Apache 2.0 |

Both are embedded in photon-core with `include_bytes!`: YuNet in `src/face_detect/mod.rs`,
SFace in `src/face_embed/mod.rs`. Nothing is downloaded. YuNet's file declares a 640 input;
photon runs it at 1280 and at 320 (`INPUT`, `CLOSE_UP_INPUT`). SFace takes a 112x112 face,
aligned to its five reference points by `src/face_embed/align.rs`. Replacing a file changes what
every library has stored: bump `face_detect::DETECTOR_VERSION` for YuNet's, and
`face_embed::EMBEDDER_VERSION` for SFace's.
