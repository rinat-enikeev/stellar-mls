// Shared JS for ceremony.onym.chat
//
// Responsibilities:
//  - Mode toggle (humans / mathematicians) persisted to localStorage.
//  - Nostr NIP-07 detection and NIP-98 Authorization header builder.
//  - Thin wrapper around the coordinator HTTP API.
//
// No bundler; targets modern evergreen browsers only.

(function () {
  'use strict';

  // ---------- Mode toggle ----------
  const MODE_KEY = 'ceremony:mode';
  function currentMode() {
    const hash = location.hash.replace(/^#/, '');
    if (hash === 'math' || hash === 'humans') return hash;
    return localStorage.getItem(MODE_KEY) || 'humans';
  }
  function applyMode(mode) {
    document.body.classList.toggle('mode-math', mode === 'math');
    document.body.classList.toggle('mode-humans', mode === 'humans');
    document.querySelectorAll('.mode-pill button').forEach((b) => {
      b.classList.toggle('active', b.dataset.mode === mode);
    });
  }
  function setMode(mode) {
    localStorage.setItem(MODE_KEY, mode);
    applyMode(mode);
  }
  window.ceremonySetMode = setMode;

  document.addEventListener('DOMContentLoaded', () => {
    applyMode(currentMode());
    document.querySelectorAll('.mode-pill button').forEach((b) => {
      b.addEventListener('click', () => setMode(b.dataset.mode));
    });
  });

  // ---------- API base ----------
  const API_BASE = (() => {
    // Same-origin on prod; fall back to env override for local dev.
    if (window.CEREMONY_API_BASE) return window.CEREMONY_API_BASE;
    return '/api/v1';
  })();

  // ---------- Nostr NIP-07 ----------
  async function ensureNostr() {
    if (!window.nostr) {
      throw new Error(
        'No Nostr extension detected. Install Alby, nos2x, or another NIP-07 provider.'
      );
    }
    return window.nostr;
  }

  async function getPubkey() {
    const n = await ensureNostr();
    return await n.getPublicKey();
  }

  // ---------- NIP-98 Authorization header ----------
  //
  // Builds a kind 27235 event signed via NIP-07 and encodes it as
  //   Authorization: Nostr <base64(event JSON)>
  //
  // If `bodyBytes` is provided, we add a payload tag with the lower-hex
  // SHA-256 of the body, per NIP-98. The coordinator enforces this when
  // the request has a body.
  async function nip98Header(method, url, bodyBytes) {
    const n = await ensureNostr();
    const tags = [
      ['u', url],
      ['method', method.toUpperCase()],
    ];
    if (bodyBytes && bodyBytes.byteLength > 0) {
      const hash = await crypto.subtle.digest('SHA-256', bodyBytes);
      const hex = Array.from(new Uint8Array(hash))
        .map((b) => b.toString(16).padStart(2, '0'))
        .join('');
      tags.push(['payload', hex]);
    }
    const unsigned = {
      kind: 27235,
      created_at: Math.floor(Date.now() / 1000),
      tags,
      content: '',
    };
    const signed = await n.signEvent(unsigned);
    const json = JSON.stringify(signed);
    const b64 = btoa(String.fromCharCode(...new TextEncoder().encode(json)));
    return 'Nostr ' + b64;
  }

  // ---------- API wrappers ----------
  async function apiGet(path) {
    const r = await fetch(API_BASE + path, { credentials: 'omit' });
    if (!r.ok) throw new Error(`${path}: ${r.status} ${await r.text()}`);
    return r.json();
  }

  async function apiPostAuthed(path, body) {
    const url = window.location.origin + API_BASE + path;
    const bytes = new TextEncoder().encode(JSON.stringify(body ?? {}));
    const auth = await nip98Header('POST', url, bytes);
    const r = await fetch(API_BASE + path, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: auth,
      },
      body: bytes,
    });
    if (!r.ok) throw new Error(`${path}: ${r.status} ${await r.text()}`);
    return r.json();
  }

  async function apiUploadContribution(tier, files) {
    const path = `/tiers/${tier}/contribute`;
    const url = window.location.origin + API_BASE + path;
    const auth = await nip98Header('POST', url, null);

    const form = new FormData();
    form.append('state_srs', new Blob([files.state_srs]), 'state.srs');
    form.append('state_txt', new Blob([files.state_txt], { type: 'text/plain' }), 'state.txt');
    form.append('receipt', new Blob([files.receipt], { type: 'text/plain' }), 'receipt.txt');

    const r = await fetch(API_BASE + path, {
      method: 'POST',
      headers: { Authorization: auth },
      body: form,
    });
    if (!r.ok) throw new Error(`upload: ${r.status} ${await r.text()}`);
    return r.json();
  }

  // ---------- SSE status stream ----------
  function subscribeStatus(onUpdate) {
    const es = new EventSource(API_BASE + '/status/stream');
    es.onmessage = (ev) => {
      try {
        onUpdate(JSON.parse(ev.data));
      } catch (e) {
        console.warn('status parse', e);
      }
    };
    es.onerror = () => {
      // Browser auto-reconnects; nothing to do.
    };
    return () => es.close();
  }

  // ---------- Helpers ----------
  function fmtHash(h, n = 8) {
    if (!h) return '—';
    if (h.length <= n * 2) return h;
    return h.slice(0, n) + '…' + h.slice(-n);
  }

  function fmtUnix(t) {
    if (!t) return '—';
    return new Date(t * 1000).toLocaleString();
  }

  function fmtCountdown(deadline) {
    const now = Math.floor(Date.now() / 1000);
    let s = Math.max(0, deadline - now);
    const h = Math.floor(s / 3600);
    s -= h * 3600;
    const m = Math.floor(s / 60);
    s -= m * 60;
    return `${h}h ${m}m ${s}s`;
  }

  window.Ceremony = {
    api: { get: apiGet, postAuthed: apiPostAuthed, uploadContribution: apiUploadContribution },
    nostr: { ensure: ensureNostr, getPubkey, nip98Header },
    status: { subscribe: subscribeStatus },
    fmt: { hash: fmtHash, unix: fmtUnix, countdown: fmtCountdown },
    API_BASE,
  };
})();
