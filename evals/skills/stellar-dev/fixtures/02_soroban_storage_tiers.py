"""Tests skill knowledge of Soroban storage tiers + TTL semantics.

Soroban has three storage tiers (instance / persistent / temporary)
with different rent + lifetime semantics. The skill should pick
correct tiers for the use case AND mention TTL extension.
"""

PROMPT = """I'm writing a Soroban contract that tracks per-user token balances and
per-user last-activity timestamps for a 90-day rolling activity window.

Which Soroban storage type should I use for each, and why? Mention how
I keep the data alive (TTL semantics) and what happens if I get it wrong."""

SCORERS = [
    ("mentions_persistent",  lambda t: "persistent" in t.lower()),
    ("mentions_instance",    lambda t: "instance" in t.lower() and "persistent" in t.lower()),  # used as a contrast
    ("mentions_temporary",   lambda t: "temporary" in t.lower()),
    ("mentions_ttl",         lambda t: "ttl" in t.lower() or "time-to-live" in t.lower() or "time to live" in t.lower()),
    ("mentions_extend",      lambda t: "extend" in t.lower() and ("ttl" in t.lower() or "live" in t.lower())),
    ("mentions_archived",    lambda t: "archiv" in t.lower() or "expir" in t.lower() or "evict" in t.lower()),
    ("recommends_persistent_for_balance",
        lambda t: "persistent" in t.lower() and "balance" in t.lower()),
    ("recommends_temporary_for_activity",
        lambda t: ("temporary" in t.lower() and "activity" in t.lower())
                  or ("temporary" in t.lower() and "timestamp" in t.lower())),
]
