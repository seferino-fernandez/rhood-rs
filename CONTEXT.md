# rhood

A Rust workspace exposing the Robinhood brokerage API as a library, a CLI, and an MCP
server. The MCP server lets a language model read market and account data and place real
trades, so most of the language below exists to keep "the model proposed a trade" and
"a human authorised a trade" from ever collapsing into the same idea.

## Language

### Orders

**Staged Order**:
An order the server has validated and described back to the user, but which no human has
authorised and which the brokerage has never seen.
_Avoid_: pending order, draft order

**Submitted Order**:
An order the brokerage has accepted. It carries a brokerage-assigned order ID and exists
independently of this server.
_Avoid_: placed order, live order, executed order (execution is a fill, not a submission)

**Confirmation**:
The human authorisation that turns a Staged Order into a Submitted Order. It is a safety
boundary, not a convenience step — the model may propose, but only a human may authorise.
_Avoid_: approval (reserved for OAuth consent), review

**Fill**:
The brokerage matching a Submitted Order against the market, in whole or in part. Distinct
from submission: an order can be submitted and never fill.

### Authorisation

**Scope**:
A permission carried by an access token, granted per client at consent time. `read` and
`write` are separate grants and the distinction is an enforced boundary, not a label.
_Avoid_: permission, role

**Write Tool**:
A tool that can alter brokerage state — staging, confirming, or cancelling an order, or
mutating a watchlist or recurring investment. Everything else is a read tool.
_Avoid_: mutating tool, dangerous tool

**Read-Only Mode**:
A process-wide startup posture in which no Write Tool is listed, resolvable, or callable.
It is a deployment choice, distinct from a token's Scope, and the two constrain
independently.
_Avoid_: safe mode, dry run

**Consent**:
The account owner's browser-mediated decision to let a specific registered client obtain
tokens. Distinct from Confirmation: Consent grants a client standing authority, whereas
Confirmation authorises one specific trade.
_Avoid_: authorization (ambiguous between the OAuth step and the general concept)

**Principal**:
The identity a request is attributed to, which is the Registered Client — not the account
owner, who is a constant here, and not the access token, which rotates. Staged Order
ownership and Scope both bind to the Principal.
_Avoid_: user, caller, identity

**Registered Client**:
An application the account owner has entered into configuration, granting it a client ID.
Registration is an administrative act, not a self-service one — there is no runtime
registration endpoint.
_Avoid_: app, integration, consumer
