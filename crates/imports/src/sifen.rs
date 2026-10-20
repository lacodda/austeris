//! A receipt as Paraguay's tax authority issues it.
//!
//! Every sale in Paraguay is documented to SIFEN, the tax authority's system of
//! electronic documents, and the buyer can have the document itself: an XML
//! file, e-mailed by the shop or downloaded from its page. It says who sold,
//! when, every item and what it came to, and how it was paid - everything an
//! entry needs, in a form nobody has to read off a photo.
//!
//! Reading it is pure: bytes in, a [`Receipt`] or the reason there is none out.
//! What the receipt becomes in the books is decided elsewhere ([`crate::book`]).
//!
//! The format is SIFEN's own (Manual Técnico, version 150): an `rDE` holding a
//! `DE`, the document, in the namespace below. A file arriving inside some
//! other envelope - a batch, a download wrapper - is read the same way, by
//! finding the one `DE` in it.

use std::str::FromStr;

use chrono::{NaiveDate, NaiveDateTime};
use roxmltree::{Document, Node};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// The namespace every SIFEN element is in.
pub const NAMESPACE: &str = "http://ekuatia.set.gov.py/sifen/xsd";

/// A receipt, as its document says it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Receipt {
    /// The document's own code, the CDC: 44 digits that name it among every
    /// document SIFEN holds. Absent from a document that does not carry one.
    pub code: Option<String>,
    /// Whether money was paid, or given back.
    pub kind: Kind,
    /// Its number as printed: establishment, point of sale, document -
    /// `001-001-0000123`.
    pub number: Option<String>,
    /// When it was issued, in the shop's own time.
    pub issued_at: NaiveDateTime,
    /// Who issued it.
    pub issuer: Issuer,
    /// What its amounts are in, as a currency code.
    pub currency: String,
    /// What was bought, as it was printed.
    pub items: Vec<Item>,
    /// What it came to: what was paid, after discounts and rounding.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "32950")]
    pub total: Decimal,
    /// Sold on credit: paid later, not at the till - so the receipt itself
    /// says nothing about which account the money left.
    pub on_credit: bool,
    /// How it was paid, when the document says.
    pub payments: Vec<Payment>,
}

impl Receipt {
    /// The day it was issued.
    #[must_use]
    pub fn issued_on(&self) -> NaiveDate {
        self.issued_at.date()
    }

    /// Paid in cash and nothing else.
    #[must_use]
    pub fn paid_in_cash(&self) -> bool {
        !self.on_credit && !self.payments.is_empty() && self.payments.iter().all(|payment| payment.means == Means::Cash)
    }
}

/// What kind of document a receipt is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Money paid for what is on it: an invoice, or a note adding to one.
    Invoice,
    /// Money given back: a credit note against an earlier invoice.
    CreditNote,
}

/// Who issued a receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Issuer {
    /// Its legal name: `SOL ALIMENTOS S.A.`.
    pub name: String,
    /// The name it trades under, when the document gives one: `SUPERMERCADO
    /// SOL`.
    pub trade_name: Option<String>,
    /// Its RUC with the check digit: `80099999-1`.
    pub tax_id: Option<String>,
    /// The city it issued from, as SIFEN names it: `ASUNCION (DISTRITO)`.
    pub city: Option<String>,
}

/// One item on a receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Item {
    /// As it was printed: `PAN FELIPE X KG`.
    pub description: String,
    /// How many, or how much of it.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "0.534")]
    pub quantity: Decimal,
    /// The unit, as printed: `UNI`, `KG`.
    pub unit: Option<String>,
    /// What it came to, after its own discount.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "6408")]
    pub total: Decimal,
}

/// One way a receipt was paid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Payment {
    /// How.
    pub means: Means,
    /// How much this way.
    #[serde(with = "rust_decimal::serde::str")]
    #[schema(value_type = String, example = "32950")]
    pub amount: Decimal,
    /// In what.
    pub currency: String,
    /// The last four digits of the card, when it was a card and the document
    /// says.
    pub card_digits: Option<String>,
}

