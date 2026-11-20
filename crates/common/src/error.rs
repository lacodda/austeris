//! One error type for every HTTP surface in the workspace.
//!
//! A handler returns [`AppResult`]; whatever goes wrong becomes an [`AppError`]
//! carrying a status, and axum renders it as the same JSON body everywhere. The
//! message reaching the client is deliberately the one the constructor was
//! given: no error the client cannot act on is spelled out to it.
//!
//! Every error carries a [`Code`] and the facts its message was written from,
//! so a client can offer "record it again" or say it in the reader's language. The message stays for the command line and the
//! logs; the code is what a screen decides by.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

/// What a handler returns.
pub type AppResult<T> = Result<T, AppError>;

/// An error on its way to a client, with the status it should arrive as and
/// the [`Code`] a client decides by.
#[derive(Debug)]
pub struct AppError {
    status: StatusCode,
    source: anyhow::Error,
    code: Code,
    params: serde_json::Value,
}

impl AppError {
    fn build(status: StatusCode, code: Code, source: impl Into<anyhow::Error>) -> Self {
        Self {
            status,
            source: source.into(),
            code,
            params: serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    /// 500 - the service failed at something the client did not ask for wrongly.
    /// It answers [`Code::Internal`]: the client did nothing it could mend.
    pub fn internal(source: impl Into<anyhow::Error>) -> Self {
        Self::build(StatusCode::INTERNAL_SERVER_ERROR, Code::Internal, source)
    }

    /// 400 - the request itself is wrong.
    pub fn bad_request(code: Code, source: impl Into<anyhow::Error>) -> Self {
        Self::build(StatusCode::BAD_REQUEST, code, source)
    }

    /// 404 - the thing addressed does not exist.
    pub fn not_found(code: Code, source: impl Into<anyhow::Error>) -> Self {
        Self::build(StatusCode::NOT_FOUND, code, source)
    }

    /// 409 - the request is fine, but what is there says no to it.
    pub fn conflict(code: Code, source: impl Into<anyhow::Error>) -> Self {
        Self::build(StatusCode::CONFLICT, code, source)
    }

    /// 503 - the service is up but something it depends on is not.
    pub fn unavailable(code: Code, source: impl Into<anyhow::Error>) -> Self {
        Self::build(StatusCode::SERVICE_UNAVAILABLE, code, source)
    }

    /// Any other client-side refusal - 401, 413, 415, 429, 502.
    pub fn refused(status: StatusCode, code: Code, source: impl Into<anyhow::Error>) -> Self {
        Self::build(status, code, source)
    }

    /// The same error, with the facts its message was written from - the names
    /// and numbers a translation of it needs. An object, by name.
    #[must_use]
    pub fn with_params(mut self, params: serde_json::Value) -> Self {
        self.params = params;
        self
    }

    /// The status this will be rendered with.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// The code this carries.
    #[must_use]
    pub fn code(&self) -> Code {
        self.code
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.source()
    }
}

/// An unclassified error is a server fault: a handler that propagates one with
/// `?` gets a 500, not a misleading 400. A blanket impl over `Into<anyhow::Error>`
/// would collide with the standard `From<T> for T`, so the sources handlers
/// actually propagate are listed instead.
impl From<anyhow::Error> for AppError {
    fn from(source: anyhow::Error) -> Self {
        Self::internal(source)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(source: sqlx::Error) -> Self {
        Self::internal(source)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        // Server faults carry a stack of context worth keeping; client mistakes
        // are the client's to fix and only clutter the log at that level.
        if self.status.is_server_error() {
            tracing::error!(status = %self.status, error = ?self.source, "request failed");
        } else {
            tracing::debug!(status = %self.status, error = %self.source, "request rejected");
        }

        let body = ErrorBody {
            status: self.status.as_u16(),
            error: self.status.canonical_reason().unwrap_or("Error").to_owned(),
            message: self.source.to_string(),
            code: self.code.as_str().to_owned(),
            params: self.params,
        };
        (self.status, Json(body)).into_response()
    }
}

/// The JSON every failing endpoint in austeris answers with.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ErrorBody {
    /// The HTTP status, repeated in the body so a logged payload is self-contained.
    pub status: u16,
    /// The status' canonical name, e.g. `Not Found`.
    pub error: String,
    /// What went wrong, in the words the constructor was given.
    pub message: String,
    /// What went wrong, as a stable identifier a client can act on and
    /// translate: `entry.possible_duplicate`. Always there; a client meeting
    /// a code it does not know falls back on `message`.
    pub code: String,
    /// The facts `message` was written from, by name, for a client writing
    /// it in another language. An object, empty when there are none.
    #[schema(value_type = Object)]
    pub params: serde_json::Value,
}

/// Declares the codes and the list of them in one place, so a code that is
/// answered but missing from [`Code::ALL`] cannot be written.
macro_rules! codes {
    ($($(#[$doc:meta])* $variant:ident = $text:literal,)*) => {
        /// What went wrong, as a client decides by it.
        ///
        /// Every code is in [`Code::ALL`], and a test holds the web interface
        /// to having a sentence for each in every language it speaks.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Code {
            $($(#[$doc])* $variant,)*
        }

        impl Code {
            /// Every code a service can answer with.
            pub const ALL: &[Self] = &[$(Self::$variant,)*];

            /// The code as it travels: `entry.possible_duplicate`.
            #[must_use]
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)*
                }
            }
        }
    };
}

codes! {
    /// An entry the same as this one was recorded a moment ago - a second
    /// press of the same button, most likely. Params: `entry_id`, `seconds`.
    PossibleDuplicate = "entry.possible_duplicate",
    /// The entry is kept by a module - a trade's money - and its day,
    /// description and money are changed there, and it is deleted there.
    /// Params: `module`.
    EntryKept = "entry.kept",

    // A typed line that cannot be read. These are what a screen shows while
    // the line is still being typed, so they are the ones most worth saying
    // in the reader's language.
    /// Nothing but spaces.
    LineEmpty = "line.empty",
    /// It does not start with an amount. Params: `word`.
    LineNoAmount = "line.no_amount",
    /// An amount of nothing.
    LineZeroAmount = "line.zero_amount",
    /// An amount and nothing it was for. Params: `amount`.
    LineNoCategory = "line.no_category",
    /// `from` or `to` with nothing after it. Params: `word`.
    LineDanglingAccount = "line.dangling_account",
    /// `@` followed by a number that is not one. Params: `word`.
    LineBadRate = "line.bad_rate",
    /// `@` followed by nothing.
    LineBareAt = "line.bare_at",
    /// A second rate or a second counterparty. Params: `word`.
    LineTwice = "line.twice",
    /// Spent on a category of money coming in. Params: `category`, `amount`.
    LineIncomeCategory = "line.income_category",
    /// Earned on a category of money going out. Params: `category`.
    LineExpenseCategory = "line.expense_category",
    /// No account by the name typed. Params: `name`.
    LineUnknownAccount = "line.unknown_account",
    /// No open account at all.
    LineNoOpenAccount = "line.no_open_account",
    /// Several open accounts and none named. Params: `example`, `accounts`.
    LineWhichAccount = "line.which_account",
    /// A counterparty named with no usual category, and no category typed.
    /// Params: `counterparty`, `amount`, `key`.
    LineNoUsualCategory = "line.no_usual_category",
    /// A counterparty never named before, and no category to give it.
    /// Params: `name`, `amount`.
    LineUnknownCounterparty = "line.unknown_counterparty",
    /// No category by the name typed. Params: `name`.
    LineUnknownCategory = "line.unknown_category",
    /// A name several categories have. Params: `name`, `names`.
    LineAmbiguousCategory = "line.ambiguous_category",
    /// A rate stated for an amount already in the account's currency.
    /// Params: `currency`.
    LineRateWithoutCurrency = "line.rate_without_currency",
    /// An amount in a currency no rate is known for. Params: `from`, `to`,
    /// `day`.
    LineNoRate = "line.no_rate",
    /// An amount too small to be worth a unit of the account's currency.
    /// Params: `amount`, `currency`, `account_currency`.
    LineWorthless = "line.worthless",

    /// Another of the person's buttons says the same.
    TemplateNameTaken = "template.name_taken",

    /// A file larger than a file kept with an entry can be. Params:
    /// `limit_mb`.
    AttachmentTooLarge = "attachment.too_large",
    /// A file that is not a picture, a PDF or an XML document.
    AttachmentUnsupported = "attachment.unsupported",
    /// The same file is on the entry already. Params: `name`, the name it is
    /// on there under, when it is known.
    AttachmentAlready = "attachment.already_attached",
    /// A request larger than the gateway carries. Params: `limit_mb`.
    RequestTooLarge = "request.too_large",

    /// A module the installation has switched off. Params: `module`.
    ModuleSwitchedOff = "module.switched_off",

    // Why a document handed to austeris was refused, or waits. Kept with the
    // document as well as answered, so a screen says them the same way
    // whenever it is asked.
    /// A file that is not a receipt's XML. Params: `media_type`, absent when
    /// the bytes are no kind of document austeris knows.
    DocumentUnsupported = "document.unsupported",
    /// A file larger than one kept can be. Params: `limit_mb`.
    DocumentTooLarge = "document.too_large",
    /// XML that does not parse, or text that is not XML. Params: `why`.
    DocumentUnreadable = "document.unreadable",
    /// An XML document that is not a SIFEN receipt.
    DocumentNotAReceipt = "document.not_a_receipt",
    /// Several receipts in one file. Params: `count`.
    DocumentSeveral = "document.several",
    /// A SIFEN document that records no payment - a delivery note. Params:
    /// `kind`, as the document names itself.
    DocumentNotAPayment = "document.not_a_payment",
    /// A receipt missing something every receipt has, or with it malformed.
    /// Params: `what`.
    DocumentIncomplete = "document.incomplete",
    /// The folder a document arrived in is named after no account of the
    /// person's. Params: `name`.
    DocumentUnknownAccount = "document.unknown_account",
    /// The account chosen is closed. Params: `name`.
    DocumentClosedAccount = "document.closed_account",
    /// The account is in another currency than the receipt, and no rate
    /// between them is known. Params: `from`, `to`, `day`.
    DocumentNoRate = "document.no_rate",
    /// The ledger could not be reached; it is tried again when asked.
    DocumentLedgerUnavailable = "document.ledger_unavailable",
    /// The ledger refused the entry. Params: `message`, its own sentence.
    DocumentRefusedByLedger = "document.refused_by_ledger",

    /// Reading a photo needs OCR, and this installation has none.
    SlipOcrUnavailable = "slip.ocr_unavailable",
    /// A photo OCR cannot read: not a JPEG, PNG or WebP picture.
    SlipUnsupported = "slip.unsupported",

    // Why a trade was not recorded, changed or deleted.
    /// An account the trade names is not the person's.
    TradeUnknownAccount = "trade.unknown_account",
    /// The instrument is not in the market's catalogue.
    TradeUnknownInstrument = "trade.unknown_instrument",
    /// Positions are held on a broker's or a wallet's account. Params:
    /// `name`, `kind`.
    TradeNotAHoldingAccount = "trade.not_a_holding_account",
    /// The account is closed. Params: `name`.
    TradeClosedAccount = "trade.closed_account",
    /// The instrument is money - a currency, or a stablecoin an account is
    /// kept in. Params: `symbol`.
    TradeMoneyNotHolding = "trade.money_not_holding",
    /// The fee is as much as the trade: nothing arrives, or a sale brings in
    /// nothing.
    TradeFeeTooLarge = "trade.fee_too_large",
    /// The money it moves rounds to nothing. Params: `currency`.
    TradeNothingMoved = "trade.nothing_moved",
    /// It leaves a position below nothing. Params: `symbol`, `short`, `at`.
    TradeOversold = "trade.oversold",
    /// Units received with no worth said, and no price the market knows for
    /// the moment they arrived. Params: `symbol`.
    TradeReceiveUnpriced = "trade.receive_unpriced",
    /// The ledger refused the trade's money. Params: `message`, its own
    /// sentence.
    TradeRefusedByLedger = "trade.refused_by_ledger",
    /// The ledger, which keeps the accounts and the money, did not answer.
    PortfolioLedgerUnavailable = "portfolio.ledger_unavailable",
    /// The market, which knows the instruments and their prices, did not
    /// answer.
    PortfolioMarketUnavailable = "portfolio.market_unavailable",

    // A fault on the server's side, which the client did nothing to cause.
    /// Something went wrong that the client could not have mended; the reason
    /// is in the server's log, never in the answer.
    Internal = "server.internal",

    // Who is asking.
    /// No session, or one that is not valid any more.
    AuthSignedOut = "auth.signed_out",
    /// The email or the password is wrong - or there is no such account, which
    /// is deliberately not told apart.
    AuthWrongPassword = "auth.wrong_password",
    /// Too many wrong passwords in a row from this address. Params:
    /// `minutes`, how long the lock lasts.
    AuthLockedOut = "auth.locked_out",

    // What is wrong with a request as such, whatever it asks for.
    /// A body, a query or a path that cannot be read as what the endpoint
    /// takes. Params: `why`, the reader's own words.
    RequestMalformed = "request.malformed",
    /// A body sent without saying it is JSON.
    RequestNotJson = "request.not_json",
    /// No endpoint answers at this address.
    RequestNoSuchEndpoint = "request.no_such_endpoint",
    /// This address asks faster than austeris answers.
    RequestRateLimited = "request.rate_limited",
    /// A window that ends before it starts. Params: `from`, `to`.
    RequestBackwardsWindow = "request.backwards_window",

    /// A module the gateway reaches for did not answer. Params: `module`.
    ModuleNotAnswering = "module.not_answering",

    // Things named in a request that are not there, or not usable.
    /// The currency is not a code. Params: `currency`, as it was written.
    CurrencyNotACode = "currency.not_a_code",
    /// An amount that has to be more than nothing is not.
    AmountNotPositive = "amount.not_positive",
    /// More decimal places than the currency has. Params: `amount`,
    /// `currency`.
    AmountTooPrecise = "amount.too_precise",
    /// Not a number. Params: `text`, as it was written.
    AmountNotANumber = "amount.not_a_number",

    // Accounts and categories.
    /// The person has no such account.
    AccountNotFound = "account.not_found",
    /// An account was sent with no name.
    AccountNameRequired = "account.name_required",
    /// The person has an account by that name.
    AccountNameTaken = "account.name_taken",
    /// The person has no such category.
    CategoryNotFound = "category.not_found",
    /// A category was sent with no name.
    CategoryNameRequired = "category.name_required",
    /// The parent named is not the person's category.
    CategoryParentNotFound = "category.parent_not_found",
    /// A sibling already has the name.
    CategoryNameTaken = "category.name_taken",
    /// The category fees are filed under cannot be made: the person has one
    /// by that name that is money coming in.
    CategoryFeeBlocked = "category.fee_blocked",

    // Counterparties, places and tags.
    /// The person has no such counterparty.
    CounterpartyNotFound = "counterparty.not_found",
    /// A counterparty was sent with no name.
    CounterpartyNameRequired = "counterparty.name_required",
    /// Entries name the counterparty, so it cannot be deleted.
    CounterpartyInUse = "counterparty.in_use",
    /// Not a tax number. Params: `tax_id`, as it was written.
    CounterpartyBadTaxId = "counterparty.bad_tax_id",
    /// Another counterparty has the tax number.
    CounterpartyTaxIdTaken = "counterparty.tax_id_taken",
    /// A counterparty is typed the same way. Params: `name`.
    CounterpartyNameTaken = "counterparty.name_taken",
    /// A name with no letter or digit in it could never be typed. Params:
    /// `name`.
    CounterpartyUntypeable = "counterparty.untypeable",
    /// Not a country code. Params: `country`, as it was written.
    PlaceBadCountry = "place.bad_country",
    /// The person has no such place.
    PlaceNotFound = "place.not_found",
    /// The person has the place already.
    PlaceAlready = "place.already",
    /// Not a tag. Params: `tag`, as it was written.
    TagBadName = "tag.bad_name",
    /// The person has no such tag.
    TagNotFound = "tag.not_found",
    /// A tag was sent with no name.
    TagNameRequired = "tag.name_required",
    /// The person has a tag by that name.
    TagNameTaken = "tag.name_taken",

    // Entries: what is wrong with one being recorded or changed.
    /// The person has no such entry.
    EntryNotFound = "entry.not_found",
    /// The trash holds no such entry.
    EntryNotInTrash = "entry.not_in_trash",
    /// An entry has two sides at least.
    EntryTooFewLines = "entry.too_few_lines",
    /// A line names an account, a category or a conversion, exactly one.
    EntryLineSide = "entry.line_side",
    /// A line of zero records nothing.
    EntryLineZero = "entry.line_zero",
    /// The lines do not sum to nothing in a currency. Params: `currency`,
    /// `sum`.
    EntryUnbalanced = "entry.unbalanced",
    /// A rule of the books the entry breaks. Params: `message`, the rule's own
    /// sentence.
    EntryRuleBroken = "entry.rule_broken",
    /// The entry names a counterparty or a category that is not the person's.
    EntryNotYours = "entry.not_yours",
    /// The entry names an account or a category that does not exist.
    EntryUnknownReference = "entry.unknown_reference",
    /// It moved money on an account or under a category that is gone, so it
    /// cannot come back from the trash as it was.
    EntrySideGone = "entry.side_gone",
    /// The module that posted it has posted it again since.
    EntryPostedAgain = "entry.posted_again",
    /// A payment posts on or after the day it was made. Params: `day`.
    EntryPostsBeforeMade = "entry.posts_before_made",
    /// It was posted already. Params: `day`.
    EntryAlreadyPosted = "entry.already_posted",
    /// An entry moving several accounts cannot post for another amount.
    /// Params: `count`.
    EntryRepostAccounts = "entry.repost_accounts",
    /// A split entry cannot post for another amount.
    EntryRepostSplit = "entry.repost_split",
    /// Only a payment to a category posts for another amount.
    EntryRepostNotPayment = "entry.repost_not_payment",
    /// A conversion's categories are not in one other currency.
    EntryRepostCurrencies = "entry.repost_currencies",
    /// Not one payment: it moves money between accounts.
    EntryBetweenAccounts = "entry.between_accounts",
    /// Not one payment: it moves no account.
    EntryNoAccount = "entry.no_account",
    /// Not one payment: it is split across categories.
    EntrySplit = "entry.split",
    /// Not one payment: it is on no category.
    EntryNoCategory = "entry.no_category",

    // Files kept with an entry.
    /// A file was sent with no name.
    AttachmentNameRequired = "attachment.name_required",
    /// A file with nothing in it.
    AttachmentEmpty = "attachment.empty",
    /// No such file on an entry.
    AttachmentNotFound = "attachment.not_found",
    /// The trash holds no such file.
    AttachmentNotInTrash = "attachment.not_in_trash",
    /// The entry the file was on is in the trash too.
    AttachmentEntryInTrash = "attachment.entry_in_trash",

    // Buttons.
    /// The person has no such button.
    TemplateNotFound = "template.not_found",
    /// A button was sent with no name.
    TemplateNameRequired = "template.name_required",
    /// An order that does not name every button, each once.
    TemplateOrderIncomplete = "template.order_incomplete",
    /// The button asks for the amount each time. Params: `name`.
    TemplateAsksAmount = "template.asks_amount",

    // Exchanges that cannot be planned.
    /// Both amounts have to be more than nothing.
    ExchangeNotPositive = "exchange.not_positive",
    /// Both sides are in one currency. Params: `currency`.
    ExchangeSameCurrency = "exchange.same_currency",
    /// The amount received is worth nothing in the currency given. Params:
    /// `amount`, `currency`.
    ExchangeTooSmall = "exchange.too_small",
    /// The reference rate is for another pair. Params: `base`, `quote`, the
    /// rate's; `given`, `got`, the exchange's.
    ExchangeWrongReference = "exchange.wrong_reference",

    // Rates.
    /// A currency is worth one of itself.
    RateSameCurrency = "rate.same_currency",
    /// A rate is a positive number.
    RateNotPositive = "rate.not_positive",
    /// No rate for the pair is known on or before the day. Params: `from`,
    /// `to`, `day`.
    RateNotFound = "rate.not_found",
    /// No source publishes a rate for a day that has not happened.
    RateFutureDay = "rate.future_day",

    // The market's instruments and prices.
    /// The market has no such instrument. Params: `id`, when it was named by
    /// one in a list.
    InstrumentNotFound = "instrument.not_found",
    /// An instrument was sent with no symbol.
    InstrumentSymbolRequired = "instrument.symbol_required",
    /// Not an instrument's id. Params: `id`, as it was written.
    InstrumentBadId = "instrument.bad_id",
    /// A price is more than nothing.
    PriceNotPositive = "price.not_positive",
    /// A price is of a moment that has happened.
    PriceInFuture = "price.in_future",
    /// Not a source's name. Params: `source`.
    PriceBadSource = "price.bad_source",
    /// A source this installation asks itself. Params: `source`.
    PriceLiveSource = "price.live_source",
    /// No prices were sent.
    PriceNoneGiven = "price.none_given",
    /// More prices in one call than there can be. Params: `count`, `limit`.
    PriceTooMany = "price.too_many",
    /// A source that is switched off. Params: `source`.
    SourceSwitchedOff = "source.switched_off",

    // The portfolio.
    /// The person has no such snapshot.
    SnapshotNotFound = "snapshot.not_found",
    /// A day that has not begun anywhere has no value yet.
    PortfolioFutureDay = "portfolio.future_day",
    /// The person has no such trade.
    TradeNotFound = "trade.not_found",
    /// A trade is of more than nothing.
    TradeNothingTraded = "trade.nothing_traded",
    /// A fee is not negative.
    TradeNegativeFee = "trade.negative_fee",
    /// A buy or a sale has a price of more than nothing.
    TradePriceRequired = "trade.price_required",
    /// A trade other than a transfer happens on one account.
    TradeOneAccount = "trade.one_account",
    /// What a unit was worth is not below nothing.
    TradeNegativeWorth = "trade.negative_worth",
    /// Units received cost nothing to receive; a fee is not theirs.
    TradeReceiveFee = "trade.receive_fee",
    /// A transfer has no price.
    TradeTransferPriced = "trade.transfer_priced",
    /// A transfer goes to another account, and none was named.
    TradeTransferTarget = "trade.transfer_target",
    /// A transfer goes to another account than it left.
    TradeTransferToSelf = "trade.transfer_to_self",
    /// A trade is of a moment that has happened.
    TradeInFuture = "trade.in_future",
    /// An idempotency key is not empty.
    TradeKeyEmpty = "trade.key_empty",
    /// An idempotency key is longer than allowed. Params: `limit`.
    TradeKeyTooLong = "trade.key_too_long",

    // Documents handed to the importer, beside the ones refused above.
    /// A file was sent with no name.
    DocumentNameRequired = "document.name_required",
    /// A file with nothing in it.
    DocumentEmpty = "document.empty",
    /// No such document.
    DocumentNotFound = "document.not_found",
}

#[cfg(test)]
mod tests {
    use super::{AppError, Code};
    use axum::response::IntoResponse;
    use http_body_util::BodyExt;

    #[test]
    fn every_code_is_listed_once_and_reads_as_a_dotted_name() {
        let mut seen: Vec<&str> = Code::ALL.iter().map(|code| code.as_str()).collect();
        let listed = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), listed, "a code is listed twice");
        for code in seen {
            assert!(
                code.split('.').count() == 2 && code.chars().all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
                "`{code}` is not `area.what`"
            );
        }
    }

    #[tokio::test]
    async fn a_coded_error_carries_its_code_and_params_beside_the_message() {
        let error = AppError::bad_request(Code::PossibleDuplicate, anyhow::anyhow!("recorded a moment ago")).with_params(serde_json::json!({ "seconds": 3 }));
        let body = error.into_response().into_body().collect().await.expect("a body").to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["code"], "entry.possible_duplicate");
        assert_eq!(json["params"]["seconds"], 3);
        assert_eq!(json["message"], "recorded a moment ago");
    }

    #[tokio::test]
    async fn an_internal_error_answers_the_server_code_with_no_params() {
        let body = AppError::internal(anyhow::anyhow!("the pool is gone"))
            .into_response()
            .into_body()
            .collect()
            .await
            .expect("a body")
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["code"], "server.internal");
        assert_eq!(json["params"], serde_json::json!({}));
    }
}
