"""Tests skill knowledge of Stellar RPC (preferred) vs Horizon (legacy).

The skill explicitly notes that Stellar RPC is the preferred API and
Horizon is the legacy path. A model without the skill might suggest
Horizon (it's been around longer and more documented historically).
"""

PROMPT = """I'm building a wallet UI that needs to:

  1. Submit transactions
  2. Watch a specific account for incoming payments
  3. Look up the latest sequence number for an account

Which Stellar API should I use? Be specific — don't just say "use the
Stellar SDK." Name the API endpoint(s)."""

SCORERS = [
    ("mentions_rpc",                lambda t: "stellar rpc" in t.lower() or "soroban-rpc" in t.lower() or "soroban rpc" in t.lower() or "rpc" in t.lower()),
    ("mentions_horizon",            lambda t: "horizon" in t.lower()),
    ("flags_horizon_legacy",        lambda t: ("legacy" in t.lower() or "deprecat" in t.lower() or "preferred" in t.lower())
                                              and ("rpc" in t.lower() or "horizon" in t.lower())),
    ("mentions_send_transaction",   lambda t: "sendtransaction" in t.lower() or "send_transaction" in t.lower()
                                              or "submit transaction" in t.lower() or "submittransaction" in t.lower()),
    ("mentions_get_account",        lambda t: "getaccount" in t.lower() or "get_account" in t.lower() or "/accounts/" in t.lower()),
    ("recommends_rpc_for_modern_apps",
        lambda t: ("rpc" in t.lower() and "soroban" in t.lower())
                  or ("rpc" in t.lower() and ("recommend" in t.lower() or "preferred" in t.lower() or "modern" in t.lower()))),
]
