---
title: Web interface
description: The screens served at the root of every installation - accounts, entries, exchanges and held payments.
---

Every installation serves its web interface at its own root: open
`http://127.0.0.1:8084` (or wherever the gateway is published) and sign in with
the same email and password the command line uses. The interface is compiled
into the binary, so there is nothing else to install, and it can never be from
another version than the server.

## Accounts

What every account holds today, as its bank would say it, and what that adds up
to in one currency. The total names the currencies it could not convert and
the ones it converted at an old rate, rather than quietly leaving them out.
Pick the currency of the total in the corner; the choice is remembered in this
browser.

**Available** appears when money is held: a card payment the bank has not
posted yet is already gone from what can be spent, not yet from the balance.

**New account** asks for a name, a kind, a currency (`PYG`, `USD` - or a
coin's code, such as `BTC`) and what was in it before. Closing an account takes
it out of the lists of where money can go and keeps its entries; it can be
reopened. An account's name links to its entries.

## Entries

Every movement of money, newest first, with who was paid, where, the tags, and
what it did to each account.

### One line

The field at the top takes the same line as `austeris add`, read by the same
parser on the server:

```text
45000 food lunch @casarica #trip from cash
```

`from` names the account, `@` who was paid, `#` a tag, a currency code and
`@rate` an amount in another currency, and a leading `~` marks it held. What
the line created - a counterparty, a tag - is said when it is recorded, so a
typo is seen at once. See [the ledger reference](/austeris/reference/ledger/)
for the whole grammar.

### New entry

An expense or an income split across categories, or a transfer between two of
your accounts in one currency.

The account's side is never typed: it is the sum of the split, worked out
exactly, so the entry balances by construction. A category or a counterparty
that does not exist yet is typed into its field and created when the entry is
saved - a cancelled form leaves nothing behind. A counterparty with a usual
category fills it in. **Held** records a card payment the bank has not posted.

Money in another currency than the account's is the one line's job, or an
exchange's.

### Exchange

What left one account and what arrived in another, both as the receipt says
them. The form shows the day's official rate while you type; once recorded, it
says the rate the deal was done at, the reference it is measured against, and
what the difference cost - on a line of its own, under the exchange fees
category.

### An entry

Opening an entry shows every line of it. From there its description, day,
counterparty, tags and place can be changed; its lines cannot - an amount that
was wrong is an entry deleted and recorded again, which keeps every balance it
ever produced explainable.

A held entry is **posted** with the day the bank posted it, and, when the bank
took another amount than was held - a hotel's pre-authorisation, a charge in
another currency at the bank's rate - with that amount. A split cannot post for
another amount: there is no saying which of its parts changed.

### Filters

A period, an account, a category, a tag, a counterparty, and whether the bank
has posted it. The filter is in the address, so a filtered list can be
bookmarked, reloaded or sent to the next tab.

## Language, numbers and themes

English and Russian. The first visit follows the browser's languages; a choice
made on the sign-in screen or in the menu is remembered in this browser. Dates
and numbers are written the way the interface's language writes them -
`₲1,250,000` in English, `1 250 000 ₲` in Russian - with each currency's own
digits: guaraníes have no minor unit, dollars and roubles have two, a coin keeps
every digit it has.

The theme follows the operating system: dark or light, with nothing to set.

On a phone the screens are the same, the navigation moves to the bottom, and
the filters open as a sheet.
