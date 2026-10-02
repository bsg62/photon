# Bundled models

| File | What | Source | Licence |
|---|---|---|---|
| `face_detection_yunet_2023mar.onnx` | YuNet face detector, 232,589 bytes | [opencv_zoo](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet), fetched 2026-10-01 | MIT, Copyright (c) 2020 Shiqi Yu |

Embedded in photon-core with `include_bytes!` (`src/face_detect/mod.rs`). Replacing a file
changes what every library has stored: bump `face_detect::DETECTOR_VERSION`.
