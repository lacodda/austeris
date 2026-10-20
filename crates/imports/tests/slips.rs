//! An exchange office's slip read from a photo, by the OCR this installation
//! carries.
//!
//! Without Tesseract on the `PATH` these skip themselves, and the CI job that
//! installs it fails when they do: the image ships Tesseract, so the photo a
//! person takes is read by the same program these tests run.

use austeris_imports::slip::{self, Ocr};
use rust_decimal::Decimal;

/// The marker the CI job greps for; changing it means changing that job too.
const SKIP: &str = "skipped: Tesseract is not installed";

#[tokio::test]
async fn a_photo_of_a_slip_reads_as_what_changed_hands_and_the_rate() {
    let Some(ocr) = Ocr::find() else {
        eprintln!("{SKIP}");
        return;
    };
    // A synthetic slip, photographed slightly soft: every name and number on
    // it is made up.
    let photo = std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/slips/exchange-slip.jpg")).expect("reading the photo");

    let text = ocr.read(&photo).await.expect("reading the slip");
    let reading = slip::read_text(&text);

    let amounts: Vec<(Decimal, &str)> = reading.amounts.iter().map(|money| (money.amount, money.currency.as_str())).collect();
    assert_eq!(amounts, vec![(Decimal::from(100), "USD"), (Decimal::from(735_000), "PYG")], "read as: {text}");
    let rate = reading.rate.expect("a rate");
    assert_eq!(
        (rate.base.as_deref(), rate.quote.as_deref(), rate.rate),
        (Some("USD"), Some("PYG"), Decimal::from(7350))
    );
    assert!(rate.stated);
}

#[tokio::test]
async fn what_is_not_a_picture_is_a_failure_to_read_rather_than_a_guess() {
    let Some(ocr) = Ocr::find() else {
        eprintln!("{SKIP}");
        return;
    };
    assert!(ocr.read(b"\xFF\xD8\xFF\xE0 not really a jpeg").await.is_err());
}