/// How a receipt was paid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Means {
    /// Notes and coins.
    Cash,
    /// A credit card.
    CreditCard,
    /// A debit card.
    DebitCard,
    /// A bank transfer.
    Transfer,
    /// A cheque.
    Cheque,
    /// A phone wallet or another electronic payment.
    Wallet,
    /// Anything else SIFEN lists: a voucher, an advance, a barter.
    Other,
}

impl Means {
    /// The means a SIFEN payment code (`iTiPago`) names.
    fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Cash,
            2 => Self::Cheque,
            3 => Self::CreditCard,
            4 => Self::DebitCard,
            5 | 16 => Self::Transfer,
            7 | 17 | 21 => Self::Wallet,
            _ => Self::Other,
        }
    }
}

/// Why bytes are not a receipt that can be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Unread {
    /// Not an XML document, or not one that parses.
    #[error("this is not an XML document that can be read: {0}")]
    NotXml(String),
    /// An XML document, but not a SIFEN one.
    #[error("this XML document is not an electronic document issued through SIFEN")]
    NotSifen,
    /// More than one document in one file.
    #[error("this file holds {0} documents; one receipt per file")]
    Several(usize),
    /// A SIFEN document that moves no money: a delivery note.
    #[error("this is a {0}, which records no payment")]
    NotAPayment(String),
    /// Something every receipt has is missing.
    #[error("the document has no {0}")]
    Missing(&'static str),
    /// Something is there but is not what it should be.
    #[error("the document's {0} is `{1}`, which is not one")]
    Malformed(&'static str, String),
}

/// Reads a receipt from a SIFEN document's bytes.
///
/// # Errors
///
/// Returns [`Unread`] when the bytes are not an XML document, the document is
/// not SIFEN's, holds several documents or one that records no payment, or
/// lacks something every receipt has.
pub fn read(bytes: &[u8]) -> Result<Receipt, Unread> {
    let source = decode(bytes)?;
    // No DTD - roxmltree refuses one by default, and a receipt has none - so
    // nothing in a file dropped in a folder can make the reader fetch or
    // expand anything.
    let document = Document::parse(&source).map_err(|error| Unread::NotXml(error.to_string()))?;

    let found: Vec<Node> = document.descendants().filter(|node| is(node, "DE")).collect();
    let de = match found.as_slice() {
        [] => return Err(Unread::NotSifen),
        [one] => *one,
        several => return Err(Unread::Several(several.len())),
    };

    let kind = match path(de, &["gTimb", "iTiDE"]).map(|code| code.trim().to_owned()).as_deref() {
        // An invoice, an export or import invoice, a self-issued invoice and a
        // debit note all record money paid.
        Some("1" | "2" | "3" | "4" | "6") => Kind::Invoice,
        Some("5") => Kind::CreditNote,
        Some(_) => {
            let said = path(de, &["gTimb", "dDesTiDE"]).unwrap_or("document").trim().to_lowercase();
            return Err(Unread::NotAPayment(said));
        }
        None => return Err(Unread::Missing("document type (iTiDE)")),
    };

    let stamp = path(de, &["gDatGralOpe", "dFeEmiDE"]).ok_or(Unread::Missing("date of issue (dFeEmiDE)"))?;
    let issued_at = NaiveDateTime::parse_from_str(stamp.trim(), "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDate::parse_from_str(stamp.trim(), "%Y-%m-%d").map(|day| day.and_time(chrono::NaiveTime::MIN)))
        .map_err(|_| Unread::Malformed("date of issue", stamp.trim().to_owned()))?;

    let currency = path(de, &["gDatGralOpe", "gOpeCom", "cMoneOpe"]).unwrap_or("PYG").trim().to_uppercase();
    if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(Unread::Malformed("currency", currency));
    }

    let emitter = child(de, "gDatGralOpe")
        .and_then(|general| child(general, "gEmis"))
        .ok_or(Unread::Missing("issuer (gEmis)"))?;
    let name = text(emitter, "dNomEmi").ok_or(Unread::Missing("issuer's name (dNomEmi)"))?;
    let tax_id = match (text(emitter, "dRucEm"), text(emitter, "dDVEmi")) {
        (Some(ruc), Some(check)) => Some(format!("{ruc}-{check}")),
        (Some(ruc), None) => Some(ruc.to_owned()),
        (None, _) => None,
    };
    let issuer = Issuer {
        name: name.to_owned(),
        trade_name: text(emitter, "dNomFanEmi").map(ToOwned::to_owned),
        tax_id,
        city: text(emitter, "dDesCiuEmi").map(ToOwned::to_owned),
    };

    let specific = child(de, "gDtipDE");
    let mut items = Vec::new();
    for item in specific.into_iter().flat_map(|node| node.children().filter(|child| is(child, "gCamItem"))) {
        items.push(read_item(item)?);
    }

    let totals = child(de, "gTotSub");
    let total = totals
        .and_then(|totals| text(totals, "dTotGralOpe").or_else(|| text(totals, "dTotOpe")))
        .ok_or(Unread::Missing("total (dTotGralOpe)"))?;
    let total = amount(total, "total")?.normalize();

    let condition = specific.and_then(|node| child(node, "gCamCond"));
    let on_credit = condition.and_then(|node| text(node, "iCondOpe")) == Some("2");
    let mut payments = Vec::new();
    for paid in condition.into_iter().flat_map(|node| node.children().filter(|child| is(child, "gPaConEIni"))) {
        payments.push(read_payment(paid, &currency)?);
    }

    let number = match (path(de, &["gTimb", "dEst"]), path(de, &["gTimb", "dPunExp"]), path(de, &["gTimb", "dNumDoc"])) {
        (Some(establishment), Some(point), Some(document)) => Some(format!("{}-{}-{}", establishment.trim(), point.trim(), document.trim())),
        _ => None,
    };
    let code = de.attribute("Id").map(str::trim).filter(|id| !id.is_empty()).map(ToOwned::to_owned);

    Ok(Receipt {
        code,
        kind,
        number,
        issued_at,
        issuer,
        currency,
        items,
        total,
        on_credit,
        payments,
    })
}

/// One `gCamItem`.
fn read_item(item: Node) -> Result<Item, Unread> {
    let description = text(item, "dDesProSer").unwrap_or_default().to_owned();
    let quantity = match text(item, "dCantProSer") {
        Some(raw) => amount(raw, "quantity")?,
        None => Decimal::ONE,
    };
    let value = child(item, "gValorItem");
    // What the item came to after its own discounts; the gross amount when the
    // document leaves the net one out.
    let total = value
        .and_then(|value| {
            child(value, "gValorRestaItem")
                .and_then(|rest| text(rest, "dTotOpeItem"))
                .or_else(|| text(value, "dTotBruOpeItem"))
        })
        .ok_or(Unread::Missing("item total (dTotOpeItem)"))?;
    Ok(Item {
        description,
        quantity: quantity.normalize(),
        unit: text(item, "dDesUniMed").map(ToOwned::to_owned),
        total: amount(total, "item total")?.normalize(),
    })
}

/// One `gPaConEIni`.
fn read_payment(paid: Node, currency: &str) -> Result<Payment, Unread> {
    let code = text(paid, "iTiPago").ok_or(Unread::Missing("payment type (iTiPago)"))?;
    let code: u32 = code.parse().map_err(|_| Unread::Malformed("payment type", code.to_owned()))?;
    let amount_paid = text(paid, "dMonTiPag").ok_or(Unread::Missing("amount paid (dMonTiPag)"))?;
    let card_digits = child(paid, "gPagTarCD")
        .and_then(|card| text(card, "dNumTarj"))
        .filter(|digits| digits.len() == 4 && digits.chars().all(|c| c.is_ascii_digit()))
        .map(ToOwned::to_owned);
    Ok(Payment {
        means: Means::from_code(code),
        amount: amount(amount_paid, "amount paid")?.normalize(),
        currency: text(paid, "cMoneTiPag").map_or_else(|| currency.to_owned(), str::to_uppercase),
        card_digits,
    })
}

/// The document's text: UTF-8 as SIFEN writes it, or Latin-1 when the file
/// says so - a shop's own system re-saving the file is not unheard of.
fn decode(bytes: &[u8]) -> Result<String, Unread> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(text.to_owned());
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(100)]).to_lowercase();
    if head.contains("iso-8859-1") || head.contains("latin1") || head.contains("windows-1252") {
        // Each byte is the code point of the same number: that is what
        // Latin-1 is, and the one decoding needing no table.
        Ok(bytes.iter().map(|byte| char::from(*byte)).collect())
    } else {
        Err(Unread::NotXml("its bytes are not UTF-8 text".to_owned()))
    }
}

