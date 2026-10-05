//! OCR is Syrup's: Tesseract where it is installed, else the engine
//! Windows ships with (`SYRUP_OCR=tesseract|windows` chooses; MapleSyrup
//! also accepts `MS_OCR`, which `maplesyrup` maps to it at start-up).

pub use syrup::ocr::{Engine, OcrResult, OcrWord, engine, is_ocr_available, ocr_region};
