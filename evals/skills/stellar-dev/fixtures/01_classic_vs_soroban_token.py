"""Tests skill knowledge of the Stellar Asset Contract (SAC) bridge.

Generic Claude knows about Stellar classic assets and Soroban tokens
separately, but tends to miss the SAC bridge — the canonical pattern
for "I want a token usable from both classic Stellar Ops and Soroban
contracts." The skill should surface SAC explicitly.
"""

PROMPT = """I'm shipping a fungible token on Stellar. The token needs to:

  1. Be transferable via standard Stellar payments (so wallets like
     Freighter / Lobstr can hold it).
  2. Also be usable from a Soroban smart contract that does
     custody-and-release flows.

Should I issue a classic Stellar Asset, deploy a Soroban token contract,
or both? Walk me through the canonical pattern."""

SCORERS = [
    ("mentions_sac",                 lambda t: "stellar asset contract" in t.lower() or " sac" in t.lower() or "(sac)" in t.lower()),
    ("mentions_classic_asset",       lambda t: "classic" in t.lower() or "stellar asset" in t.lower()),
    ("mentions_trustline",           lambda t: "trustline" in t.lower() or "trust_line" in t.lower() or "change_trust" in t.lower()),
    ("mentions_issuer",              lambda t: "issuer" in t.lower()),
    ("mentions_wrap_bridge",         lambda t: "wrap" in t.lower() or "bridge" in t.lower()),
    ("recommends_classic_with_sac",  lambda t: (
        "classic" in t.lower() and ("sac" in t.lower() or "asset contract" in t.lower())
    )),
]