/// Whether a node is the SIFEN element of this name.
fn is(node: &Node, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name && node.tag_name().namespace() == Some(NAMESPACE)
}

/// A node's first SIFEN child of this name.
fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|child| is(child, name))
}

/// The trimmed, non-empty text of a node's SIFEN child.
fn text<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    child(node, name).and_then(|child| child.text()).map(str::trim).filter(|text| !text.is_empty())
}

/// The text at the end of a path of SIFEN children.
fn path<'a>(node: Node<'a, '_>, names: &[&str]) -> Option<&'a str> {
    let (last, parents) = names.split_last()?;
    let mut at = node;
    for name in parents {
        at = child(at, name)?;
    }
    text(at, last)
}

/// An amount as SIFEN writes one: a plain decimal with a point.
fn amount(raw: &str, what: &'static str) -> Result<Decimal, Unread> {
    Decimal::from_str(raw.trim()).map_err(|_| Unread::Malformed(what, raw.trim().to_owned()))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rust_decimal::Decimal;

    use super::{Kind, Means, Unread, read};

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/sifen").join(name)).expect("reading a fixture")
    }

    fn decimal(digits: &str) -> Decimal {
        Decimal::from_str(digits).expect("a decimal")
    }

    #[test]
    fn an_invoice_says_who_when_what_and_how_it_was_paid() {
        let receipt = read(&fixture("invoice-cash.xml")).expect("reading the invoice");
        assert_eq!(receipt.code.as_deref(), Some("01800999991001001000012322026101810000000011"));
        assert_eq!(receipt.kind, Kind::Invoice);
        assert_eq!(receipt.number.as_deref(), Some("001-001-0000123"));
        assert_eq!(receipt.issued_at.to_string(), "2026-10-18 16:12:43");
        assert_eq!(receipt.issuer.name, "SOL ALIMENTOS S.A.");
        assert_eq!(receipt.issuer.trade_name.as_deref(), Some("SUPERMERCADO SOL"));
        assert_eq!(receipt.issuer.tax_id.as_deref(), Some("80099999-1"));
        assert_eq!(receipt.issuer.city.as_deref(), Some("ASUNCION (DISTRITO)"));
        assert_eq!(receipt.currency, "PYG");
        assert!(!receipt.on_credit);
        assert!(receipt.paid_in_cash());

        // Every item at what it came to after its own discount; the total is
        // what was paid, rounded down by the eight guaranies the till kept.
        let items: Vec<(&str, Decimal, Decimal)> = receipt
            .items
            .iter()
            .map(|item| (item.description.as_str(), item.quantity, item.total))
            .collect();
        assert_eq!(
            items,
            vec![
                ("LECHE ENTERA LARGA VIDA 1LT", decimal("2"), decimal("17000")),
                ("PAN FELIPE X KG", decimal("0.534"), decimal("6408")),
                ("GALLETITAS DULCES 400GR", decimal("1"), decimal("9550")),
            ]
        );
        assert_eq!(receipt.total, decimal("32950"));
    }

    #[test]
    fn a_card_payment_says_which_card_when_the_document_does() {
        let receipt = read(&fixture("invoice-card.xml")).expect("reading the invoice");
        assert_eq!(receipt.issuer.trade_name, None);
        assert_eq!(receipt.payments.len(), 1);
        assert_eq!(receipt.payments[0].means, Means::DebitCard);
        assert_eq!(receipt.payments[0].card_digits.as_deref(), Some("4321"));
        assert!(!receipt.paid_in_cash());
        // `58000.0000` is fifty-eight thousand, said once.
        assert_eq!(receipt.items[0].total.to_string(), "58000");
    }

    #[test]
    fn a_credit_note_is_money_given_back() {
        let receipt = read(&fixture("credit-note.xml")).expect("reading the credit note");
        assert_eq!(receipt.kind, Kind::CreditNote);
        assert_eq!(receipt.total, decimal("9550"));
        // It says nothing about how the money came back.
        assert_eq!(receipt.payments, Vec::new());
        assert!(!receipt.paid_in_cash());
    }

    #[test]
    fn a_receipt_on_credit_is_not_paid_at_the_till() {
        let text = String::from_utf8(fixture("invoice-cash.xml"))
            .expect("text")
            .replace("<iCondOpe>1</iCondOpe>", "<iCondOpe>2</iCondOpe>");
        let receipt = read(text.as_bytes()).expect("reading");
        assert!(receipt.on_credit);
        assert!(!receipt.paid_in_cash(), "money owed was taken for cash paid");
    }

    #[test]
    fn a_delivery_note_moves_no_money() {
        let text = String::from_utf8(fixture("invoice-cash.xml"))
            .expect("text")
            .replace("<iTiDE>1</iTiDE>", "<iTiDE>7</iTiDE>")
            .replace("Factura electrónica", "Nota de remisión electrónica");
        assert_eq!(read(text.as_bytes()), Err(Unread::NotAPayment("nota de remisión electrónica".to_owned())));
    }

    #[test]
    fn what_is_not_a_sifen_document_is_said_to_be_so() {
        assert!(matches!(read(b"%PDF-1.7"), Err(Unread::NotXml(_))));
        assert_eq!(read(b"<?xml version=\"1.0\"?><invoice><total>5</total></invoice>"), Err(Unread::NotSifen));
        // A `DE` outside SIFEN's namespace is someone else's.
        assert_eq!(read(b"<?xml version=\"1.0\"?><rDE><DE/></rDE>"), Err(Unread::NotSifen));
    }

    #[test]
    fn a_file_with_a_dtd_is_refused_rather_than_expanded() {
        // An entity can name a file on the machine reading it; nothing a
        // receipt needs is worth reading one.
        let hostile = b"<?xml version=\"1.0\"?><!DOCTYPE r [<!ENTITY x SYSTEM \"file:///etc/passwd\">]><r>&x;</r>";
        assert!(matches!(read(hostile), Err(Unread::NotXml(_))));
    }

    #[test]
    fn several_documents_in_one_file_are_refused_rather_than_half_read() {
        let one = String::from_utf8(fixture("invoice-card.xml")).expect("text");
        let start = one.find("<DE ").expect("a DE");
        let end = one.find("</DE>").expect("its end") + "</DE>".len();
        let two = one.replacen(&one[start..end], &format!("{0}{0}", &one[start..end]), 1);
        assert_eq!(read(two.as_bytes()), Err(Unread::Several(2)));
    }

    #[test]
    fn what_every_receipt_has_is_required() {
        let text = String::from_utf8(fixture("invoice-card.xml")).expect("text");
        let without_date = text.replace("<dFeEmiDE>2026-10-19T09:30:00</dFeEmiDE>", "");
        assert_eq!(read(without_date.as_bytes()), Err(Unread::Missing("date of issue (dFeEmiDE)")));
        let bad_total = text.replace("<dTotGralOpe>58000</dTotGralOpe>", "<dTotGralOpe>58.000,00</dTotGralOpe>");
        assert_eq!(read(bad_total.as_bytes()), Err(Unread::Malformed("total", "58.000,00".to_owned())));
    }

    #[test]
    fn a_latin1_file_says_so_and_is_read_as_latin1() {
        let text = String::from_utf8(fixture("invoice-card.xml"))
            .expect("text")
            .replace("encoding=\"UTF-8\"", "encoding=\"ISO-8859-1\"")
            .replace("FARMACIA CENTRAL", "FARMACIA SEÑOR");
        let latin1: Vec<u8> = text.chars().map(|c| u8::try_from(u32::from(c)).expect("a Latin-1 character")).collect();
        let receipt = read(&latin1).expect("reading");
        assert_eq!(receipt.issuer.name, "FARMACIA SEÑOR S.R.L.");
    }
}
