//! spec 28 Phase C-2 の doc comment 抽出器は **2026-09-06 に harness へ移設**した
//! ([`harness::docs`]、spec 29 Phase B — `play spec-vocab` が同じ表を使う)。ここは薄い包み。
//! テスト 2 本も harness 側へ移った。

pub use harness::docs::{field_doc, variant_doc, variant_field_doc};
