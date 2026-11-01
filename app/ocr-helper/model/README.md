# Korean text model (built into ocr-helper.exe)

`korean_rec.onnx` and `korean_dict.txt` let the meter read Korean player names (Hangul), which Windows' English OCR
can't see. They are compiled into `ocr-helper.exe` (`include_bytes!`), so nothing is downloaded on a player's PC.

- **Model:** PaddleOCR `korean_PP-OCRv5_mobile_rec`, from https://huggingface.co/PaddlePaddle/korean_PP-OCRv5_mobile_rec
  (PaddlePaddle's own account). Licence: Apache-2.0, in `LICENSE-model.txt`. Copyright PaddlePaddle Authors.
- **Converted** on 2026-11-01 with paddlepaddle 3.1.1 and paddle2onnx 2.1.0 (opset 13):
  `paddle2onnx.export('inference.json', 'inference.pdiparams', 'korean_rec.onnx', opset_version=13)`. Then, for tract,
  the input's dimension names were renamed (`DynamicDimension.0` to `D_0`, and so on), and the stored intermediate and
  output shapes were cleared (tract can't parse paddle2onnx's shape formulas, and works them out itself).
- **Dictionary:** `PostProcess.character_dict` from the model's `inference.yml`, one character per line. The model's
  output class 0 is CTC's blank, classes 1 to 11945 are these lines, and the last class is a space.

The meter (`bossbar/hangulnames.py`) checks every Korean letter the model reads against the game's own Korean font,
so a letter the game draws almost like a Latin one (ㄱ and 7, ㄴ and L) comes out right.
