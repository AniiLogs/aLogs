# Text models (built into ocr-helper.exe)

Three PaddleOCR line recognisers read player names, which Windows' English OCR reads badly (Latin names) or not at all
(Korean and Cyrillic names). They are compiled into `ocr-helper.exe` (`include_bytes!`), so nothing is downloaded on a
player's PC. All three are Apache-2.0 (`LICENSE-model.txt`), copyright PaddlePaddle Authors, from PaddlePaddle's own
Hugging Face account.

## Latin: `latin_rec.onnx`, `latin_dict.txt`

- **Model:** `PP-OCRv6_small_rec`, as published in ONNX at https://huggingface.co/PaddlePaddle/PP-OCRv6_small_rec_onnx
  (June 2026). For tract, the input's dimension names were renamed (`DynamicDimension.0` to `D_0`) and the stored
  intermediate and output shapes cleared.
- **Why this one:** chosen on 1 Nov 2026 by reading 346 real player names cut from result, Battle Data and Party
  screens (labelled by hand), exact matches:

  | Reader | Exact | ms per box |
  |---|---|---|
  | PP-OCRv6 small | 90.5% | 9 (onnxruntime; ~120 in tract) |
  | PP-OCRv6 medium | 89.9% | 31 |
  | PP-OCRv5 mobile | 87.9% | 11 |
  | OpenOCR RepSVTR (SVTRv2) | 76.9% | 10 |
  | PP-OCRv6 tiny | 73.7% | 3 |
  | Windows OCR (en-US) | 42.5% | 10 |
  | EasyOCR | 40.5% | 51 |

  Most of its misses were the meter's own result-screen name box cutting names short (now wider). Through the meter,
  with the Korean model and the wider box, 94.5% of 419 names read exactly.

## Korean: `korean_rec.onnx`, `korean_dict.txt`

- **Model:** `korean_PP-OCRv5_mobile_rec`, from https://huggingface.co/PaddlePaddle/korean_PP-OCRv5_mobile_rec.
- **Converted** on 2026-11-01 with paddlepaddle 3.1.1 and paddle2onnx 2.1.0 (opset 13):
  `paddle2onnx.export('inference.json', 'inference.pdiparams', 'korean_rec.onnx', opset_version=13)`, then the same
  renaming and shape clearing for tract.

The meter (`crates/alogs-reader/src/hangul.rs`) checks every Korean letter the model reads against the game's own Korean font,
so a letter the game draws almost like a Latin one (ㄱ and 7, ㄴ and L) comes out right.

## Cyrillic: `cyrillic_rec.onnx`, `cyrillic_dict.txt`

- **Model:** `eslav_PP-OCRv5_mobile_rec` (East Slavic: Russian, Ukrainian, Belarusian letters, with Latin letters and
  digits), from https://huggingface.co/PaddlePaddle/eslav_PP-OCRv5_mobile_rec (`inference.json` 217,712 bytes,
  `inference.pdiparams` 7,811,519 bytes, `inference.yml` 4,538 bytes).
- **Converted** on 2026-11-21 as the Korean one, with paddlepaddle 3.1.1, paddle2onnx 2.1.0 and onnx 1.17.0 (opset 13):
  `paddle2onnx.export('inference.json', 'inference.pdiparams', 'cyrillic_rec.onnx', opset_version=13)`, then the same
  renaming and shape clearing for tract. 7.9 MB; it adds 7.9 MB to `ocr-helper.exe`.
- **Dictionary:** 517 characters, 519 output classes with the blank and the space.

The meter (`crates/alogs-reader/src/cyrillic.rs`) takes its read only when a letter no Latin one looks like (Д, Ж, Я,
и...) fits the game's own font (UI_Font_Europe, which draws Russian's 66 letters) clearly better than every Latin letter
and digit; a name made only of look-alikes (А В Е К М Н О Р С Т Х...) stays as the Latin model reads it. The model sees
the name box tidied as for the Korean one, with specks up to 3 px gone instead of 5 (5 thinned я into ч). On the
meter's saved pictures it runs in about 37 ms per name box (median, tract, warm), and only on name boxes.

## Dictionaries

`PostProcess.character_dict` from each model's `inference.yml`, one character per line. A model's output class 0 is
CTC's blank, classes 1 to N are these lines, and the last class is a space.

Numbers (scores, stars, Battle Data values) aren't read by any of these: the meter matches them digit by digit against
the game's own font (`crates/alogs-reader/src/names.rs`, `NameReader::number`), which read all of them right where OCR slipped.
