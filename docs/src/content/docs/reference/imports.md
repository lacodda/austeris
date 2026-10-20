---
title: Imports
description: Receipts read into the books - uploaded, imported from a terminal or dropped in a folder - and exchange slips read from a photo.
---

Served by the gateway under `/api/v1/imports`, and, like everything but signing
in, only to a caller with a session. A module: on unless the installation
switches it off (`AUSTERIS_IMPORTS=off`), and then its paths answer `404` with
the code `module.switched_off` and the web interface does not offer it.

It reads documents and posts what they say to the ledger through the ledger's
own contract, signing each entry `imports` - so an entry's history says it was
read from a receipt, not typed (ADR 0014).

## What is read

An electronic document issued through **SIFEN**, Paraguay's tax authority - the
XML file a shop e-mails or offers for download, version 150 of its format. An
invoice, a self-issued invoice and a debit note are money paid; a **credit
note** is money given back. A delivery note records no payment and is refused,
saying so. A photo or a PDF of a receipt is not read: attach it to its entry
instead ([Files](/austeris/reference/ledger/#files)).

## What a receipt becomes

One entry, on the day the receipt was issued:

| Line | Amount | Note |
| --- | --- | --- |
| the account it was paid from | what was paid, out | |
| the category, once per item | what the item came to, after its own discount | the item as printed, `× 2` when more than one |
| the category | what the till rounded off or took off the whole receipt | `rounding and discounts` |

The account moves by what was actually paid; the item lines add up to what the
items came to; the last line, when there is one, is the difference - so the
entry balances to the guaraní and every item can be checked against the paper.
A credit note is the same with the money coming back.

The **shop** is the counterparty: the name it trades under, or its legal name
without the legal form (`SOL ALIMENTOS S.A.` is *Sol Alimentos*). It is found by
its **tax number** (the RUC, `80099999-1`) before its name, and a shop typed by
hand gains the tax number from its first receipt. The **place** is the city the
receipt was issued in, matched to yours however its accents were typed. The
receipt's **file** is kept on the entry, in the same transaction.

The entry's key is the receipt's own code (its CDC), so **one receipt is one
entry** - the same file twice, or the same receipt sent again as another file,
is answered with what it already became.

## Which account, which category

Never guessed:

- **The account** is the one you name - on the upload, with `--from`, or by the
  folder the file was dropped in. Otherwise, a receipt paid in cash goes to your
  only open cash account in its currency. A card payment does not say which of
  your cards, so it is asked.
- **The category** is the one you name, or the one the shop is usually filed
  under. The one you name for a shop that has none becomes its usual one, so its
  next receipt goes straight in.

A receipt **paid from an account in another currency** - a dollar card in a
guaraní shop - is booked **held** at the day's rate: what the bank actually took
is said when the hold is posted
([Held payments](/austeris/reference/ledger/#held-payments)). With no rate known
for that day, it waits.

## Waiting

A receipt that cannot be booked yet is kept, with its file, until it can.
`needs` says what it waits for - `account`, `category`, or both - and `reason`,
when there is more to it, why:

| Code | Waits because |
| --- | --- |
| `document.unknown_account` | the folder is named after an account you do not have (`name`) |
| `document.closed_account` | the account named is closed (`name`) |
| `document.no_rate` | the account is in another currency and no rate is known (`from`, `to`, `day`) |
| `document.ledger_unavailable` | the books could not be reached; nothing is lost, book it again |
| `document.refused_by_ledger` | the books refused the entry (`message`, their sentence) |

Say what it waits for on the [Import screen](/austeris/reference/web/#import),
with `POST .../book`, or by importing the same file again with what was
missing.

## Refused

A file that is not a receipt is recorded as refused, without its file:

| Code | Refused because |
| --- | --- |
| `document.unsupported` | not XML - a photo or a PDF (`media_type`) |
| `document.too_large` | more than 10 MB (`limit_mb`) |
| `document.unreadable` | XML that does not parse, or not text (`why`) |
| `document.not_a_receipt` | XML, but not a SIFEN document |
| `document.several` | several documents in one file (`count`) |
| `document.not_a_payment` | a document that records no payment (`kind`) |
| `document.incomplete` | something every receipt has is missing or malformed (`what`) |

## Endpoints

### `POST /api/v1/imports/documents?name=factura.xml`

The body is the file, as it is. `account` and `category` (ids) say what the
receipt does not. Answers `201` with the document read now, `200` with the one
it already was:

```json
{ "already": false,
  "document": { "id": "…", "name": "factura.xml", "origin": "upload",
                "state": "waiting", "needs": ["category"], "shop": "Supermercado Sol",
                "receipt": { "issuer": { "name": "SOL ALIMENTOS S.A.",
                                         "trade_name": "SUPERMERCADO SOL",
                                         "tax_id": "80099999-1" },
                             "issued_at": "2026-10-18T16:12:43",
                             "currency": "PYG", "total": "32950", "items": [ … ] } } }
```

`state` is `booked` (with `entry_id`), `waiting` (with `needs` and maybe
`reason`) or `refused` (with `reason`). `shop` is the name the shop is filed
under in the books. A reason carries a `code`, its `params`
and an English `message`.

### `GET /api/v1/imports/documents`

The newest first; `?state=waiting` for what waits. Fifty at a time
(`limit`, `offset`), two hundred at most.

### `GET /api/v1/imports/documents/{id}`

### `POST /api/v1/imports/documents/{id}/book`

```json
{ "account_id": "…", "category_id": "…" }
```

What the document waited for; what is left out stays as said before. Answers
the document - booked, or still waiting and why.

### `DELETE /api/v1/imports/documents/{id}`

Takes it off the list, with its file. An entry it became stays in the books.

### `POST /api/v1/imports/slips`

The body is a photo of an exchange office's slip - JPEG, PNG or WebP. Read on
this machine by Tesseract; nothing is kept:

```json
{ "amounts": [{ "amount": "100", "currency": "USD" },
              { "amount": "735000", "currency": "PYG" }],
  "rate": { "base": "USD", "quote": "PYG", "rate": "7350", "stated": true },
  "text": "CAMBIOS EJEMPLO S.A.\nUSD 100,00\nCOTIZ. 7.350\nTOTAL Gs. 735.000" }
```

One amount per currency - the largest, which on a slip is the one that changed
hands. The rate is the one the slip states (`COTIZ.`, `T/C`, `1 USD = 7.350`),
or the one its two amounts imply (`stated: false`); a currency it cannot place
is left out rather than guessed. `text` is what the photo was read as, for
checking. `415` for another kind of file, `503` with `slip.ocr_unavailable` on
an installation without Tesseract.

## The folder

With `AUSTERIS_INBOX` set - the compose files mount `./inbox` there - receipts
dropped in a folder are in the books within half a minute:

```text
inbox/
  owner@example.com/          yours: named by the address you sign in with
    factura.xml               paid from wherever the receipt says, or waiting
    Visa Debit/               one of your accounts, by name
      factura.xml             paid from that account
    done/                     taken in: booked, or waiting on the Import screen
    refused/                  not a receipt
```

A file is taken once it has stood still for a few seconds, so one still being
copied is not read half-written; hidden files and the names a download uses
while it writes (`.part`, `.crdownload`) are left alone. A folder named after
nobody is left as it is, and said once in the log. Taking a file in twice
answers it with what it already is, so a folder kept in step with another
machine - receipts saved from e-mail on a desktop - can be copied over in full.

On Linux the folder must be writable by the container's user, uid 10001, which
moves every file it has read:

```console
$ mkdir -p inbox/owner@example.com && sudo chown -R 10001 inbox
```

## From a terminal

```console
$ austeris import Downloads/factura.xml
factura.xml: booked - Supermercado Sol, 32950 PYG on 2026-10-18

$ austeris import Receipts/
farmacia.xml: waiting - Farmacia Central, 58000 PYG on 2026-10-19; say which account paid (--from) and what it was for (--for)
pedido.xml: not read into the books - this XML document is not an electronic document issued through SIFEN
1 booked, 1 waiting, 1 not read

$ austeris import Receipts/farmacia.xml --from "Visa Debit" --for health
farmacia.xml: in the books - Farmacia Central, 58000 PYG on 2026-10-19
```

A path is a file, or a folder whose files are each one. `--from` names an
account, `--for` a category - by name, or by path (`living/food`) when two share
it. The same file again is the same document: what it waited for is said, and
it is booked.

## Which modules run

### `GET /api/v1/modules`

```json
{ "modules": [{ "name": "imports", "on": true }] }
```

Served by the gateway itself, without a session: it describes the
installation, never anyone's books.
