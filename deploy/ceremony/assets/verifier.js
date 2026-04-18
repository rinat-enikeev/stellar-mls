// Browser-side verifier entry point.
//
// Loads the ceremony-wasm module once, exposes two high-level helpers:
//
//   await CeremonyVerifier.load()                -> ready-check
//   CeremonyVerifier.verifyRound(tier, round)    -> verify a single round
//     - when round > 1, also fetches round N-1 and runs the full
//       contribution pairing check instead of the standalone state check.
//
// All six pairing equations run in the browser; no coordinator trust.

import init, {
  verify_state_js,
  verify_contribution_js,
  hash_srs_js,
} from '/wasm/ceremony_wasm.js';

let ready = null;

async function load() {
  if (!ready) {
    ready = init('/wasm/ceremony_wasm_bg.wasm').then(() => true);
  }
  return ready;
}

async function fetchBytes(url) {
  const r = await fetch(url, { credentials: 'omit' });
  if (!r.ok) throw new Error(`${url}: HTTP ${r.status}`);
  const buf = await r.arrayBuffer();
  return new Uint8Array(buf);
}

async function fetchText(url) {
  const r = await fetch(url, { credentials: 'omit' });
  if (!r.ok) throw new Error(`${url}: HTTP ${r.status}`);
  return r.text();
}

function artifactUrl(tier, round, name) {
  return `/api/v1/tiers/${tier}/rounds/${round}/artifacts/${name}`;
}

// Parse the JSON string returned by the WASM entry points into a plain
// object. The shape matches VerifyResult in crates/ceremony-wasm/src/lib.rs:
//   { ok, round, tier, srs_hash, error }
function parseResult(json) {
  try {
    return JSON.parse(json);
  } catch (_) {
    return { ok: false, error: 'verifier returned non-JSON: ' + json };
  }
}

async function verifyRound(tier, round) {
  await load();

  const [srs, state, receipt] = await Promise.all([
    fetchBytes(artifactUrl(tier, round, 'state.srs')),
    fetchText(artifactUrl(tier, round, 'state.txt')),
    fetchText(artifactUrl(tier, round, 'receipt.txt')),
  ]);

  if (round <= 1) {
    return {
      mode: 'state',
      result: parseResult(verify_state_js(srs, state, receipt)),
    };
  }

  const prev = round - 1;
  const [prevSrs, prevState, prevReceipt] = await Promise.all([
    fetchBytes(artifactUrl(tier, prev, 'state.srs')),
    fetchText(artifactUrl(tier, prev, 'state.txt')),
    fetchText(artifactUrl(tier, prev, 'receipt.txt')),
  ]);

  return {
    mode: 'contribution',
    result: parseResult(
      verify_contribution_js(prevSrs, prevState, prevReceipt, srs, state, receipt)
    ),
  };
}

async function hashSrs(tier, round) {
  await load();
  const srs = await fetchBytes(artifactUrl(tier, round, 'state.srs'));
  return hash_srs_js(srs);
}

window.CeremonyVerifier = { load, verifyRound, hashSrs };
